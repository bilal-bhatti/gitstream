//! Keyboard / view-model integration tests. Drives the pure `step` + `frame`
//! seam exposed via `render::test_harness` — no terminal, no threads.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use gitstream::render::test_harness::{
    NavEvent, ViewState, content_total_rows, file_offsets, frame, step,
};
use gitstream::state::{ChangeKind, DiffUpdate, Hunk, HunkLine, State};

const DIFF_W: u16 = 200; // wide enough that no diff line wraps
const VIEWPORT_H: u16 = 60;

fn mk_file(name: &str, mtime_offset: u64, lines: usize) -> DiffUpdate {
    let hunk_lines: Vec<HunkLine> = (0..lines)
        .map(|i| HunkLine::Added(format!("line {i:04}")))
        .collect();
    DiffUpdate {
        path: PathBuf::from(name),
        mtime: SystemTime::UNIX_EPOCH + Duration::from_secs(mtime_offset),
        status: ChangeKind::Modified,
        hunks: vec![Hunk {
            old_range: (0, 0),
            new_range: (0, lines as u32),
            lines: hunk_lines,
        }],
        added: lines as u32,
        removed: 0,
        binary: false,
    }
}

/// State sorts by mtime DESC, so we hand out descending mtimes in declaration
/// order — `files[0]` lands at index 0, `files[1]` at index 1, etc.
fn build_state(files: &[(&str, usize)]) -> State {
    let mut state = State::new();
    let n = files.len() as u64;
    for (i, (name, lines)) in files.iter().enumerate() {
        let mtime_off = n - i as u64;
        state.apply(mk_file(name, mtime_off, *lines));
    }
    state
}

/// Regression: pressing `n` to a file whose offset sits in the "tail-visible"
/// zone (offset > content_total - viewport_h) used to silently clamp the
/// scroll position back, while the sidebar/title still indicated the user's
/// selection. Result: focus indicator and diff content desynced — sidebar
/// said "you're on D" but the pane still showed B at the top.
///
/// Invariant we want: after `n` lands on a file, the post-clamp scroll equals
/// that file's offset, so the file is anchored at the top of the diff pane.
#[test]
fn n_anchors_tail_visible_file_at_top_of_viewport() {
    // a:50 → 52 rows, b:30 → 32, c:20 → 22, d:10 → 12, e:5 → 7. Total = 133.
    // viewport_h = 60. Naive max_scroll = 73. Offsets: a=0, b=54, c=88, d=112,
    // e=126 — c, d, e are tail-visible.
    let state = build_state(&[("a", 50), ("b", 30), ("c", 20), ("d", 10), ("e", 5)]);
    let offsets = file_offsets(&state, DIFF_W);
    let total = content_total_rows(&state, DIFF_W);
    let naive_max = total.saturating_sub(VIEWPORT_H);

    // Test setup precondition: c's offset must exceed the naive bound,
    // otherwise the bug doesn't trigger and the test isn't testing what we
    // think it is.
    assert!(
        offsets[2] > naive_max,
        "test setup: c (offset {}) must be tail-visible vs naive_max {}",
        offsets[2],
        naive_max
    );

    let mut view = ViewState::new();
    view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::NextFile); // -> b
    view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::NextFile); // -> c
    let f = frame(&state, &view, VIEWPORT_H, DIFF_W);

    assert_eq!(f.focused_idx, 2, "n n should focus c");
    assert_eq!(
        f.scroll, offsets[2],
        "diff pane must anchor c at the top — scroll={} offsets[c]={}",
        f.scroll, offsets[2]
    );
}

/// Invariant: walking forward through every file with `n`, then backward with
/// `b` from a file's top, the post-clamp scroll equals the focused file's
/// offset at each step. This is the cross-pane sync contract — sidebar
/// highlight and diff content always describe the same file.
#[test]
fn n_b_round_trip_keeps_focus_synced_with_diff_pane() {
    let state = build_state(&[("a", 50), ("b", 30), ("c", 20), ("d", 10), ("e", 5)]);
    let offsets = file_offsets(&state, DIFF_W);
    let mut view = ViewState::new();

    let n = state.len();

    // Forward: a → b → c → d → e.
    for (i, &offset) in offsets.iter().enumerate() {
        let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
        assert_eq!(f.focused_idx, i, "forward step {i}: focused_idx mismatch");
        assert_eq!(
            f.scroll, offset,
            "forward step {i}: scroll {} should anchor file at offset {}",
            f.scroll, offset
        );
        if i + 1 < n {
            view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::NextFile);
        }
    }

    // Backward: e → d → c → b → a. Each `b` press is at the file's top, so it
    // moves up one file rather than rewinding within.
    for (i, &offset) in offsets.iter().enumerate().rev() {
        let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
        assert_eq!(f.focused_idx, i, "backward step {i}: focused_idx mismatch");
        assert_eq!(
            f.scroll, offset,
            "backward step {i}: scroll {} should anchor file at offset {}",
            f.scroll, offset
        );
        if i > 0 {
            view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::PrevFile);
        }
    }
}

