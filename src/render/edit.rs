//! Editor shell-out. Tears down the input thread + alt screen before spawning
//! the child process, rebuilds them after. Per the global TUI guidance: a
//! pause flag won't unblock `event::read()`, so the only safe yield is full
//! teardown.

use crate::render::input::{InputEvent, spawn_input_thread};
use crate::render::term::TerminalGuard;
use crossbeam_channel::Sender;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::Stdout;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

/// Suspend the TUI, run the user's editor on `path`, then rebuild.
///
/// Tear-down (drop input thread, leave alt screen, disable raw mode) is the
/// only safe way to yield the tty — `event::read()` won't unblock to check a
/// pause flag, so the input thread would race the editor for keystrokes.
pub fn edit_file(
    path: &Path,
    guard: &TerminalGuard,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    input_tx: &Sender<InputEvent>,
    stop: &Arc<AtomicBool>,
    input_handle: &mut Option<JoinHandle<()>>,
) {
    stop.store(true, Ordering::Relaxed);
    if let Some(h) = input_handle.take() {
        let _ = h.join();
    }
    if let Err(e) = guard.suspend() {
        tracing::error!(error = %e, "tui suspend failed");
    }

    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("EDITOR")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".to_string());
    let mut parts = editor.split_whitespace();
    let prog = parts.next().unwrap_or("vi");
    let extra: Vec<&str> = parts.collect();

    let result = Command::new(prog).args(&extra).arg(path).status();
    if let Err(e) = result {
        tracing::error!(editor = %editor, path = %path.display(), error = %e, "editor spawn failed");
    }

    if let Err(e) = guard.resume() {
        tracing::error!(error = %e, "tui resume failed");
    }
    let _ = terminal.clear();
    stop.store(false, Ordering::Relaxed);
    *input_handle = Some(spawn_input_thread(input_tx.clone(), Arc::clone(stop)));
}
