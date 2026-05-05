//! Terminal UI orchestrator. Composes the submodules:
//!
//! - [`view`]: pure view-model — scroll/focus state and the input → state step.
//! - [`draw`]: ratatui frame rendering.
//! - [`term`]: terminal lifecycle + theme palette.
//! - [`input`]: keyboard event thread.
//! - [`edit`]: editor shell-out with full TUI tear-down/rebuild.

mod draw;
mod edit;
mod input;
mod term;
mod view;

use crate::error::{Error, Result};
use crate::state::DiffUpdate;
use crossbeam_channel::{Receiver, TryRecvError, select};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use draw::{diff_inner_width, draw};
use edit::edit_file;
use input::{InputEvent, spawn_input_thread};
use term::{TerminalGuard, make_terminal, palette};
use view::{ViewState, frame, step};

const TICK: Duration = Duration::from_millis(250);

pub fn run(repo_name: &str, repo_root: &Path, updates: Receiver<DiffUpdate>) -> Result<()> {
    // Resolve the palette before raw mode + alt screen — terminal-colorsaurus
    // does its OSC 11 query against a normal-mode tty, and pre-warming the
    // OnceLock here keeps later draw calls allocation-free.
    palette();
    let guard = TerminalGuard::install()?;
    let mut terminal = make_terminal()?;
    let mut state = crate::state::State::new();
    let mut view = ViewState::new();
    let mut sidebar_visible: bool = true;

    let (input_tx, input_rx) = crossbeam_channel::bounded::<InputEvent>(32);
    let stop = Arc::new(AtomicBool::new(false));
    let mut input_handle = Some(spawn_input_thread(input_tx.clone(), Arc::clone(&stop)));

    'main: loop {
        let size = terminal.size().map_err(|e| Error::Term { source: e })?;
        let diff_w = diff_inner_width(size.width, sidebar_visible);
        let viewport_h = size.height.saturating_sub(2); // borders + footer
        let f = frame(&state, &view, viewport_h, diff_w);
        // Sticky-clamp: persist the post-clamp scroll into view so subsequent
        // ScrollDown(n).saturating_add can't accumulate beyond the visible
        // ceiling across many ticks.
        view.scroll = f.scroll;

        terminal
            .draw(|fr| {
                draw(
                    fr,
                    &state,
                    f.scroll,
                    f.focused_idx,
                    sidebar_visible,
                    repo_name,
                )
            })
            .map_err(|e| Error::Term { source: e })?;

        select! {
            recv(updates) -> msg => match msg {
                Ok(update) => {
                    state.apply(update);
                    // Drain any updates already queued so we coalesce a burst
                    // (rescan on a dirty repo emits one DiffUpdate per file)
                    // into a single redraw instead of one redraw per update.
                    loop {
                        match updates.try_recv() {
                            Ok(extra) => state.apply(extra),
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => break 'main,
                        }
                    }
                }
                Err(_) => break 'main,
            },
            recv(input_rx) -> msg => match msg {
                Ok(InputEvent::Quit) => break 'main,
                Ok(InputEvent::Nav(ev)) => {
                    view = step(&state, &view, viewport_h, diff_w, ev);
                }
                Ok(InputEvent::ToggleSidebar) => {
                    sidebar_visible = !sidebar_visible;
                }
                Ok(InputEvent::Edit) => {
                    let Some(rel) = state
                        .iter_ordered()
                        .nth(f.focused_idx)
                        .map(|u| u.path.clone())
                    else {
                        continue;
                    };
                    let abs = repo_root.join(&rel);
                    edit_file(
                        &abs,
                        &guard,
                        &mut terminal,
                        &input_tx,
                        &stop,
                        &mut input_handle,
                    );
                }
                Err(_) => break 'main,
            },
            default(TICK) => {}
        }
    }

    stop.store(true, Ordering::Relaxed);
    if let Some(h) = input_handle.take() {
        let _ = h.join();
    }
    Ok(())
}

/// Escape hatch for `tests/keys.rs`: re-exports the pure view-model so input
/// → state transitions can be driven without a real terminal. Not part of
/// the stable API.
#[doc(hidden)]
pub mod test_harness {
    pub use super::view::{
        Frame, NavEvent, ViewState, content_total_rows, file_offsets, focused_index, frame,
        max_scroll, path_at_scroll, step,
    };
}

/// Escape hatch for `benches/render.rs`. Not part of the stable API.
#[doc(hidden)]
pub mod bench {
    pub use super::draw::render_lines;
    pub use super::view::{content_total_rows, file_offsets};
}
