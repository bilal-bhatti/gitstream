use crate::diff;
use crate::error::{Error, Result};
use crate::render;
use crate::watcher;
use std::path::PathBuf;

/// Watcher → worker. fsnotify bursts (an editor save fires rename + write +
/// chmod) plus debouncing usually keep this small.
const EV_CHANNEL_CAP: usize = 64;

/// Worker → render. A `Rescan` of a dirty repo emits one `DiffUpdate` per
/// changed file in a tight loop, so this needs more headroom than the event
/// channel; render coalesces drained updates into a single frame.
const UP_CHANNEL_CAP: usize = 1024;

pub fn run(repo_root: PathBuf) -> Result<()> {
    let repo = gix::open(&repo_root)
        .map_err(|e| Error::RepoOpen {
            path: repo_root.clone(),
            source: Box::new(e),
        })?
        .into_sync();

    let (ev_tx, ev_rx) = crossbeam_channel::bounded::<watcher::WatchEvent>(EV_CHANNEL_CAP);
    let (up_tx, up_rx) = crossbeam_channel::bounded::<crate::state::DiffUpdate>(UP_CHANNEL_CAP);

    // Drop order is reverse declaration order: declare worker first so it drops
    // last. On quit the watcher drops first → ev_tx closed → worker's
    // ev_rx.recv() returns Err immediately → worker thread exits without
    // blocking the join in WorkerGuard::drop. Otherwise q would deadlock.
    let _worker_guard = diff::spawn_worker(repo_root.clone(), repo.clone(), ev_rx, up_tx)?;
    let _watcher_guard = watcher::spawn(&repo_root, repo, ev_tx)?;

    let repo_name = repo_root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repository")
        .to_string();
    render::run(&repo_name, &repo_root, up_rx)?;
    Ok(())
}
