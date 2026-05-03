use crate::error::{Error, Result};
use crossbeam_channel::Sender;
use notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    DebounceEventResult, Debouncer, RecommendedCache, new_debouncer,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeHint {
    Modify,
    Create,
    Remove,
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

pub fn spawn(repo_root: &Path, tx: Sender<WatchEvent>) -> Result<WatcherGuard> {
    let root = repo_root.to_path_buf();
    let mut debouncer = new_debouncer(
        DEBOUNCE_WINDOW,
        None,
        move |result: DebounceEventResult| match result {
            Ok(events) => {
                for ev in events {
                    let hint = classify(&ev.event.kind);
                    for path in &ev.event.paths {
                        if is_in_dotgit(path, &root) {
                            continue;
                        }
                        let watch_event = WatchEvent {
                            path: path.clone(),
                            kind: hint,
                            at: Instant::now(),
                        };
                        if tx.send(watch_event).is_err() {
                            tracing::debug!("watcher channel closed; stopping emission");
                            return;
                        }
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
    })?;

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

fn classify(kind: &EventKind) -> ChangeHint {
    match kind {
        EventKind::Create(_) => ChangeHint::Create,
        EventKind::Remove(_) => ChangeHint::Remove,
        _ => ChangeHint::Modify,
    }
}

fn is_in_dotgit(path: &Path, repo_root: &Path) -> bool {
    if let Ok(rel) = path.strip_prefix(repo_root) {
        rel.components().next().is_some_and(|c| c.as_os_str() == ".git")
    } else {
        path.components().any(|c| c.as_os_str() == ".git")
    }
}