/// `b` is two-step: first press inside a file rewinds to its top, second
/// moves up a file. Setup needs a non-last focused file so j has room to
/// scroll into its body — when the last file fits in viewport, max_scroll
/// caps at its offset and j-from-its-top is correctly a no-op.
#[test]
fn b_inside_file_rewinds_to_top_then_moves_up() {
    let state = build_state(&[("a", 50), ("b", 50), ("c", 50)]);
    let offsets = file_offsets(&state, DIFF_W);
    let mut view = ViewState::new();

    view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::NextFile); // -> b
    view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::ScrollDown(5));
    let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
    assert!(
        f.scroll > offsets[1],
        "scrolled into b's body — scroll {} should exceed offsets[b] {}",
        f.scroll,
        offsets[1]
    );
    assert_eq!(f.focused_idx, 1);

    // First b: rewinds to b's top.
    view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::PrevFile);
    let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
    assert_eq!(f.scroll, offsets[1], "first b rewinds to b's top");
    assert_eq!(f.focused_idx, 1);

    // Second b: now at b's top, moves up to a.
    view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::PrevFile);
    let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
    assert_eq!(f.scroll, offsets[0], "second b moves to a's top");
    assert_eq!(f.focused_idx, 0);
}

/// `g` jumps to top: focused file becomes order[0], scroll resets to 0.
#[test]
fn g_jumps_to_first_file() {
    let state = build_state(&[("a", 50), ("b", 30), ("c", 20)]);
    let mut view = ViewState::new();
    view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::NextFile); // -> b
    view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::NextFile); // -> c
    view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::Top);
    let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
    assert_eq!(f.focused_idx, 0);
    assert_eq!(f.scroll, 0);
}

/// j/k re-derive focus from scroll position, so as the user scrolls past a
/// file boundary the sidebar highlight follows.
#[test]
fn j_k_focus_tracks_path_at_scroll() {
    let state = build_state(&[("a", 50), ("b", 30), ("c", 20)]);
    let offsets = file_offsets(&state, DIFF_W);
    let mut view = ViewState::new();
    let f0 = frame(&state, &view, VIEWPORT_H, DIFF_W);
    assert_eq!(f0.focused_idx, 0);

    // Scroll just past a/b boundary — focus should jump to b.
    view = step(
        &state,
        &view,
        VIEWPORT_H,
        DIFF_W,
        NavEvent::ScrollDown(offsets[1]),
    );
    let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
    assert_eq!(f.focused_idx, 1, "focus follows scroll into b");

    // Scroll back up — focus returns to a.
    view = step(
        &state,
        &view,
        VIEWPORT_H,
        DIFF_W,
        NavEvent::ScrollUp(offsets[1]),
    );
    let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
    assert_eq!(f.focused_idx, 0, "focus follows scroll back to a");
}

/// State updates can reorder files (mtime DESC). focused_path is path-keyed,
/// so it must continue tracking the same file through a resort — n must move
/// from the file's *new* position, not where it was before the reorder.
#[test]
fn focused_path_survives_state_resort() {
    let mut state = build_state(&[("a", 50), ("b", 30), ("c", 20)]);
    let mut view = ViewState::new();
    view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::NextFile); // -> b

    let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
    assert_eq!(f.focused_idx, 1);

    // Bump c's mtime so it sorts to the top: order becomes [c, a, b].
    state.apply(mk_file("c", 9999, 20));

    let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
    assert_eq!(f.focused_idx, 2, "focused_path tracks b through resort");
    assert_eq!(state.order()[f.focused_idx], PathBuf::from("b"));

    // n from b (now last) is a no-op.
    view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::NextFile);
    let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
    assert_eq!(f.focused_idx, 2, "n at last file is a no-op");
}

/// Even when content fits in the viewport (naive max_scroll = 0), n still
/// anchors the focused file at the top of the viewport rather than leaving
/// scroll pinned at 0 with the focus indicator out of sync.
#[test]
fn n_anchors_focus_when_content_fits_in_viewport() {
    // Three small files: total well under viewport_h.
    let state = build_state(&[("a", 5), ("b", 5), ("c", 5)]);
    let offsets = file_offsets(&state, DIFF_W);
    let total = content_total_rows(&state, DIFF_W);
    assert!(
        total < VIEWPORT_H,
        "test setup: content must fit in viewport"
    );

    let mut view = ViewState::new();
    view = step(&state, &view, VIEWPORT_H, DIFF_W, NavEvent::NextFile); // -> b
    let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
    assert_eq!(f.focused_idx, 1);
    assert_eq!(
        f.scroll, offsets[1],
        "scroll must reach b's offset even when content fits"
    );
}

/// Empty state — n/b/g must be safe no-ops, frame must not panic.
#[test]
fn nav_on_empty_state_is_a_safe_noop() {
    let state = State::new();
    let mut view = ViewState::new();
    let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
    assert_eq!(f.focused_idx, 0);
    assert_eq!(f.scroll, 0);

    for ev in [
        NavEvent::NextFile,
        NavEvent::PrevFile,
        NavEvent::Top,
        NavEvent::ScrollDown(10),
        NavEvent::ScrollUp(10),
    ] {
        view = step(&state, &view, VIEWPORT_H, DIFF_W, ev);
        let f = frame(&state, &view, VIEWPORT_H, DIFF_W);
        assert_eq!(f.focused_idx, 0);
        assert_eq!(f.scroll, 0);
    }
}
