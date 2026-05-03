# Sidebar — Selectable File List

**Status**: Draft
**Last Updated**: 2026-05-02

## Problem

The MVP renders one diff stream, scroll-ordered by file mtime. There is no way to *hold* on a single file's diff while editing continues elsewhere — every save somewhere else in the tree re-sorts the list and pulls the view away from what the user was reading. We want a sidebar that mirrors the same sorted file list, plus a way to pin the diff pane to one file (pausing the auto-follow behavior) and resume live mode with `Esc`. Keyboard navigation should follow vim conventions so the tool feels at home next to a vim/nvim editing session.

## Current State

- MVP plan (`plans/mvp/mvp.md`) is drafted but **not implemented**. Only `src/main.rs` exists as a hello-world stub. This plan assumes the MVP lands first; it layers onto the rendered output, not onto unwritten code.
- Pipeline (watcher → diff → state) is unchanged by this work. Sidebar is a render-layer concern plus two view-state flags on `State`.
- `crossterm` is already in the MVP dependency list. Mouse events come from the same crate — no new dependencies required.
- `ratatui` is already planned. Splitting layout is `Layout::default().direction(Horizontal)…` — no extra surface.

## Approach

Two states for the renderer: **live** (default) and **paused**.

- **Live**: sidebar shows `state.iter_ordered()`, top row is implicitly focused, diff pane renders the top file. New `DiffUpdate`s re-sort the list and the diff pane follows the new top.
- **Paused**: diff pane is pinned to a chosen path. The sidebar keeps re-sorting in the background — we don't lie about ordering — but the diff pane no longer follows the top. Updates to the *selected* path still flow through and re-render that pane.

Entered by mouse click on a sidebar row, or `Enter` on the highlighted row. Exited by `Esc` (snaps focus back to top, resumes live).

The sort, the data model, and the pipeline don't change. The only shared-state additions are two fields:

```rust
pub struct State {
    by_path: HashMap<PathBuf, DiffUpdate>,
    order: Vec<PathBuf>,
    selected: Option<PathBuf>,   // Some(_) ⇒ paused
    highlight: usize,            // cursor row in the sidebar
}
```

Why mutate `State` rather than tracking selection in `render.rs` only: `render::run` already owns `State`. Putting selection on `State` keeps a single source of truth for "what is the user looking at" and lets future features (e.g. a `--select <path>` startup flag) reuse the same field. There's no thread-safety cost — `State` lives on the render thread.

### Rejected alternatives

- **Two separate stores (one for live, one for pinned)**: doubles the data; sort still has to run anywhere we display the list; "follow updates to the selected file" gets awkward across two stores.
- **Implementing pause as "stop applying updates"**: would freeze the sidebar too. Wrong. The list must keep sorting; only the pane should pin.
- **Tab-focus model with `j/k` rebinding by focus**: deferred. Simpler scheme below covers it. Revisit if scroll feel is bad.

## Design Details

### Layout

`ratatui` horizontal split, fixed ratio for MVP.

```
┌─ files ──────┬─ diff ───────────────────────────┐
│ ▸ src/x.rs   │ diff --git a/src/x.rs b/src/x.rs │
│   src/y.rs   │ @@ -1,3 +1,4 @@                  │
│ ●●src/z.rs   │  fn main() {                     │
│   …          │ +    log("hi");                  │
└──────────────┴──────────────────────────────────┘
```

- Left pane: 30% width, min 24 cols, max 48 cols (clamp). Single line per file: `{symbol} {path}  +N -N`.
- Right pane: existing diff renderer (unchanged from MVP).
- Status bar at bottom: `live` or `paused: src/z.rs — esc to resume`.

### Visual affordances

Two distinct things to show:

| State | Marker |
|------|--------|
| Highlighted (cursor row) | reverse video on the row |
| Selected (pinned, paused) | leading `●` glyph in column 0, bold path |

These can co-occur (cursor sits on the selected row). Cursor and selection are independent — `j`/`k` moves the cursor without changing the selection.

### Input handling

```
key       live mode                         paused mode
─────────────────────────────────────────────────────────
j / ↓     highlight ++                      highlight ++
k / ↑     highlight --                      highlight --
g g       highlight = 0                     highlight = 0
G         highlight = order.len() - 1       highlight = order.len() - 1
Enter     select highlighted → paused       select highlighted (re-pin)
Esc       (no-op)                           selected = None → live
Ctrl-d    diff pane: half-page down         diff pane: half-page down
Ctrl-u    diff pane: half-page up           diff pane: half-page up
PgDn/PgUp diff pane: full-page              diff pane: full-page
q / C-c   quit                              quit
```

`gg` is a two-key sequence — use a 500ms pending-key timer; any other key cancels.

### Mouse

- Enable `EnableMouseCapture` in the terminal guard; disable on shutdown.
- Single left-click inside the sidebar pane: hit-test against rendered row rects (track them when drawing each frame), set `highlight` to that row, set `selected = Some(path)`. Equivalent to `j/k` to that row + `Enter`.
- Clicks outside the sidebar pane: ignored in MVP.
- Scroll wheel: defer (would otherwise need to choose between sidebar scroll and diff scroll based on cursor position — non-trivial).

### Render flow

```rust
// render.rs (sketch — additions only)

fn draw(frame: &mut Frame, state: &State, view: &View) {
    let [left, right] = horizontal_split(frame.area(), 30);
    draw_sidebar(frame, left, state, view.highlight);
    let pinned = state.selected.as_deref().or_else(|| state.order.first().map(|p| p.as_path()));
    draw_diff(frame, right, state, pinned, view.diff_scroll);
    draw_status(frame, &state.selected);
}
```

