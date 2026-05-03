# Sidebar — File List

**Status**: Implemented
**Last Updated**: 2026-05-03

## Problem

The diff pane renders one stream, scroll-ordered by file mtime. Without a sidebar there's no at-a-glance view of which files are in flight or where the current scroll position lands within them. A sidebar fixes both: a list of changed files in the same sort order, with the file under the current scroll offset visually marked.

## Approach

A toggleable left pane that mirrors `state.iter_ordered()`. The diff pane is unchanged: one scrolling stream of all files concatenated and separated by horizontal rules. As the user scrolls, the sidebar's cursor glyph moves to mark which file's content is at the top of the viewport.

The sort, the data model, and the pipeline don't change. The cursor index is derived per frame from scroll position; nothing is stored on `State`.

## Design Details

### Layout

`ratatui` horizontal split.

```
┌─ files ──────┬─ diff ───────────────────────────┐
│ ▎M  src/x.rs │ MODIFIED  src/x.rs  +1 -0        │
│       +1 -0  │   @@ -1,3 +1,4 @@                │
│  M  src/y.rs │    fn main() {                   │
│       +0 -2  │ +    log("hi");                  │
└──────────────┴──────────────────────────────────┘
```

- Sidebar width: 25% of total, clamped to `[18, 32]` cols, further clamped so the diff pane keeps at least 20 cols.
- Each file occupies two rows: `▎ M  path/to/file` then `      +N -N` (cursor glyph + badge + path on row one; cursor + counts on row two).
- Footer: a one-line hint strip with key bindings.

### Visual affordances

- Cursor glyph `▎` in column 0 marks the file at the current scroll offset. Path is bold white on the current row; dark-grey gutter cursor and default path style otherwise.
- Status badges: `M` (Modified, yellow), `D` (Deleted, red), `?` (Untracked, cyan). Black foreground on coloured background, bold.
- Path truncated from the left with `…` when wider than the pane. Width is measured in display cells via `unicode-width`, so CJK / wide-emoji paths truncate to fit ratatui's actual rendering.
- The sidebar pane scrolls its own visible window to keep the current row centred when the file list overflows pane height.

### Input handling

```
key        action
──────────────────────────────────────────
j / ↓      diff: scroll down 1 line
k / ↑      diff: scroll up 1 line
PgDn       diff: scroll down 20 lines
PgUp       diff: scroll up 20 lines
g / Home   diff: scroll to top
n          diff: jump to next file's header
b          diff: jump to previous file's header
s          toggle sidebar visibility
q / C-c    quit
```

The sidebar isn't directly navigable — its cursor follows the diff scroll. `n` and `b` are the way to move file-by-file.

### Render flow

```rust
// draw() (sketch)
let offsets = file_offsets(state, diff_inner_width);
let current_idx = offsets.iter().rposition(|&o| o <= scroll).unwrap_or(0);
draw_sidebar(frame, sidebar_area, state, current_idx);
draw_diff(frame, diff_area, state, scroll, repo_name, focused_path);
```

`file_offsets` returns the cumulative row offset for each file's header in the rendered diff stream. `sidebar_scroll(current_idx, visible_files, total)` decides where the sidebar's visible window starts so the cursor stays in view.

### Edge cases

- **Empty state** (clean tree): sidebar shows `(none)` in dark grey; diff pane shows `(no changes — waiting for edits)`. `n/b/g` are no-ops.
- **Sidebar overflow** (`order.len()` × 2 > pane height): visible window centres on the cursor row, snapping to top/bottom near the ends.
- **Path wider than pane**: `truncate_left` drops chars from the front, prefixing `…`, measured by display width.

## Files

| File | Role |
|------|------|
| `src/render.rs` | Sidebar layout and rendering: `draw_sidebar`, `sidebar_row`, `sidebar_scroll`, `sidebar_width`, `truncate_left`; `s` toggle in input dispatch. |
| `Cargo.toml` | `unicode-width` dependency for correct width measurement on non-ASCII content. |
