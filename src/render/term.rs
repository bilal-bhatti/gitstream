//! Terminal lifecycle: theme detection, alt-screen guard, and the ratatui
//! backend handle. All raw-mode / alt-screen state lives here so suspend/
//! resume around editor shell-out has one place to reach.

use crate::error::{Error, Result};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::style::Color;
use std::io::{self, Stdout};
use std::sync::{Mutex, OnceLock};
use terminal_colorsaurus::{QueryOptions, ThemeMode, theme_mode};

pub struct Palette {
    /// Subtle row-highlight bg for the focused sidebar entry.
    pub row_bg: Color,
    /// Foreground for path text on the highlighted row — must contrast with
    /// `row_bg`. White on dark themes, Black on light themes.
    pub highlight_fg: Color,
}

static PALETTE: OnceLock<Palette> = OnceLock::new();

pub fn palette() -> &'static Palette {
    PALETTE.get_or_init(|| {
        let dark = match theme_mode(QueryOptions::default()) {
            Ok(ThemeMode::Light) => false,
            Ok(ThemeMode::Dark) => true,
            Err(e) => {
                tracing::debug!("OSC 11 background query failed; assuming dark: {e}");
                true
            }
        };
        if dark {
            Palette {
                row_bg: Color::Indexed(237),
                highlight_fg: Color::White,
            }
        } else {
            Palette {
                row_bg: Color::Indexed(250),
                highlight_fg: Color::Black,
            }
        }
    })
}

pub fn make_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    let backend = CrosstermBackend::new(io::stdout());
    Terminal::new(backend).map_err(|e| Error::Term { source: e })
}

pub struct TerminalGuard;

static GUARD_INSTALLED: OnceLock<Mutex<bool>> = OnceLock::new();

impl TerminalGuard {
    pub fn install() -> Result<Self> {
        let installed = GUARD_INSTALLED.get_or_init(|| Mutex::new(false));
        let mut flag = installed.lock().expect("guard mutex");
        if *flag {
            return Ok(TerminalGuard);
        }
        enable_raw_mode().map_err(|e| Error::Term { source: e })?;
        execute!(io::stdout(), EnterAlternateScreen).map_err(|e| Error::Term { source: e })?;
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            prev_hook(info);
        }));
        *flag = true;
        Ok(TerminalGuard)
    }

    /// Hand the tty back to a child process — leave alt screen, drop raw mode.
    /// Pair with [`Self::resume`].
    pub fn suspend(&self) -> Result<()> {
        execute!(io::stdout(), LeaveAlternateScreen).map_err(|e| Error::Term { source: e })?;
        disable_raw_mode().map_err(|e| Error::Term { source: e })?;
        Ok(())
    }

    /// Reclaim the tty after the child exits. The panic hook is unaffected
    /// (registered once via `install`), so a panic between suspend and resume
    /// still cleans up.
    pub fn resume(&self) -> Result<()> {
        enable_raw_mode().map_err(|e| Error::Term { source: e })?;
        execute!(io::stdout(), EnterAlternateScreen).map_err(|e| Error::Term { source: e })?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}
