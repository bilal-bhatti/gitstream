use crate::error::{Error, Result};
use crate::state::{ChangeKind, DiffUpdate, Hunk, HunkLine};
use crate::watcher::{ChangeHint, WatchEvent};
use crossbeam_channel::{Receiver, Sender};
use imara_diff::{Algorithm, Diff, InternedInput};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::SystemTime;

const CONTEXT_LINES: u32 = 3;
const TAB_WIDTH: usize = 4;

/// Strip the line terminator imara-diff includes in tokens and expand `\t` to
/// the next tab stop. Ratatui's grapheme iterator silently drops every
/// `char::is_control` codepoint — including `\t` — so a tab-indented line would
/// otherwise render with its indentation gone.
fn normalize_for_display(s: &str) -> String {
    let s = s.strip_suffix('\n').unwrap_or(s);
    let s = s.strip_suffix('\r').unwrap_or(s);
    if !s.contains('\t') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + TAB_WIDTH);
    let mut col: usize = 0;
    for ch in s.chars() {
        if ch == '\t' {
            let pad = TAB_WIDTH - (col % TAB_WIDTH);
            for _ in 0..pad {
                out.push(' ');
            }
            col += pad;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}

pub struct WorkerGuard {
    handle: Option<JoinHandle<()>>,
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

pub fn spawn_worker(
    repo_root: PathBuf,
    repo: gix::ThreadSafeRepository,
    ev_rx: Receiver<WatchEvent>,
    up_tx: Sender<DiffUpdate>,
) -> Result<WorkerGuard> {
    let engine = Arc::new(Engine::new(repo_root.clone(), repo));
    let handle = thread::Builder::new()
        .name("diff-worker".into())
        .spawn(move || run(engine, ev_rx, up_tx))
        .map_err(|e| Error::Io {
            path: repo_root,
            source: e,
        })?;
    Ok(WorkerGuard {
        handle: Some(handle),
    })
}

fn run(engine: Arc<Engine>, ev_rx: Receiver<WatchEvent>, up_tx: Sender<DiffUpdate>) {
    let mut known: HashSet<PathBuf> = HashSet::new();
    if !rescan(&engine, &up_tx, &mut known) {
        return;
    }
    while let Ok(ev) = ev_rx.recv() {
        let ok = match ev.kind {
            ChangeHint::Rescan => rescan(&engine, &up_tx, &mut known),
            _ => handle_event(&engine, &ev, &up_tx, &mut known),
        };
        if !ok {
            return;
        }
    }
}

fn handle_event(
    engine: &Engine,
    ev: &WatchEvent,
    up_tx: &Sender<DiffUpdate>,
    known: &mut HashSet<PathBuf>,
) -> bool {
    match engine.recompute(&ev.path) {
        Ok(Some(update)) => emit(update, up_tx, known),
        Ok(None) => {
            let rel = relative_or_absolute(&engine.repo_root, &ev.path);
            if known.remove(&rel) {
                let cleared = clean_update(rel, ChangeKind::Modified);
                if up_tx.send(cleared).is_err() {
                    return false;
                }
            }
            true
        }
        Err(err) => {
            tracing::error!(error = %err, path = %ev.path.display(), "diff recompute failed");
            true
        }
    }
}

fn rescan(engine: &Engine, up_tx: &Sender<DiffUpdate>, known: &mut HashSet<PathBuf>) -> bool {
    let updates = match engine.initial_scan() {
        Ok(u) => u,
        Err(err) => {
            tracing::error!(error = %err, "rescan failed");
            return true;
        }
    };

    let mut new_known: HashSet<PathBuf> = HashSet::with_capacity(updates.len());
    for update in updates {
        let path = update.path.clone();
        let is_clean = update.is_clean();
        if up_tx.send(update).is_err() {
            return false;
        }
        if !is_clean {
            new_known.insert(path);
        }
    }

    for stale in known.difference(&new_known) {
        let cleared = clean_update(stale.clone(), ChangeKind::Modified);
        if up_tx.send(cleared).is_err() {
            return false;
        }
    }

    *known = new_known;
    true
}

fn emit(update: DiffUpdate, up_tx: &Sender<DiffUpdate>, known: &mut HashSet<PathBuf>) -> bool {
    let path = update.path.clone();
    let is_clean = update.is_clean();
    if up_tx.send(update).is_err() {
        return false;
    }
    if is_clean {
        known.remove(&path);
    } else {
        known.insert(path);
    }
    true
}

fn clean_update(path: PathBuf, status: ChangeKind) -> DiffUpdate {
    DiffUpdate {
        path,
        mtime: SystemTime::now(),
        status,
        hunks: vec![],
        added: 0,
        removed: 0,
        binary: false,
    }
}

struct Engine {
    repo_root: PathBuf,
    repo: gix::ThreadSafeRepository,
}

#[doc(hidden)]
pub mod bench {
    //! Escape hatch for `benches/diff.rs`. Not part of the stable API.
    use super::*;

    pub struct BenchEngine(Engine);

    pub fn open(repo_root: &Path) -> Result<BenchEngine> {
        let repo = gix::open(repo_root)
            .map_err(|e| Error::RepoOpen {
                path: repo_root.to_path_buf(),
                source: Box::new(e),
            })?
            .into_sync();
        Ok(BenchEngine(Engine::new(repo_root.to_path_buf(), repo)))
    }

    impl BenchEngine {
        pub fn recompute(&self, abs_path: &Path) -> Result<Option<DiffUpdate>> {
            self.0.recompute(abs_path)
        }
    }
}

impl Engine {
    fn new(repo_root: PathBuf, repo: gix::ThreadSafeRepository) -> Self {
        Self { repo_root, repo }
    }

    fn initial_scan(&self) -> Result<Vec<DiffUpdate>> {
        let repo = self.repo.to_thread_local();
        let platform = repo.status(gix::progress::Discard).map_err(|e| Error::Diff {
            path: self.repo_root.clone(),
            source: Box::new(e),
        })?;

        let iter = platform.into_iter(None).map_err(|e| Error::Diff {
            path: self.repo_root.clone(),
            source: Box::new(e),
        })?;

        let mut paths: HashSet<PathBuf> = HashSet::new();
        for item in iter {
            let item = match item {
                Ok(i) => i,
                Err(err) => {
                    tracing::error!(error = %err, "status iterator error");
                    continue;
                }
            };
            paths.insert(bstr_to_path(item.location()));
        }

        let mut updates = Vec::with_capacity(paths.len());
        for rel in paths {
            let abs = self.repo_root.join(&rel);
            match self.recompute(&abs) {
                Ok(Some(u)) => updates.push(u),
                Ok(None) => {}
                Err(err) => {
                    tracing::error!(error = %err, path = %abs.display(), "initial diff failed");
                }
            }
        }
        Ok(updates)
    }

    fn recompute(&self, abs_path: &Path) -> Result<Option<DiffUpdate>> {
        let rel = match abs_path.strip_prefix(&self.repo_root) {
            Ok(r) => r.to_path_buf(),
            Err(_) => return Ok(None),
        };
        if rel.as_os_str().is_empty() {
            return Ok(None);
        }

        let worktree_bytes = read_optional(abs_path)?;
        let head_bytes = self.read_head_blob(&rel)?;

        let (status, mtime) = match (head_bytes.is_some(), worktree_bytes.is_some()) {
            (false, false) => return Ok(None),
            (false, true) => (ChangeKind::Untracked, file_mtime(abs_path)?),
            (true, false) => (ChangeKind::Deleted, SystemTime::now()),
            (true, true) => (ChangeKind::Modified, file_mtime(abs_path)?),
        };

        let before = head_bytes.unwrap_or_default();
        let after = worktree_bytes.unwrap_or_default();

        if before == after {
            return Ok(Some(DiffUpdate {
                path: rel,
                mtime,
                status,
                hunks: vec![],
                added: 0,
                removed: 0,
                binary: false,
            }));
        }

        if looks_binary(&before) || looks_binary(&after) {
            return Ok(Some(DiffUpdate {
                path: rel,
                mtime,
                status,
                hunks: vec![],
                added: 0,
                removed: 0,
                binary: true,
            }));
        }

        let before_str = String::from_utf8_lossy(&before).into_owned();
        let after_str = String::from_utf8_lossy(&after).into_owned();
        let input = InternedInput::new(before_str.as_str(), after_str.as_str());
        let diff = Diff::compute(Algorithm::Histogram, &input);
        let (hunks, added, removed) = build_hunks_with_context(&diff, &input, CONTEXT_LINES);

        Ok(Some(DiffUpdate {
            path: rel,
            mtime,
            status,
            hunks,
            added,
            removed,
            binary: false,
        }))
    }

    fn read_head_blob(&self, rel: &Path) -> Result<Option<Vec<u8>>> {
        let repo = self.repo.to_thread_local();
        let head_id = repo.head_tree_id_or_empty().map_err(|e| Error::Diff {
            path: rel.to_path_buf(),
            source: Box::new(e),
        })?;
        if head_id.is_empty_tree() {
            return Ok(None);
        }
        let tree = repo
            .find_object(head_id)
            .map_err(|e| Error::Diff {
                path: rel.to_path_buf(),
                source: Box::new(e),
            })?
            .into_tree();
        let entry = match tree.lookup_entry_by_path(rel).map_err(|e| Error::Diff {
            path: rel.to_path_buf(),
            source: Box::new(e),
        })? {
            Some(e) => e,
            None => return Ok(None),
        };
        if !entry.mode().is_blob() {
            return Ok(None);
        }
        let blob = entry
            .object()
            .map_err(|e| Error::Diff {
                path: rel.to_path_buf(),
                source: Box::new(e),
            })?
            .into_blob();
        Ok(Some(blob.data.clone()))
    }
}

fn build_hunks_with_context(
    diff: &Diff,
    input: &InternedInput<&str>,
    context: u32,
) -> (Vec<Hunk>, u32, u32) {
    let raw: Vec<imara_diff::Hunk> = diff.hunks().collect();
    if raw.is_empty() {
        return (Vec::new(), 0, 0);
    }

    let mut merged: Vec<imara_diff::Hunk> = Vec::with_capacity(raw.len());
    for h in raw {
        if let Some(last) = merged.last_mut()
            && h.before.start.saturating_sub(last.before.end) <= 2 * context
        {
            last.before.end = h.before.end;
            last.after.end = h.after.end;
            continue;
        }
        merged.push(h);
    }

    let n_before = input.before.len() as u32;
    let n_after = input.after.len() as u32;

    let mut hunks = Vec::with_capacity(merged.len());
    let mut total_removed = 0u32;
    let mut total_added = 0u32;

    for h in merged {
        let pre_b = h.before.start.saturating_sub(context);
        let post_b = h.before.end.saturating_add(context).min(n_before);
        let pre_a = h.after.start.saturating_sub(context);
        let post_a = h.after.end.saturating_add(context).min(n_after);

        let mut lines = Vec::new();

        for i in pre_b..h.before.start {
            let token = input.before[i as usize];
            lines.push(HunkLine::Context(normalize_for_display(input.interner[token])));
        }

        let mut i = h.before.start;
        let mut j = h.after.start;
        while i < h.before.end || j < h.after.end {
            let removed = i < h.before.end && diff.is_removed(i);
            let added = j < h.after.end && diff.is_added(j);
            if removed {
                let token = input.before[i as usize];
                lines.push(HunkLine::Removed(normalize_for_display(input.interner[token])));
                total_removed += 1;
                i += 1;
            } else if added {
                let token = input.after[j as usize];
                lines.push(HunkLine::Added(normalize_for_display(input.interner[token])));
                total_added += 1;
                j += 1;
            } else if i < h.before.end && j < h.after.end {
                let token = input.before[i as usize];
                lines.push(HunkLine::Context(normalize_for_display(input.interner[token])));
                i += 1;
                j += 1;
            } else {
                break;
            }
        }

        for i in h.before.end..post_b {
            let token = input.before[i as usize];
            lines.push(HunkLine::Context(normalize_for_display(input.interner[token])));
        }

        hunks.push(Hunk {
            old_range: (pre_b, post_b - pre_b),
            new_range: (pre_a, post_a - pre_a),
            lines,
        });
    }

    (hunks, total_added, total_removed)
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(Error::Io {
                path: path.to_path_buf(),
                source: e,
            });
        }
    };
    if !metadata.file_type().is_file() {
        // directories, symlinks, sockets, etc. — not diffable
        return Ok(None);
    }
    let mut f = fs::File::open(path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(Some(buf))
}

fn file_mtime(path: &Path) -> Result<SystemTime> {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e,
        })
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|&b| b == 0)
}

