# gitstream

A real-time git diff watcher for the terminal. Edits in your working tree appear as a live, scrolling diff ordered by file mtime — most recently changed first.

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

### Keys

| key            | action               |
|----------------|----------------------|
| `q`, `Ctrl-C`  | quit                 |
| `j`, `Down`    | scroll down          |
| `k`, `Up`      | scroll up            |
| `PgDn`, `PgUp` | scroll one page      |
| `g`, `Home`    | jump to top          |

## Why

Existing tools shell out to the `git` binary on every fsnotify event, which is too slow and visually disruptive to be useful as a live "what am I changing right now" surface. gitstream reads diffs directly via `gix` (gitoxide) and keeps a stable scroll surface across repaints.

The diff target is **working tree vs `HEAD`** — both staged and unstaged changes are visible.

## Logging

Set `GITSTREAM_LOG=info` (or `debug`, `trace`) to enable structured logs on stderr — they don't interfere with the TUI, which lives on the alternate screen.
