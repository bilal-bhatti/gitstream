use crate::error::{Error, Result};
use crate::state::{ChangeKind, DiffUpdate, Hunk, HunkLine};
use crate::watcher::WatchEvent;
use crossbeam_channel::{Receiver, Sender};
use imara_diff::{Algorithm, Diff, InternedInput};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::SystemTime;

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
    ev_rx: Receiver<WatchEvent>,
    up_tx: Sender<DiffUpdate>,
) -> Result<WorkerGuard> {
    let engine = Arc::new(Engine::open(&repo_root)?);

    let initial_engine = Arc::clone(&engine);
    let initial_tx = up_tx.clone();
    thread::spawn(move || {
        match initial_engine.initial_scan() {
            Ok(updates) => {
                for u in updates {
                    if initial_tx.send(u).is_err() {
                        return;
                    }
                }
            }
            Err(err) => {
                tracing::error!(error = %err, "initial scan failed");
            }
        }
    });

    let handle = thread::Builder::new()
        .name("diff-worker".into())
        .spawn(move || run(engine, ev_rx, up_tx))
        .map_err(|e| Error::Io {
            path: repo_root.clone(),
            source: e,
        })?;

    Ok(WorkerGuard {
        handle: Some(handle),
    })
}

fn run(engine: Arc<Engine>, ev_rx: Receiver<WatchEvent>, up_tx: Sender<DiffUpdate>) {
    while let Ok(ev) = ev_rx.recv() {
        match engine.recompute(&ev.path) {
            Ok(Some(update)) => {
                if up_tx.send(update).is_err() {
                    tracing::debug!("update channel closed; worker exiting");
                    return;
                }
            }
            Ok(None) => {
                let cleared = DiffUpdate {
                    path: relative_or_absolute(&engine.repo_root, &ev.path),
                    mtime: SystemTime::now(),
                    status: ChangeKind::Modified,
                    hunks: vec![],
                    added: 0,
                    removed: 0,
                    binary: false,
                };
                if up_tx.send(cleared).is_err() {
                    return;
                }
            }
            Err(err) => {
                tracing::error!(error = %err, path = %ev.path.display(), "diff recompute failed");
            }
        }
    }
}

struct Engine {
    repo_root: PathBuf,
    repo: gix::ThreadSafeRepository,
}

impl Engine {
    fn open(repo_root: &Path) -> Result<Self> {
        let repo = gix::open(repo_root).map_err(|e| Error::RepoOpen {
            path: repo_root.to_path_buf(),
            source: Box::new(e),
        })?;
        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            repo: repo.into_sync(),
        })
    }

    fn initial_scan(&self) -> Result<Vec<DiffUpdate>> {
        let repo = self.repo.to_thread_local();
        let platform = repo
            .status(gix::progress::Discard)
            .map_err(|e| Error::Diff {
                path: self.repo_root.clone(),
                source: Box::new(e),
            })?;

        let iter = platform
            .into_iter(None)
            .map_err(|e| Error::Diff {
                path: self.repo_root.clone(),
                source: Box::new(e),
            })?;

        let mut paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for item in iter {
            let item = match item {
                Ok(i) => i,
                Err(err) => {
                    tracing::error!(error = %err, "status iterator error");
                    continue;
                }
            };
            let location = item.location();
            let rel = bstr_to_path(location);
            paths.insert(rel);
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

        let binary = looks_binary(&before) || looks_binary(&after);
        if binary {
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

        let mut hunks = Vec::new();
        let mut added_total: u32 = 0;
        let mut removed_total: u32 = 0;
        for h in diff.hunks() {
            let before_lines: Vec<HunkLine> = (h.before.start..h.before.end)
                .map(|i| {
                    let token = input.before[i as usize];
                    let s = input.interner[token];
                    HunkLine::Removed(s.to_string())
                })
                .collect();
            let after_lines: Vec<HunkLine> = (h.after.start..h.after.end)
                .map(|i| {
                    let token = input.after[i as usize];
                    let s = input.interner[token];
                    HunkLine::Added(s.to_string())
                })
                .collect();

            removed_total = removed_total.saturating_add(before_lines.len() as u32);
            added_total = added_total.saturating_add(after_lines.len() as u32);

            let mut lines = before_lines;
            lines.extend(after_lines);

            hunks.push(Hunk {
                old_range: (h.before.start, h.before.end - h.before.start),
                new_range: (h.after.start, h.after.end - h.after.start),
                lines,
            });
        }

        Ok(Some(DiffUpdate {
            path: rel,
            mtime,
            status,
            hunks,
            added: added_total,
            removed: removed_total,
            binary: false,
        }))
    }

    fn read_head_blob(&self, rel: &Path) -> Result<Option<Vec<u8>>> {
        let repo = self.repo.to_thread_local();
        let head_id = match repo.head_tree_id_or_empty() {
            Ok(id) => id,
            Err(err) => {
                return Err(Error::Diff {
                    path: rel.to_path_buf(),
                    source: Box::new(err),
                });
            }
        };
        if head_id.is_empty_tree() {
            return Ok(None);
        }
        let tree = match repo.find_object(head_id) {
            Ok(o) => o.into_tree(),
            Err(err) => {
                return Err(Error::Diff {
                    path: rel.to_path_buf(),
                    source: Box::new(err),
                });
            }
        };
        let entry = match tree.lookup_entry_by_path(rel) {
            Ok(Some(e)) => e,
            Ok(None) => return Ok(None),
            Err(err) => {
                return Err(Error::Diff {
                    path: rel.to_path_buf(),
                    source: Box::new(err),
                });
            }
        };
        if !entry.mode().is_blob() {
            return Ok(None);
        }
        let blob = match entry.object() {
            Ok(o) => o.into_blob(),
            Err(err) => {
                return Err(Error::Diff {
                    path: rel.to_path_buf(),
                    source: Box::new(err),
                });
            }
        };
        Ok(Some(blob.data.clone()))
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::File::open(path) {
        Ok(mut f) => {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).map_err(|e| Error::Io {
                path: path.to_path_buf(),
                source: e,
            })?;
            Ok(Some(buf))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Io {
            path: path.to_path_buf(),
            source: e,
        }),
    }
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
