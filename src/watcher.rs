use crate::error::{Error, Result};
use crossbeam_channel::Sender;
use notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    DebounceEventResult, Debouncer, RecommendedCache, new_debouncer,
};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeHint {
    Modify,
    Create,
    Remove,
    /// Repo metadata changed (HEAD, index, .gitignore, ...). Path is unused;
    /// receiver should rescan repo state rather than touching a single file.
    Rescan,
}

#[derive(Debug, Clone)]
pub struct WatchEvent {
    pub path: PathBuf,
    pub kind: ChangeHint,
    pub at: Instant,
}

pub struct WatcherGuard {
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

const DEBOUNCE_WINDOW: Duration = Duration::from_millis(100);

pub fn spawn(
    repo_root: &Path,
    repo: gix::ThreadSafeRepository,
    tx: Sender<WatchEvent>,
) -> Result<WatcherGuard> {
    let stack = build_excludes_stack(&repo, repo_root)?;
    let root = repo_root.to_path_buf();
    let repo_handle = repo.clone();

    let mut debouncer = {
        let mut stack = stack;
        new_debouncer(
            DEBOUNCE_WINDOW,
            None,
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    let local = repo_handle.to_thread_local();
                    let mut rescan_pending = false;

                    for ev in events {
                        let hint = classify(&ev.event.kind);
                        for path in &ev.event.paths {
                            match classify_path(path, &root) {
                                PathClass::DotGitMetadata => {
                                    rescan_pending = true;
                                    continue;
                                }
                                PathClass::DotGitIgnored => continue,
                                PathClass::OutsideRoot => continue,
                                PathClass::Worktree(rel) => {
                                    if rel.file_name() == Some(OsStr::new(".gitignore")) {
                                        match build_excludes_stack(&repo_handle, &root) {
                                            Ok(s) => {
                                                stack = s;
                                                tracing::info!(
                                                    path = %path.display(),
                                                    "excludes stack reloaded"
                                                );
                                            }
                                            Err(err) => {
                                                tracing::error!(
                                                    error = %err,
                                                    path = %path.display(),
                                                    "excludes reload failed"
                                                );
                                            }
                                        }
                                        rescan_pending = true;
                                        continue;
                                    }

                                    match stack.at_path(rel.as_path(), None, &local.objects) {
                                        Ok(platform) => {
                                            if platform.is_excluded() {
                                                continue;
                                            }
                                        }
                                        Err(err) => {
                                            tracing::debug!(
                                                error = %err,
                                                path = %path.display(),
                                                "excludes lookup failed; passing through"
                                            );
                                        }
                                    }
                                    let watch_event = WatchEvent {
                                        path: path.clone(),
                                        kind: hint,
                                        at: Instant::now(),
                                    };
                                    if tx.send(watch_event).is_err() {
                                        tracing::debug!(
                                            "watcher channel closed; stopping emission"
                                        );
                                        return;
                                    }
                                }
                            }
                        }
                    }

                    if rescan_pending {
                        let signal = WatchEvent {
                            path: PathBuf::new(),
                            kind: ChangeHint::Rescan,
                            at: Instant::now(),
                        };
                        if tx.send(signal).is_err() {
                            tracing::debug!("watcher channel closed; stopping emission");
                        }
                    }
                }
                Err(errs) => {
                    for err in errs {
                        tracing::error!(error = %err, "notify error");
                    }
                }
            },
        )
        .map_err(|e| Error::Watch {
            path: repo_root.to_path_buf(),
            source: e,
        })?
    };

    debouncer
        .watch(repo_root, RecursiveMode::Recursive)
        .map_err(|e| Error::Watch {
            path: repo_root.to_path_buf(),
            source: e,
        })?;

    tracing::info!(path = %repo_root.display(), "watch started");
    Ok(WatcherGuard {
        _debouncer: debouncer,
    })
}

fn build_excludes_stack(
    repo: &gix::ThreadSafeRepository,
    repo_root: &Path,
) -> Result<gix::worktree::Stack> {
    let local = repo.to_thread_local();
    let index = local
        .index_or_load_from_head_or_empty()
        .map_err(|e| Error::Excludes {
            path: repo_root.to_path_buf(),
            source: Box::new(e),
        })?;
    let stack = local
        .excludes(
            &index,
            None,
            gix::worktree::stack::state::ignore::Source::default(),
        )
        .map_err(|e| Error::Excludes {
            path: repo_root.to_path_buf(),
            source: Box::new(e),
        })?
        .detach();
    Ok(stack)
}

fn classify(kind: &EventKind) -> ChangeHint {
    match kind {
        EventKind::Create(_) => ChangeHint::Create,
        EventKind::Remove(_) => ChangeHint::Remove,
        _ => ChangeHint::Modify,
    }
}

enum PathClass {
    OutsideRoot,
    DotGitMetadata,
    DotGitIgnored,
    Worktree(PathBuf),
}

fn classify_path(path: &Path, repo_root: &Path) -> PathClass {
    let Ok(rel) = path.strip_prefix(repo_root) else {
        return PathClass::OutsideRoot;
    };
    let mut comps = rel.components();
    let Some(first) = comps.next() else {
        return PathClass::OutsideRoot;
    };
    if first.as_os_str() == ".git" {
        return classify_dotgit(comps.as_path());
    }
    PathClass::Worktree(rel.to_path_buf())
}

fn classify_dotgit(rel_in_dotgit: &Path) -> PathClass {
    let mut comps = rel_in_dotgit.components();
    let Some(first) = comps.next() else {
        return PathClass::DotGitIgnored;
    };
    let name = first.as_os_str();
    if matches!(
        name.to_str(),
        Some(
            "HEAD"
                | "ORIG_HEAD"
                | "MERGE_HEAD"
                | "FETCH_HEAD"
                | "CHERRY_PICK_HEAD"
                | "REVERT_HEAD"
                | "index"
                | "packed-refs"
                | "refs"
        )
    ) {
        PathClass::DotGitMetadata
    } else {
        PathClass::DotGitIgnored
    }
}
