# Polish — Quick Wins + Demo GIF

**Status**: Draft
**Last Updated**: 2026-05-03

## Problem

Core flow is shippable (releases out, Homebrew + crates.io live), but the README has no demo, the keymap has no help screen, and a few small features would substantially raise the "first 30 seconds" experience. The two threads — feature wins and a demo GIF — are coupled: the GIF is much more compelling once a couple of the features below land, because there's something to *show*.

## Current State

- Single TUI with sidebar, diff stream, vim-flavored keymap (`q j k u d g n b s`).
- No `?` help, no clipboard, no editor-launch, no word-level diff, no mtime label, no title stats.
- Diff target is hard-coded `working tree vs HEAD`; `Engine::read_head_blob` resolves the ref via `head_tree_id_or_empty()`.
- Footer hint string is `" q quit · j/k line · u/d or PgUp/PgDn page · n/b file · g top · s sidebar "` — already at the limit of what fits before truncating on narrow terminals.
- `tools` available: VHS for GIF, `arboard` for clipboard, `imara-diff` already used for line-diff (supports char-level too).

## Approach

Two-track, sequential:

1. **Ship 2–3 features** that are individually small and *visually demoable*. Prioritize the ones that would show up well on a 10-second GIF.
2. **Then** record a VHS GIF that puts the new features on screen alongside the core live-diff loop, and embed it in the README near the badge row.

Why this order: a GIF that only shows scrolling reveals less than one that shows `?` overlay, word-level highlighting on a small edit, and a stat in the title. The features make the demo, not the other way around.

## Design Details

### Quick feature wins

| Feature | What | Why | Cost | Demo-worthy? |
|---|---|---|---|---|
| **`?` help overlay** | Modal listing all keybindings, dismissed by any key | Onboarding; footer hint won't scale past ~10 binds. Lazygit, k9s, btop all have this. | ~40 LOC in `render.rs`; one new draw helper, one `View` flag | Yes — pop it for half a beat |
| **`o` open in `$EDITOR`** | Hitting `o` opens the focused file in `$EDITOR` | Closes the diff → fix → diff loop without alt-tabbing | ~30 LOC; `std::process::Command`, save/restore terminal, re-enter raw mode | Yes — show jumping to `nvim`, editing, returning |
| **`y` yank path / `Y` yank hunk** | `y` copies the focused path to clipboard, `Y` copies the focused hunk text | "Let me show you this" workflow; `gh pr comment` paste | small (`arboard` crate ~+15 LOC) | Marginal — clipboard isn't visible on screen |
| **`--head <ref>`** | CLI flag changing the diff base from `HEAD` to any rev | Review a branch live as you check it out / rebase | trivial — `head_tree_id_or_empty()` swap to a parsed ref | Marginal — would need a "before/after branch checkout" demo |
| **Title stats** | `repo · 5 files +120 -34 · v0.1.1` | One-glance "how big is this PR" | trivial — sum across `state.iter_ordered()` in `draw_diff` | Yes — visible in every frame |
| **Mtime label in sidebar** | `src/x.rs · 5s ago` second line | Recency cue, especially with a slow agent | trivial — `humantime` already a dep, format `mtime` | Yes — ticks visibly while agent edits |
| **Word-level diff** | Within an Added/Removed pair, highlight just the changed substring | Major readability upgrade for typical agent edits (1-2 words on a line) | medium — `imara_diff::sliders` or `Diff::compute` over `&[char]`, then style spans differently | Yes — single biggest visual improvement |

**Recommended slice for the GIF**: `?` help, title stats, mtime label, word-level diff. The first three are trivial and stack visibly; word-level is the readability win that justifies the GIF carrying more than a "scroll through diffs" message. Editor-launch and `--head` can ship in the same wave but probably won't be in the GIF.

### Demo GIF

