//! Pure view-model: scroll/focus state and the input → state transition.
//!
//! Everything here is a pure function of `State`, the current `ViewState`,
//! and viewport geometry — no terminal I/O, no threads. Drives both the
//! production loop in [`super::run`] and the keyboard test harness.

use crate::state::{ChangeKind, DiffUpdate, HunkLine, State};
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthStr;

/// User-facing scroll/focus state. The renderer derives a [`Frame`] from this
/// each tick; input handlers produce a new `ViewState` via [`step`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewState {
    pub scroll: u16,
    pub focused_path: Option<PathBuf>,
}

impl ViewState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Navigation intents — the subset of input events that affect view state.
/// Quit/Edit/ToggleSidebar are handled outside the pure step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavEvent {
    ScrollUp(u16),
    ScrollDown(u16),
    Top,
    NextFile,
    PrevFile,
}

/// Per-tick derived view: clamped scroll, resolved focus index, and the
/// current scroll ceiling.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub scroll: u16,
    pub focused_idx: usize,
    pub max_scroll: u16,
}

pub fn frame(state: &State, view: &ViewState, viewport_h: u16, diff_w: u16) -> Frame {
    let max_scroll = max_scroll(state, viewport_h, diff_w);
    let scroll = view.scroll.min(max_scroll);
    let focused_idx = focused_index(state, view.focused_path.as_deref());
    Frame {
        scroll,
        focused_idx,
        max_scroll,
    }
}

/// Apply a navigation event to the view. Reads the clamped frame, then writes
/// new (scroll, focused_path). Pure — no global state.
pub fn step(
    state: &State,
    view: &ViewState,
    viewport_h: u16,
    diff_w: u16,
    ev: NavEvent,
) -> ViewState {
    let f = frame(state, view, viewport_h, diff_w);
    let offsets = file_offsets(state, diff_w);
    let mut next = view.clone();
    match ev {
        NavEvent::ScrollDown(n) => {
            next.scroll = f.scroll.saturating_add(n);
            next.focused_path = path_at_scroll(state, diff_w, next.scroll);
        }
        NavEvent::ScrollUp(n) => {
            next.scroll = f.scroll.saturating_sub(n);
            next.focused_path = path_at_scroll(state, diff_w, next.scroll);
        }
        NavEvent::Top => {
            next.scroll = 0;
            next.focused_path = state.order().first().cloned();
        }
        NavEvent::NextFile => {
            let cur = f.focused_idx;
            if cur + 1 < state.len() {
                next.focused_path = state.order().get(cur + 1).cloned();
                next.scroll = offsets.get(cur + 1).copied().unwrap_or(next.scroll);
            }
        }
        NavEvent::PrevFile => {
            // First press inside a file rewinds to its top; second moves up a file.
            let cur_offset = offsets.get(f.focused_idx).copied().unwrap_or(0);
            if f.scroll > cur_offset {
                next.scroll = cur_offset;
            } else if f.focused_idx > 0 {
                let prev = f.focused_idx - 1;
                next.focused_path = state.order().get(prev).cloned();
                next.scroll = offsets.get(prev).copied().unwrap_or(next.scroll);
            }
        }
    }
    next
}

/// Resolve a tracked path to its current index in the ordered file list.
/// Falls back to 0 when the path is unset or has been dropped from state.
pub fn focused_index(state: &State, focused_path: Option<&Path>) -> usize {
    let Some(path) = focused_path else { return 0 };
    state
        .order()
        .iter()
        .position(|p| p.as_path() == path)
        .unwrap_or(0)
}

/// File at the top of the viewport for a given scroll offset — the largest
/// file offset that's still ≤ scroll. Used to re-derive focus when the user
/// scrolls with j/k/u/d.
pub fn path_at_scroll(state: &State, diff_w: u16, scroll: u16) -> Option<PathBuf> {
    let offsets = file_offsets(state, diff_w);
    let idx = offsets.iter().rposition(|&o| o <= scroll).unwrap_or(0);
    state.order().get(idx).cloned()
}

