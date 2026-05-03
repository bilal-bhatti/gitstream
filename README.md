# gitstream

`tail -f` for your working tree.

Trying to figure out what your coding agent is *actually* doing while it edits 15 files? Tired of flipping between `git diff` panes, `git status`, that one watch loop you never killed, and a fourth terminal because you forgot which file the agent touched last? Just want to see your tree breathe during a long refactor?

gitstream watches your working tree and renders every change as a live, scrolling diff in one pane — sorted most-recently-changed first, so the file you care about is always at the top.

It's useful any time someone or something else is editing your code:

- **Watching a coding agent work.** Diffs land as the agent saves; you read them in the order they happened, not in whatever order your editor's file tree felt like showing them.
- **Pair programming over SSH or a shared session.** No more "wait, what did you just change?" — the file's diff is at the top of the pane.
- **Long refactors and generator scripts.** Run `cargo fmt --all`, a codemod, or a custom script and watch the blast radius in real time instead of staring at `git status` after the fact.
- **Code review while it's still warm.** Review diffs the moment they land, not after a 40-commit PR.

## Install

```bash
cargo install --path .
```

Installs `gitstream` into `~/.cargo/bin`.

## Use

```bash
gitstream            # watch the current repository (walks up to .git)
gitstream <path>     # watch the repository rooted at <path>
```

The diff target is **working tree vs `HEAD`** — both staged and unstaged changes show up. Untracked files appear as all-additions; `.gitignore` is respected (and reloaded live when you edit it). External `git commit` / `git add` / `git checkout` propagate automatically — gitstream watches `.git/HEAD`, `.git/index`, and `.git/refs/*` for those.

### Keys

| key            | action               |
|----------------|----------------------|
| `q`, `Ctrl-C`  | quit                 |
| `j`, `Down`    | scroll down          |
| `k`, `Up`      | scroll up            |
| `n`            | jump to next file    |
| `b`            | jump back to previous file |
| `g`, `Home`    | jump to top          |
| `s`            | toggle sidebar       |
| `d`, `PgDn`    | scroll one page down |
| `u`, `PgUp`    | scroll one page up   |

## Why not just `watch git diff`?

Shelling out to the `git` binary on every fsnotify event is too slow and visually disruptive to be useful as a live "what am I changing right now" surface. Every save → fork+exec → repaint → lose your scroll position.

gitstream reads diffs directly via [`gix`](https://github.com/GitoxideLabs/gitoxide) (gitoxide), keeps a stable scroll surface across repaints, and is fully event-driven — no polling, no `watch`, no busy loop. Worst-case diff cost on a 5000-line scattered change is ~0.8ms (`cargo bench --bench diff`), so the bottleneck is the 100ms editor-debounce window, not the tool.

## Logging

Logs are off by default. To debug, run with `GITSTREAM_LOG=debug` (also accepts `info`, `trace`, env-filter syntax). Output goes to `/tmp/gitstream.log` so it doesn't corrupt the TUI; override the path with `GITSTREAM_LOG_FILE=/some/path`.

```bash
GITSTREAM_LOG=debug gitstream &
tail -f /tmp/gitstream.log
```
