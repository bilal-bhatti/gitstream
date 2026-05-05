# Bugs

Tracking bugs that aren't urgent enough to fix immediately but shouldn't be forgotten.

---

## editor shell-out occasionally truncates target file to 0 bytes

**Status**: parked — not reproducible on retry
**Last seen**: 2026-05-04

### Symptom

Pressed `e` on `src/render.rs` from the gitstream sidebar. Helix opened, made an
edit, saved with `:w`, quit. After returning to gitstream, `src/render.rs` was 0
bytes on disk.

Retry of the exact same flow (same file, same editor, same session) worked
correctly. Could not reproduce.

### What we know rules things out

- gitstream itself never writes user files. Truncation didn't come from our side
  directly.
- Same helix as `$EDITOR` works cleanly when invoked from yazi — so this is not
  a helix-side save bug.
- The teardown order in `edit_file` (`src/render.rs:199`) is the right shape:
  set stop flag → join input thread → `guard.suspend()` (leaves alt screen,
  disables raw mode) → `Command::status()` (waits) → resume → `terminal.clear()`
  → respawn input thread.

### Where it likely diverges from yazi

Per the global TUI shell-out guidance (informed by yazi's `yazi-actor/src/app/stop.rs`
and `yazi-scheduler/src/process/process.rs`), yazi:

- **drops the `Term` entirely** before the spawn and rebuilds after
- wraps the spawn in an RAII `Permit` whose `Drop` calls `resume()`

gitstream instead keeps `Terminal<CrosstermBackend<Stdout>>` and `TerminalGuard`
alive across the spawn, and only suspends. ratatui's back buffer and crossterm's
process-level state are not torn down.

### Most plausible mechanism

The input thread polls every 100ms (`render.rs:257`), so there's a ≤100ms window
after the user presses `e` during which:

- gitstream's still-alive input thread can consume one or more keystrokes
- those keystrokes are translated to no-op or unrelated `InputEvent`s and
  discarded (or worse, queued)
- helix sees fewer keystrokes than the user typed; first keystrokes (e.g. `i`
  for insert mode) get eaten

Helix in normal mode interprets sequences like `%d` as "select all + delete";
followed by a saved buffer (`:w`) that's now empty, you get a 0-byte file.
Muscle memory after a swallowed keystroke is enough to trigger this.

### Fixes worth trying if it recurs

1. **Drop the `Terminal` across the spawn** (take it out of an `Option`,
   rebuild after `guard.resume()`). Aligns with yazi's "drop Term entirely."
2. **Drain the tty input buffer between suspend and spawn**:
   `libc::tcflush(STDIN_FILENO, TCIFLUSH)`. Discards anything queued during
   teardown.
3. **Tighten input poll** from 100ms → 25ms. Shrinks the keystroke-steal
   window 4×.

### Repro recipe (none — for next attempt)

Run with `GITSTREAM_LOG=debug gitstream` so `/tmp/gitstream.log` captures the
`edit_file` lifecycle. If it happens again, the log will show whether the input
thread saw extra events around the spawn boundary.