/// Maximum reachable scroll position.
///
/// Naive bound `content_total - viewport_h` keeps the diff pane "full" but
/// silently clamps any scroll that would put a tail-visible file at the top
/// of the viewport — n's jump to such a file lands the focus indicator on the
/// file while the post-clamp scroll keeps showing an earlier file's tail,
/// producing a perceived desync. The looser bound `max(naive, last_offset)`
/// guarantees every file's offset is a reachable scroll position so n/b
/// always anchor the focused file at the top.
pub fn max_scroll(state: &State, viewport_h: u16, diff_w: u16) -> u16 {
    let naive = content_total_rows(state, diff_w).saturating_sub(viewport_h);
    let last = file_offsets(state, diff_w).last().copied().unwrap_or(0);
    naive.max(last)
}

pub fn file_offsets(state: &State, diff_width: u16) -> Vec<u16> {
    let mut offsets = Vec::with_capacity(state.len());
    let mut cur: u32 = 0;
    for (i, update) in state.iter_ordered().enumerate() {
        if i > 0 {
            cur = cur.saturating_add(2); // empty line + separator
        }
        offsets.push(cur.min(u16::MAX as u32) as u16);
        cur = cur.saturating_add(file_visual_rows(update, diff_width));
    }
    offsets
}

pub fn content_total_rows(state: &State, diff_width: u16) -> u16 {
    let mut total: u32 = 0;
    for (i, update) in state.iter_ordered().enumerate() {
        if i > 0 {
            total = total.saturating_add(2);
        }
        total = total.saturating_add(file_visual_rows(update, diff_width));
    }
    total.min(u16::MAX as u32) as u16
}

pub fn file_visual_rows(update: &DiffUpdate, width: u16) -> u32 {
    let mut n: u32 = 1; // header line (truncated to fit in 1 row by the renderer)
    if update.binary {
        return n + 1;
    }
    if update.hunks.is_empty() && !matches!(update.status, ChangeKind::Deleted) {
        return n + 1;
    }
    let content_w = width.saturating_sub(4); // 4-cell prefix: "    ", "  + ", "  - "
    for hunk in &update.hunks {
        n = n.saturating_add(1); // @@ header (assumed to fit; typical < 30 cells)
        for line in &hunk.lines {
            let content = match line {
                HunkLine::Context(s) | HunkLine::Added(s) | HunkLine::Removed(s) => s.as_str(),
            };
            n = n.saturating_add(diff_line_rows(content, content_w));
        }
    }
    n
}

/// Visual rows a single diff line takes after pre-wrapping at `content_w`
/// display cells (the area available after the 4-cell prefix). Pairs with
/// [`wrap_at`] — same chunk count.
pub fn diff_line_rows(content: &str, content_w: u16) -> u32 {
    let max = content_w.max(1) as usize;
    let cells = UnicodeWidthStr::width(content);
    if cells == 0 {
        return 1;
    }
    cells.div_ceil(max) as u32
}

/// Split `content` into substrings whose display width is ≤ `max_cells` each.
/// Borrows from `content` — no allocation. Always returns at least one slice
/// (an empty content yields one empty slice so the renderer emits one row).
///
/// Char-bounded, not word-bounded — matches `diff_line_rows` exactly so
/// `file_visual_rows` and the post-wrap line count agree. Word wrap looks
/// nicer for prose but breaks the count contract on minified / no-whitespace
/// content (a single ~50KB minified JS line wraps short of `width` because
/// WordWrapper flushes at the last word boundary).
pub fn wrap_at(content: &str, max_cells: u16) -> Vec<&str> {
    let max = max_cells.max(1) as usize;
    let mut out: Vec<&str> = Vec::new();
    let mut chunk_start = 0;
    let mut chunk_cells = 0usize;
    for (i, ch) in content.char_indices() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if chunk_cells + w > max && chunk_cells > 0 {
            out.push(&content[chunk_start..i]);
            chunk_start = i;
            chunk_cells = w;
        } else {
            chunk_cells += w;
        }
    }
    if chunk_start < content.len() || out.is_empty() {
        out.push(&content[chunk_start..]);
    }
    out
}