`View` holds transient render state that doesn't belong on the model: `highlight: usize`, `diff_scroll: u16`, `pending_g: Option<Instant>` for `gg`, `row_rects: Vec<Rect>` for click hit-test.

### Edge cases

- **`order` shrinks below `highlight`**: clamp on each frame to `order.len().saturating_sub(1)`.
- **`order` is empty** (clean tree): sidebar shows a single `(no changes)` line; diff pane shows the same. `j/k/Enter` are no-ops.
- **Selected file's diff becomes empty** (user reverts): drop the selection, return to live mode, leave a one-tick status message ("`src/z.rs` clean — resumed live"). This avoids a stuck "paused on nothing" state.
- **Selected file is deleted from `order`** for any other reason: same as above.
- **Sidebar overflow** (`order.len()` > pane height): scroll the visible window to keep `highlight` in view. No scrollbar widget — just an off-screen-count hint at the top/bottom of the pane (`↑ 3 more`).

## Impact

| File | Action | Change |
|------|--------|--------|
| `src/state.rs` | Modify | Add `selected: Option<PathBuf>` and `highlight: usize` to `State`; add `set_selected`, `clear_selected`, `move_highlight` helpers. Clamp `highlight` and reconcile `selected` (drop if path no longer in `order`) on every `apply` / `drop_if_clean`. |
| `src/render.rs` | Modify | Replace single-pane draw with horizontal split. Add `View` struct (highlight, diff_scroll, pending_g, row_rects). Vim keymap. Mouse hit-test. Status bar. |
| `src/app.rs` | Modify | Wrap terminal init with `EnableMouseCapture` / `DisableMouseCapture`. No new threads, no channel changes. |
| `src/main.rs` | Modify | Add `--no-mouse` flag (off → keep mouse capture on by default). Pass through to `app::run`. |
| `tests/sidebar.rs` | New | Drives `State` through `apply` sequences with selection set/cleared; asserts `selected` reconciles correctly when paths leave `order`. No TUI in this test. |

Five files. No new modules, no new dependencies.

### Refactor opportunity (do during sidebar work, not before)

Today `src/render.rs` houses pure-draw functions, the input-listener thread, the `Event → InputEvent` translation, and the dispatch loop in `run()`. At ~280 LOC that's still tractable, but the sidebar work roughly doubles the input surface (mouse hit-tests, `gg`/chord sequences, focus toggle, paused vs live keymaps). When that lands, split:

- `src/input.rs` — `InputEvent`, the listener thread, `translate(Event) → InputEvent` (and its chord/timer state).
- `src/render.rs` — pure `draw` + helpers (`render_lines`, `render_file`, `separator_line`, `file_offsets`) + `TerminalGuard`. No I/O, no key handling.
- The `run()` loop (state ownership + `select!` + dispatch) stays where it is, just imports from both.

Don't pre-split for the MVP — it's premature given a single consumer. Trigger is "I'm adding the second keymap" or "the input thread needs chord state."

## Risks

- **Mouse capture breaks terminal shift-select-to-copy.** Users who rely on copying diff text from the terminal will hit this immediately. *Mitigation*: `--no-mouse` opt-out, documented in `--help`. Most modern terminals also offer a modifier (Option on iTerm2, Shift on most Linux terminals) to override mouse capture and select natively — call this out in README.
- **`gg` two-key sequence collides with anything starting in `g`.** Nothing else starts with `g` in this keymap, but if/when one is added the timer model needs to grow. *Mitigation*: keep the keymap small; revisit if a `g`-prefixed family appears.
- **Pinning the diff pane while the sidebar reorders feels jumpy.** The cursor row may "move under" the user when a different file gets a save and jumps to the top. *Mitigation*: when paused, also pin `highlight` to the selected row (keep cursor on the selection rather than on the row index). Document and confirm during impl — could go either way.
- **Hit-test rects go stale between draw and event.** If we cache rects on the frame and use them to interpret the next click, a re-sort between the two could land the click on the wrong row. *Mitigation*: re-resolve clicks by row index → `state.order[index]`, and accept the user clicking the visually-correct row even if `order` changed underneath. Consistent with what the user saw.

## Validation

- [ ] `cargo build` and `cargo clippy -- -D warnings` clean.
- [ ] Live mode unchanged from MVP behavior: edits push the changed file to top of sidebar and diff pane follows.
- [ ] `j`/`k` move highlight without changing selection or pausing.
- [ ] `Enter` on a highlighted row pauses; status bar shows the pinned path.
- [ ] While paused, edits to *other* files still re-sort the sidebar but the diff pane stays put.
- [ ] While paused, edits to the *selected* file update the pinned diff in place.
- [ ] `Esc` resumes live mode; cursor snaps to top.
- [ ] Single click on a sidebar row is equivalent to `j/k` + `Enter` to that row.
- [ ] Reverting all changes to the selected file drops the selection and surfaces a transient status message.
- [ ] `--no-mouse` disables mouse capture; clicks have no effect; everything else still works.
- [ ] `tests/sidebar.rs` exercises `State` selection reconciliation across `apply`/`drop_if_clean`.
- [ ] `gg` works; pressing `g` then any non-`g` key within 500ms cancels.
- [ ] `q` / `Ctrl-C` still quits and restores terminal cleanly (mouse capture disabled on exit).

## Implementation Notes
[Updated during implementation. Record deviations, discoveries, decisions made.]
