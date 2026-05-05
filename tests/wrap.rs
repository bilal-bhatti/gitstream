//! Verifies that `view::file_offsets` and `view::content_total_rows` agree
//! with ratatui's actual wrap output. A mismatch here means n/b would jump
//! to a stale scroll position in the rendered diff pane.
//!
//! Specifically targets the dash-repo scenario: a deleted minified-JS file
//! with one ~50KB line, where any ceil(N/W) approximation diverges from
//! WordWrapper's actual line count.

use std::path::PathBuf;
use std::time::SystemTime;

use gitstream::render::bench::render_lines;
use gitstream::render::test_harness::{
    NavEvent, ViewState, content_total_rows, file_offsets, frame, step,
};
use gitstream::state::{ChangeKind, DiffUpdate, Hunk, HunkLine, State};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};

const DIFF_W: u16 = 80;

fn mk_modified(name: &str, mtime: u64, lines: Vec<HunkLine>) -> DiffUpdate {
    let added = lines
        .iter()
        .filter(|l| matches!(l, HunkLine::Added(_)))
        .count() as u32;
    let removed = lines
        .iter()
        .filter(|l| matches!(l, HunkLine::Removed(_)))
        .count() as u32;
    DiffUpdate {
        path: PathBuf::from(name),
        mtime: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(mtime),
        status: ChangeKind::Modified,
        hunks: vec![Hunk {
            old_range: (0, 0),
            new_range: (0, lines.len() as u32),
            lines,
        }],
        added,
        removed,
        binary: false,
    }
}

fn mk_deleted(name: &str, mtime: u64, removed_lines: Vec<String>) -> DiffUpdate {
    let removed = removed_lines.len() as u32;
    let lines: Vec<HunkLine> = removed_lines.into_iter().map(HunkLine::Removed).collect();
    DiffUpdate {
        path: PathBuf::from(name),
        mtime: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(mtime),
        status: ChangeKind::Deleted,
        hunks: vec![Hunk {
            old_range: (0, removed),
            new_range: (0, 0),
            lines,
        }],
        added: 0,
        removed,
        binary: false,
    }
}

/// Render the diff content (no Paragraph::block, no scroll, no border) onto a
/// tall buffer. Returns the buffer so the caller can locate each file's
/// header row by scanning for the badge text. No `Wrap` — production renders
/// without ratatui wrapping; pre-wrapped Lines are emitted 1 row each.
fn render_to_buffer(state: &State, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    let lines: Vec<Line<'_>> = render_lines(state, width);
    let para = Paragraph::new(lines);
    para.render(area, &mut buf);
    buf
}

/// Find the y of the row whose content contains `needle`. Returns the first
/// match scanning top-to-bottom, or panics — the caller picks needles that
/// are unique enough to not collide.
fn find_row(buf: &Buffer, needle: &str) -> u16 {
    for y in 0..buf.area.height {
        let mut row = String::new();
        for x in 0..buf.area.width {
            row.push_str(buf[(x, y)].symbol());
        }
        if row.contains(needle) {
            return y;
        }
    }
    panic!("needle {needle:?} not found in buffer");
}

