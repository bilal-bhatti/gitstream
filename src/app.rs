use crate::diff;
use crate::error::Result;
use crate::render;
use crate::watcher;
use std::path::PathBuf;

const CHANNEL_CAP: usize = 64;

pub fn run(repo_root: PathBuf) -> Result<()> {
    let (ev_tx, ev_rx) = crossbeam_channel::bounded::<watcher::WatchEvent>(CHANNEL_CAP);
    let (up_tx, up_rx) = crossbeam_channel::bounded::<crate::state::DiffUpdate>(CHANNEL_CAP);

    let _watcher_guard = watcher::spawn(&repo_root, ev_tx)?;
    let _worker_guard = diff::spawn_worker(repo_root.clone(), ev_rx, up_tx)?;

    render::run(up_rx)?;
    Ok(())
}
