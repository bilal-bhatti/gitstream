# MVP — Live Git Diff Watcher

**Status**: Approved
**Last Updated**: 2026-05-03

## Approved Defaults (open questions resolved)

1. Diff target: working tree vs `HEAD`.
2. Untracked files: included as all-additions.
3. `.gitignore`: respected for both watcher and diff.
4. Initial status scan: blocking on startup.
5. Repo root resolution: walk upward from CWD looking for `.git`.

## Problem
There is no off-the-shelf tool that gives a real-time, scrolling diff view of a working tree, ordered by file mtime (most-recently-touched first). Existing options shell out to the `git` binary on every fsnotify event and lose interactive scroll position on each repaint, which is too slow and visually disruptive to be useful as a live "what am I changing right now" surface. gitstream fills that gap as a single-binary CLI that reads diffs directly via `gix` and renders them in a stable scroll surface.

## Current State
- Empty cargo binary scaffold at `src/main.rs` (hello-world stub).
- No dependencies declared in `Cargo.toml` yet.
- Project conventions in `CLAUDE.md`: fail-fast (no silent fallbacks), no shelling out to `git`, lowercase logs via `tracing`, `thiserror` internally with `anyhow` only at the binary entrypoint, no premature abstraction.
- Edition 2024.

## Approach

A three-stage pipeline with bounded channels between stages, run on threads (no async runtime — the workload is small, latency-sensitive, and async adds complexity without a payoff here):

```
notify-debouncer-full ──► [WatchEvent] ──► diff worker ──► [DiffUpdate] ──► render loop (main thread)
        (its own thread)                  (one worker thread)               (ratatui + crossterm)
```

- **Watcher** wraps `notify-debouncer-full`. Already debounces per-path event bursts (editor saves fire write+rename+chmod). Emits one `WatchEvent { path, kind, timestamp }` per debounced change.
- **Diff worker** receives a `WatchEvent`, asks `gix` for the diff of that file vs `HEAD`, and emits a `DiffUpdate { path, mtime, status, hunks, added, removed }`. Errors on the hot path are logged with context and dropped — they do not crash the render loop. (This is *not* a silent fallback; it is the documented behavior for transient diff failures, e.g. a file disappearing mid-event. We log and continue.)
- **Render loop** owns `State`, applies each `DiffUpdate` (insert/replace/remove), re-sorts by mtime descending, and repaints. Input handling (q/Ctrl-C/PgUp/PgDn/j/k) lives here.

**Diff target**: working tree vs `HEAD`. Captures both staged and unstaged changes — the full set of "things I've done since last commit". Single target only in MVP, no flag.

**Initial state**: on startup, run `gix` status to seed `State` with all currently-changed files (otherwise the view is empty until something changes). mtime for these = file mtime from `fs::metadata`.

**Untracked files**: included, treated as 100% additions.
**Deleted files**: rendered with a "deleted" header and the removed lines.
**Submodules / binary files**: skipped in MVP, with a one-line placeholder entry.

### Why threads, not async
- Three coarse stages, no fan-out, no IO multiplexing. Channels + threads express this directly.
- Avoids pulling tokio + its transitive surface for what is essentially three loops.
- `crossbeam-channel` gives bounded SPSC/MPSC channels with `select!`, which is all we need.

### Why one diff worker, not a pool
- Diff cost is dominated by file size, not count. A burst of saves across 5 files debounces to 5 sequential diffs, each typically sub-millisecond on `gix`. A pool adds ordering complexity (out-of-order updates would need sequence numbers) for no real gain at MVP scale. Revisit if profiling shows the worker as the bottleneck.

### Why re-sort the whole list per update
- Changed-file count `n` in a typical session is < 50. `O(n log n)` per update is invisible. A sorted-data-structure (BTreeMap by `(mtime, path)`) trades clarity for negligible wins. Sort on update; revisit if `n` grows.

## Design Details

### Module layout
```
src/
├── main.rs        # argv parsing (clap), tracing init, error reporting, calls app::run
├── lib.rs         # re-exports modules; enables integration tests against the library
├── app.rs         # builds channels, spawns watcher + worker, runs render loop
├── watcher.rs     # notify-debouncer-full wrapper → WatchEvent stream
├── diff.rs        # gix-backed: WatchEvent → DiffUpdate (or initial-status scan)
├── state.rs       # in-memory model: entries by path, ordered list by mtime
├── render.rs      # ratatui draw + crossterm input → AppCommand
└── error.rs       # single Error enum (thiserror) for the library
```