Tool: [VHS](https://github.com/charmbracelet/vhs) (`brew install vhs`). Tape script (`assets/demo.tape`) renders to `assets/demo.gif`. Embed in README right after the badge row, before the install section.

The mechanic: gitstream watches; the tape needs *something else* editing files concurrently. VHS supports this via backgrounded shell commands typed during the recording.

```tape
# assets/demo.tape (sketch, not final)
Output assets/demo.gif
Set Width 1200
Set Height 700
Set FontSize 14
Set Theme "Catppuccin Mocha"

# Hidden setup: temp repo with a starter file
Hide
Type "cd /tmp && rm -rf gs-demo && git init -q -b main gs-demo && cd gs-demo"
Enter
Type 'echo "fn main() {}" > main.rs && git add . && git commit -q -m init'
Enter
Show

# Foreground: gitstream watches
Type "gitstream"
Enter
Sleep 1.5s

# Backgrounded edits — gitstream picks them up live; gif captures the diff
Hide
Type 'bash -c "sleep 1.5; printf \"fn main() {\\n    println!(\\\"hello\\\");\\n}\\n\" > main.rs" &'
Enter
Show
Sleep 3s

Type "?"        # if implemented: show help overlay
Sleep 2s
Type "Escape"

Type "q"
Sleep 500ms
```

Targets: 800–1500 KB GIF, ≤30 seconds runtime. Re-record any time a feature lands that's worth showing.

## Impact

| File | Action | Change |
|------|--------|--------|
| `src/render.rs` | Modify | Help overlay + dismiss logic; title stats span; mtime label in sidebar row; word-level diff styling in hunk lines |
| `src/main.rs` | Modify | (if shipping `--head`) parse `--head <ref>` flag; thread to `app::run` |
| `src/app.rs` | Modify | (same) thread `head_ref` parameter |
| `src/diff.rs` | Modify | (same) `Engine::head_ref` field; replace `head_tree_id_or_empty` with parsed ref |
| `Cargo.toml` | Modify | (if `y`/`Y`) add `arboard`; (always) `humantime` already present |
| `assets/demo.tape` | New | VHS script |
| `assets/demo.gif` | New | Generated artifact, committed |
| `README.md` | Modify | Embed demo gif after badge row |

## Risks

- **Help overlay key conflicts**: `?` is unused today, but if a future binding wants `?` we'd have to re-route. Low risk.
- **Editor launch terminal corruption**: `$EDITOR` like `vim` will scribble over the alternate screen unless we drop and re-enter the alt-screen + raw-mode around the call. Existing `TerminalGuard` doesn't quite cover this — needs a scoped "release the terminal" pattern.
- **Word-level diff false positives**: line-by-line diff already handles 95% of cases well. Char-level on lines that differ a lot can produce noisy "every char changed" output; need a length-ratio fallback to "no word-level marking" for lines that diverge past, say, 50%.
- **GIF reproducibility**: VHS depends on font availability and theme. Pin both in the tape script. Re-recording on a different machine may produce slightly different output — acceptable.
- **GIF binary in repo**: ~1 MB GIF in git history adds permanent weight. Acceptable for one-off demo; if it grows past 2 MB consider Git LFS or hosting the GIF on the GitHub release assets and linking from README.

## Validation

- [ ] `?` opens overlay; any key dismisses; existing keymap unaffected.
- [ ] Title shows `+N -M` totals that sum across `state.iter_ordered()`; updates within one frame of an apply.
- [ ] Sidebar second row shows e.g. `+1 -0  · 3s ago` and ticks every redraw.
- [ ] Word-level diff: on a single-word edit (`foo` → `bar` in a longer line), only `foo`/`bar` is highlighted, the rest is plain context style.
- [ ] `vhs assets/demo.tape` produces `assets/demo.gif` reproducibly on macOS arm64.
- [ ] README renders the GIF inline on github.com (relative path works).
- [ ] `cargo build`, `cargo test`, `cargo clippy -- -D warnings` clean after every feature.

## Implementation Notes
[Updated during implementation. Record deviations, discoveries, decisions made.]