/// Mimic the dash repo's deleted minified JS file: one giant `Removed` line
/// of ~48KB. Verify that the next file's header lands at exactly
/// `file_offsets()[1]` in the rendered buffer.
#[test]
fn deleted_minified_js_offsets_match_ratatui_wrap() {
    let big_js = "var htmx=function(){\"use strict\";const Q={onLoad:null,process:null,on:null};".repeat(640);
    assert!(big_js.len() > 40_000, "test setup: expect ~50KB single line");

    let mut state = State::new();
    state.apply(mk_deleted("htmx.min.js", 100, vec![big_js]));
    state.apply(mk_modified(
        "small.txt",
        50,
        vec![
            HunkLine::Context("ctx 1".into()),
            HunkLine::Added("added 1".into()),
            HunkLine::Context("ctx 2".into()),
        ],
    ));

    let offsets = file_offsets(&state, DIFF_W);

    // Use the absolute u16 max — if the deleted file wraps to far more rows
    // than our content_total_rows predicted, a tight bound would put MODIFIED
    // past the buffer end and we'd misread the failure as "needle missing"
    // rather than "wrap miscount".
    let render_height = u16::MAX;
    let buf = render_to_buffer(&state, DIFF_W, render_height);

    // First file's header.
    let row_deleted = find_row(&buf, "DELETED");
    assert_eq!(
        row_deleted as u16, offsets[0],
        "deleted-file header at {row_deleted} but offsets[0]={}",
        offsets[0]
    );

    // Second file's header — the one that breaks if our row count for the
    // huge wrapped file diverges from ratatui's actual output.
    let row_modified = find_row(&buf, "MODIFIED");
    assert_eq!(
        row_modified as u16, offsets[1],
        "second file's header at row {row_modified} but file_offsets says {} — \
         ratatui wrapped the deleted file into a different number of rows than \
         file_visual_rows predicted",
        offsets[1]
    );
}

/// End-to-end repro of the dash-repo symptom: deleted minified-JS at the top,
/// user presses `n` to focus the next file, and the diff pane must show that
/// file's header at the top of the viewport — not several rows down inside
/// the prior file's wrapped content. Drives the same step/frame seam that the
/// production loop uses.
#[test]
fn n_past_huge_minified_file_anchors_next_file_at_viewport_top() {
    let big_js = "var htmx=function(){\"use strict\";const Q={onLoad:null,process:null,on:null};".repeat(640);
    let mut state = State::new();
    state.apply(mk_deleted("htmx.min.js", 100, vec![big_js]));
    state.apply(mk_modified(
        "small.txt",
        50,
        vec![
            HunkLine::Context("ctx 1".into()),
            HunkLine::Added("added 1".into()),
            HunkLine::Context("ctx 2".into()),
        ],
    ));

    let viewport_h: u16 = 60;
    let view = ViewState::new();
    let view = step(&state, &view, viewport_h, DIFF_W, NavEvent::NextFile);
    let f = frame(&state, &view, viewport_h, DIFF_W);
    let offsets = file_offsets(&state, DIFF_W);

    assert_eq!(f.focused_idx, 1);
    assert_eq!(
        f.scroll, offsets[1],
        "diff pane scroll must equal small.txt's offset; otherwise the user \
         sees minified JS at the top while sidebar/title say small.txt"
    );

    // Render and assert the file header lands exactly at the top of the
    // viewport (row 0 within the windowed area starting at f.scroll).
    let render_height = u16::MAX;
    let buf = render_to_buffer(&state, DIFF_W, render_height);
    let row_modified = find_row(&buf, "MODIFIED");
    assert_eq!(
        row_modified as u16, f.scroll,
        "rendered MODIFIED header at row {row_modified} but diff pane scrolled \
         to {} — these must match for the focused file to be at viewport top",
        f.scroll
    );
}

/// Sanity baseline: small files with normal-length lines should already match.
/// If this fails, the harness itself is wrong, not our wrap accounting.
#[test]
fn normal_files_offsets_match_ratatui_wrap() {
    let mut state = State::new();
    state.apply(mk_modified(
        "a.txt",
        100,
        vec![
            HunkLine::Context("alpha".into()),
            HunkLine::Added("BETA".into()),
            HunkLine::Context("gamma".into()),
        ],
    ));
    state.apply(mk_modified(
        "b.txt",
        50,
        vec![
            HunkLine::Context("one".into()),
            HunkLine::Removed("two".into()),
            HunkLine::Context("three".into()),
        ],
    ));

    let offsets = file_offsets(&state, DIFF_W);
    let total = content_total_rows(&state, DIFF_W);

    let buf = render_to_buffer(&state, DIFF_W, total + 5);
    let row_a = find_row(&buf, "a.txt");
    assert_eq!(row_a as u16, offsets[0]);
    let row_b = find_row(&buf, "b.txt");
    assert_eq!(row_b as u16, offsets[1]);
}
