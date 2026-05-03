# gitstream

A real-time git diff watcher CLI. Watches a working tree for file changes, computes diffs against the index/HEAD on every event, and renders a scrolling TUI of hunks ordered by file mtime (most recently changed first).

**Performance is the product.** This tool exists because shelling out to `git diff` on every fsnotify event is too slow. If a design choice trades clarity for measurable latency wins on the hot path (event → diff → render), take the win. If it adds complexity without measurable wins, reject it.

---

## Critical Principle: Fail Fast, No Silent Fallbacks

**Always return errors immediately. Never implement silent fallbacks without explicit confirmation.**

- If an operation fails, return the error to the caller — do not catch it and substitute alternative behavior
- If you are tempted to add a fallback, stop and ask whether it is an explicit product requirement first
- Treat every silent fallback as a bug until proven otherwise

```rust
// ✅ Return error, fail fast
let diff = repo.diff_tree_to_workdir(&tree, opts)
    .map_err(|e| Error::Diff { path: path.to_path_buf(), source: e })?;

// ❌ Never silently degrade
let diff = match repo.diff_tree_to_workdir(&tree, opts) {
    Ok(d) => d,
    Err(_) => Diff::empty(), // silent fallback — hides real failures
};
```

**Fallbacks are only appropriate when**: explicitly requested by product requirements, architecturally designed, properly tested, and instrumented.

**Logging vs returning errors**: Log for observability. Return errors for control flow. Do both:
```rust
tracing::error!(error = %err, path = %path.display(), "diff computation failed");
return Err(Error::Diff { path: path.to_path_buf(), source: err });
```

---

## Performance Constraints

These are non-negotiable and shape every design decision:

- **No shelling out to `git`.** Use `gix` (gitoxide) for repository operations. Forking the git binary on every event is the thing this tool exists to avoid.
- **No global locks on the hot path.** The render thread must not block on diff computation, and the watcher must not block on either.
- **Diff cost scales with the change, not the repo.** Recomputing diffs for unchanged files on every event is a bug.
- **Event coalescing is mandatory.** Editors fire bursts of fsnotify events on save (rename + write + chmod). Debounce per-path before triggering a diff recompute.
- **Allocations on the hot path are suspect.** Reuse buffers where the type system allows; benchmark before optimizing.
- **Measure before claiming a win.** Performance claims need a Criterion benchmark, not vibes.

---

## Code Style

### Logging
- All log messages: lowercase
- Use `tracing` macros (`tracing::info!`, `tracing::error!`) — not `println!` or `log::`
- Structured fields over string interpolation: `tracing::info!(path = %p.display(), "watch started")` not `tracing::info!("watch started: {}", p.display())`

### Errors
- Use `thiserror` for library/internal error types — concrete enums with `#[from]` conversions
- Use `anyhow` only at the binary entrypoint (`main.rs`) and never inside library modules
- Wrap with context using `.map_err(|e| Error::Variant { ..., source: e })?`
- Include identifying details on every wrap (path, ref, oid, etc.)
- Match with `matches!` or destructuring; check kind via concrete variants, not string comparison

### Modernization
- Edition 2024
- Use `let-else` for early returns over nested `match`/`if let`
- Use `?` for error propagation; reserve `unwrap`/`expect` for genuinely impossible cases (and write a comment explaining why)
- Prefer `&str` and borrowed slices in signatures; take owned types only when you need to keep them

### No Premature Abstraction
- One impl, no trait. Add a trait only when there is a second impl, or a real need to mock at a boundary.
- No builder patterns for structs with < 4 fields.
- No `Box<dyn Trait>` until generics demonstrably hurt.

---

## AI Assistant Rules

- Never make assumptions about coding requirements
- Ask questions for clarification until requirements are clear
- Never add `Co-Authored-By: Claude ...` trailers to commits

### Tool Selection
- **Code search**: prefer `ygrep` first (indexed, fast). Fall back to `Grep`/`Glob` only when ygrep returns nothing.
- **File reads**: use `Read`. Never `cat`/`head`/`tail`.
- **Cargo**: invoke directly (`cargo build`, `cargo test`, `cargo bench`, `cargo clippy`). No wrapper scripts.

### Structured Planning
For non-trivial efforts (5+ files, multi-week features, architectural changes, system migrations). **Not for**: bug fixes, small features (< 5 files), docs-only, config-only changes.

Full details: `plans/STRUCTURED-PLANNING-APPROACH.md`

**Key principles**:
- Analysis before design, design before implementation
- **Formal review required** at each phase gate — do not proceed without approval
- **Independent execution** — each step must be detailed enough for anyone to execute
- **Dependency-based prioritization** — order work based on what unblocks other work

**Core process** — formal review required at each gate:
1. **Analysis** → `01-analysis/problem.md` + `impact.md`
2. **Design** → `02-design/architecture.md` + `testing.md`
3. **Implementation** → `03-implementation/step-NN-description.md` files

```
plans/<project-name>/
├── SUMMARY.md
├── 01-analysis/
│   ├── problem.md
│   └── impact.md
├── 02-design/
│   ├── architecture.md
│   └── testing.md
└── 03-implementation/
```

**Lite planning** (one-pager): For smaller tasks needing critical analysis. Produces `plans/<task-name>/<task-name>.md`. Use when instructed. Details: `plans/LITE-PLANNING-APPROACH.md`