Seven files. Each module has one responsibility; no module re-implements another's job. No `traits` introduced — every type has exactly one impl.

### Core types (sketch — exact gix types finalized in implementation)

```rust
// error.rs
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a git repository: {path}")]
    NotARepo { path: PathBuf },
    #[error("watcher error at {path}: {source}")]
    Watch { path: PathBuf, #[source] source: notify::Error },
    #[error("diff error at {path}: {source}")]
    Diff { path: PathBuf, #[source] source: Box<dyn std::error::Error + Send + Sync> },
    #[error("io error at {path}: {source}")]
    Io { path: PathBuf, #[source] source: std::io::Error },
    #[error("terminal error: {0}")]
    Term(#[from] std::io::Error /* crossterm */),
}

// watcher.rs
pub struct WatchEvent {
    pub path: PathBuf,
    pub kind: ChangeHint,        // Modify | Create | Remove (best-effort from notify)
    pub at: Instant,
}

// state.rs
pub struct DiffUpdate {
    pub path: PathBuf,
    pub mtime: Instant,
    pub status: ChangeKind,      // Added | Modified | Deleted | Renamed { from }
    pub hunks: Vec<Hunk>,        // empty for binary or deleted-without-content
    pub added: u32,
    pub removed: u32,
    pub binary: bool,
}

pub struct Hunk {
    pub old_range: (u32, u32),   // (start, len)
    pub new_range: (u32, u32),
    pub lines: Vec<HunkLine>,
}

pub enum HunkLine {
    Context(String),
    Added(String),
    Removed(String),
}

pub struct State {
    by_path: HashMap<PathBuf, DiffUpdate>,
    order: Vec<PathBuf>,         // mtime descending
}

impl State {
    pub fn apply(&mut self, update: DiffUpdate);  // insert/replace + resort
    pub fn drop_if_clean(&mut self, path: &Path); // remove when diff becomes empty
    pub fn iter_ordered(&self) -> impl Iterator<Item = &DiffUpdate>;
}
```

### Threading and channels

```rust
// app.rs (sketch)
pub fn run(repo_root: PathBuf) -> Result<(), Error> {
    let (ev_tx, ev_rx) = crossbeam_channel::bounded::<WatchEvent>(64);
    let (up_tx, up_rx) = crossbeam_channel::bounded::<DiffUpdate>(64);

    let _watcher_guard = watcher::spawn(&repo_root, ev_tx)?;     // handle drops on Drop
    let _worker_guard  = diff::spawn_worker(repo_root.clone(), ev_rx, up_tx)?;

    render::run(up_rx)?;  // owns the main thread until quit
    Ok(())
}
```

- Bounded channels (cap 64) provide back-pressure: a slow renderer cannot let the queue blow up memory. If the channel fills, the diff worker blocks; if the watcher's channel fills, debouncer drops (acceptable — debouncer already coalesces).
- Each spawn returns a guard that joins/aborts the thread on drop, so a panic in one stage tears down the rest cleanly.

### Render loop (main thread)

- Single `select!` over `up_rx` and an input channel fed by a tiny crossterm-input thread.
- On `DiffUpdate`: `state.apply(update)`, redraw.
- On input: scroll, quit, or no-op.
- Fully event-driven — no periodic ticks. Repo metadata changes (commits, staging, `.gitignore` edits) propagate from the watcher as `ChangeHint::Rescan` signals.
- Terminal restoration via a `Drop`-implementing guard around `enter_alternate_screen` / `enable_raw_mode`. Panics restore the terminal cleanly via `std::panic::set_hook` chained into the same guard.

### CLI surface (clap, derive)

```
gitstream [PATH]

PATH    Repository root (default: current directory; walked up to find .git)
```

No other flags in MVP. Future flags (`--target`, `--include-binary`, `--filter`) are out of scope.

## Impact

| File | Action | Change |
|------|--------|--------|
| `Cargo.toml` | Modify | Add dependencies (see below); set `[lib]` and `[[bin]]`. |
| `src/lib.rs` | New | `pub mod` re-exports for `app`, `watcher`, `diff`, `state`, `render`, `error`. |
| `src/main.rs` | Rewrite | clap arg parsing, tracing init, calls `gitstream::app::run`, prints errors via `anyhow`. |
| `src/app.rs` | New | Channel wiring, thread spawning, lifecycle. |
| `src/watcher.rs` | New | `notify-debouncer-full` wrapper, `WatchEvent` stream. |
| `src/diff.rs` | New | `gix` integration: initial status scan + per-file diff vs HEAD. |
| `src/state.rs` | New | `State` model, ordering, apply logic. |
| `src/render.rs` | New | ratatui layout, scroll state, input handling, terminal guard. |
| `src/error.rs` | New | Single `Error` enum. |
| `tests/smoke.rs` | New | One end-to-end: build a temp repo with `gix`, mutate files, assert `State` reflects ordering. No TUI in this test. |
| `.gitignore` | Already covered | `/target` already ignored. |