fn bstr_to_path(bs: &gix::bstr::BStr) -> PathBuf {
    use gix::bstr::ByteSlice;
    PathBuf::from(bs.to_os_str_lossy().into_owned())
}

fn relative_or_absolute(repo_root: &Path, abs: &Path) -> PathBuf {
    abs.strip_prefix(repo_root)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| abs.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_of(hunk: &Hunk) -> Vec<(&'static str, String)> {
        hunk.lines
            .iter()
            .map(|l| match l {
                HunkLine::Context(s) => ("ctx", s.clone()),
                HunkLine::Added(s) => ("add", s.clone()),
                HunkLine::Removed(s) => ("rem", s.clone()),
            })
            .collect()
    }

    fn diff_from(before: &str, after: &str, context: u32) -> Vec<Hunk> {
        let input = InternedInput::new(before, after);
        let diff = Diff::compute(Algorithm::Histogram, &input);
        build_hunks_with_context(&diff, &input, context).0
    }

    #[test]
    fn context_surrounds_single_change() {
        let before = "a\nb\nc\nd\ne\nf\n";
        let after = "a\nb\nc\nD\ne\nf\n";
        let hunks = diff_from(before, after, 3);
        assert_eq!(hunks.len(), 1);
        let lines = lines_of(&hunks[0]);
        assert_eq!(
            lines,
            vec![
                ("ctx", "a".into()),
                ("ctx", "b".into()),
                ("ctx", "c".into()),
                ("rem", "d".into()),
                ("add", "D".into()),
                ("ctx", "e".into()),
                ("ctx", "f".into()),
            ]
        );
    }

    #[test]
    fn tabs_expand_to_next_tab_stop() {
        let before = "a\n\tb\n";
        let after = "a\n\tB\n";
        let hunks = diff_from(before, after, 0);
        assert_eq!(hunks.len(), 1);
        let lines = lines_of(&hunks[0]);
        let expected_pad: String = " ".repeat(TAB_WIDTH);
        assert!(lines.iter().any(|(k, v)| *k == "rem" && *v == format!("{expected_pad}b")));
        assert!(lines.iter().any(|(k, v)| *k == "add" && *v == format!("{expected_pad}B")));
    }

    #[test]
    fn crlf_line_endings_are_stripped() {
        let before = "a\r\nb\r\n";
        let after = "a\r\nB\r\n";
        let hunks = diff_from(before, after, 0);
        let lines = lines_of(&hunks[0]);
        // no \r or \n should survive into the rendered content
        for (_, content) in &lines {
            assert!(!content.contains('\n'));
            assert!(!content.contains('\r'));
        }
    }

    #[test]
    fn adjacent_changes_merge_into_one_hunk() {
        let before = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let after = "a\nB\nc\nd\ne\nf\nG\nh\n";
        let hunks = diff_from(before, after, 3);
        assert_eq!(hunks.len(), 1, "context should merge the two changes");
        let lines = lines_of(&hunks[0]);
        let removed: Vec<_> = lines.iter().filter(|(k, _)| *k == "rem").collect();
        let added: Vec<_> = lines.iter().filter(|(k, _)| *k == "add").collect();
        assert_eq!(removed.len(), 2);
        assert_eq!(added.len(), 2);
    }

    #[test]
    fn far_apart_changes_stay_separate() {
        let before = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\np\nq\nr\ns\nt\n";
        let after = "A\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\nm\nn\no\np\nq\nr\ns\nT\n";
        let hunks = diff_from(before, after, 3);
        assert_eq!(hunks.len(), 2);
    }
}