### Dependencies (Cargo.toml)

```toml
[dependencies]
gix                       = { version = "*", default-features = false, features = ["max-performance-safe"] }
notify                    = "*"
notify-debouncer-full     = "*"
ratatui                   = "*"
crossterm                 = "*"
crossbeam-channel         = "*"
thiserror                 = "*"
anyhow                    = "*"   # main.rs only
tracing                   = "*"
tracing-subscriber        = { version = "*", features = ["env-filter"] }
clap                      = { version = "*", features = ["derive"] }
humantime                 = "*"   # "5s ago" formatting

[dev-dependencies]
tempfile                  = "*"
```

Versions pinned during implementation step. `gix` features will be revisited then — `max-performance-safe` is the safe-Rust performance preset.

## Risks

- **gix diff API surface drift.** `gix` is pre-1.0 and reorganizes occasionally. *Mitigation*: pin a known version at implementation time; keep the diff module thin so a future migration touches one file.
- **fsnotify on macOS misses changes inside `.git/`.** *Mitigation*: classify `.git/` events — `HEAD`, `index`, `refs/*`, `ORIG_HEAD`, `MERGE_HEAD`, `FETCH_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `packed-refs` emit a `ChangeHint::Rescan` signal so commits/staging/checkouts propagate. All other `.git/` paths stay filtered. In practice fsevent on macOS does see git's atomic `lock+rename` updates; if a particular setup misses these, surface the gap via tracing rather than papering over with polling.
- **Editor atomic-save patterns** (write-to-tempfile + rename) can produce a "delete then create" sequence. *Mitigation*: `notify-debouncer-full` already coalesces these into a single change event for the destination path.
- **Terminal corruption on panic.** *Mitigation*: panic hook + `Drop` guard restores the terminal before the panic message prints.
- **High-frequency saves** (e.g. a formatter on save firing across the repo) could swamp the diff worker. *Mitigation*: bounded channel back-pressure naturally drops the lag onto the watcher; debouncer window absorbs bursts. If observed in practice, raise debounce window or move to a worker pool.
- **Submodules and very large repos.** Out of MVP scope. We document the limitation rather than half-supporting it.

## Open Questions (please confirm or override during review)

1. **Diff target**: working tree vs `HEAD` is the proposed default. Confirm? (Alternative: vs index, which excludes staged work — less useful for a live view.)
2. **Untracked files**: include as all-additions? (Proposed: yes.)
3. **`.gitignore`'d files**: respect `.gitignore` for both watcher and diff? (Proposed: yes — saves CPU and matches user expectation.)
4. **Initial status scan**: blocking on startup vs streaming as files are discovered? (Proposed: blocking — keeps state.rs simpler; first paint delayed by ~50ms in a typical repo.)
5. **Repo root resolution**: walk upward from CWD looking for `.git`? (Proposed: yes, mirroring `git` itself.)

## Validation

- [ ] `cargo build` and `cargo clippy -- -D warnings` clean on a fresh checkout.
- [ ] Run in a real repo, edit a file, see it appear/update at the top of the view within ~150ms p95.
- [ ] Edit a second file; top of view swaps to the more-recently-changed file.
- [ ] Untracked file appears as all-additions; deleting it removes the entry.
- [ ] Stage a change with `git add` outside the tool: entry remains visible (because target is HEAD). Commit it: entry disappears on next event or tick.
- [ ] Run in a non-git directory: clean error message, no panic.
- [ ] `q` / `Ctrl-C` quits and restores the terminal cleanly. Forced panic also restores the terminal.
- [ ] Idle CPU near zero with no file activity in a 1k-file repo.
- [ ] `tests/smoke.rs` passes — exercises the watcher → diff → state path against a temp git repo without the TUI.

## Implementation Notes

**Status**: Implemented 2026-05-03. `cargo build` + `cargo clippy --all-targets -- -D warnings` clean. `cargo test` passes (6 lib unit tests + 5 smoke tests).

Deviations and decisions from the original sketch:

- **`mtime: SystemTime`, not `Instant`.** File modification time is naturally `SystemTime` (from `fs::metadata().modified()`), and we sort by it directly. `Instant` would have required a baseline shift and lost meaning across the initial scan.
- **`imara-diff` direct, not `gix::object::blob::diff::Platform`.** The gix-side Platform requires setting up a `gix_diff::blob::Platform` resource cache, which is heavier than necessary. We instead read the HEAD blob bytes and worktree bytes ourselves and feed them straight to `imara_diff::Diff::compute(Algorithm::Histogram, …)`. Adds one explicit dep (`imara-diff = "0.2"`), saves substantial setup code.
- **Hunk context lines: 3, with adjacent hunks merged.** Two hunks merge when their context regions would overlap (`B.before.start - A.before.end <= 2 * CONTEXT`). Context lines are walked from `input.before` (identical to `input.after` at unchanged positions). `HunkLine::Context` is now actually emitted.
- **Fully event-driven, no periodic ticks.** The original sketch hedged with "we re-poll status on a tick or on demand." This was wrong for a live streamer — polling is the thing this tool exists to avoid, even at the metadata layer. Instead the watcher classifies `.git/` paths and emits `ChangeHint::Rescan` signals on `HEAD`, `index`, `refs/*`, `ORIG_HEAD`, `MERGE_HEAD`, `FETCH_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `packed-refs`. The diff worker has no ticker — it only acts on signals. Commits, staging, checkouts all propagate through fsnotify.
- **`.gitignore` is filtered via `gix`'s excludes stack** (the proven Git-compatible matcher), not a hand-rolled implementation. The watcher builds a `gix::worktree::Stack` from `Repository::excludes()` once at startup and consults it on every event. When any `.gitignore` changes anywhere in the tree, the stack is rebuilt in place and a `Rescan` signal fires so previously-shown ignored paths drop and newly-allowed paths surface.
- **Quit-time drop order matters.** `app.rs` declares `_worker_guard` *before* `_watcher_guard` so on shutdown the watcher drops first → `ev_tx` closes → the worker's `ev_rx.recv()` returns `Err` immediately → worker thread exits → `WorkerGuard::drop` joins instantly. The reverse order deadlocked: the worker would block on `ev_rx.recv()` (sender held alive by the still-undropped watcher) while `WorkerGuard::drop` waited on the join, forcing the user to Ctrl-C and leaving the terminal in raw mode (exit 130).
- **Initial scan runs on the worker thread itself**, before the event loop. `spawn_worker` returns instantly; the worker does the scan and emits updates through `up_tx`. The user sees an empty list for ~50–200ms before initial entries flow in. Simpler than the throwaway-thread approach.
- **Binary detection is the standard "null byte in first 8KB" heuristic** rather than gix's heuristic. Cheap, correct in practice, and decouples us from gix internals.
- **Watcher debounce window: 100ms.** Not specified in the plan; chosen to absorb editor save bursts (write+rename+chmod) while staying well under 150ms p95.
- **`gix` features pinned to `["max-performance-safe", "sha1", "status", "blob-diff", "dirwalk", "excludes"]`.** Default features pull in network transports we don't need; this is the minimum that compiles `status`-driven workflows. Without explicit `sha1`, `gix-hash` fails to compile (defaults include `sha1` but disabling defaults drops it).
- **`Error::Term` distinct from `Error::Io`.** Crossterm errors are `std::io::Error` but have no associated path; keeping them separate avoids a dummy path on terminal failures. `Error::Excludes` added for excludes-stack setup failures.

Files shipped:

- `src/error.rs` — `Error` enum + `Result` alias
- `src/state.rs` — `State`, `DiffUpdate`, `Hunk`, `HunkLine`, `ChangeKind` (+ unit tests)
- `src/watcher.rs` — `notify-debouncer-full` wrapper; gix excludes filter; `.git/` classification with `Rescan` signals; `.gitignore` reload
- `src/diff.rs` — `gix`-backed initial scan + per-event recompute + `Rescan` handling; hunk context+merge; runs on worker thread
- `src/app.rs` — channel wiring, guards (worker declared before watcher for clean quit)
- `src/render.rs` — ratatui draw, input thread, panic-hook + Drop terminal guard
- `src/main.rs` — clap CLI, walk-up repo root resolution, anyhow at the boundary
- `src/lib.rs` — module re-exports
- `tests/smoke.rs` — 5 e2e tests against temp git repos (modify-and-order, untracked classification, gitignore filtering, rescan-after-external-commit, revert-drops-entry)

Open follow-ups (not blocking MVP):
- Validation checklist item "p95 latency < 150ms" needs a Criterion bench, deferred.
- Sidebar (separate plan: `plans/sidebar/sidebar.md`).
