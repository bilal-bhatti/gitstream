use crossbeam_channel::bounded;
use gitstream::diff;
use gitstream::state::{ChangeKind, State};
use gitstream::watcher::{ChangeHint, WatchEvent};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn git(repo: &PathBuf, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(repo)
        .args(args)
        .status()
        .expect("git invoked from test setup");
    assert!(status.success(), "git {:?} failed", args);
}

fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().to_path_buf();
    git(&repo, &["init", "--quiet", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "test"]);
    fs::write(repo.join("a.txt"), "alpha\nbeta\ngamma\n").unwrap();
    fs::write(repo.join("b.txt"), "one\ntwo\nthree\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "--quiet", "-m", "init"]);
    dir
}

fn open_repo(repo: &PathBuf) -> gix::ThreadSafeRepository {
    gix::open(repo).expect("gix::open").into_sync()
}

fn drain_into_state(rx: &crossbeam_channel::Receiver<gitstream::state::DiffUpdate>, state: &mut State, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(remaining) {
            Ok(update) => state.apply(update),
            Err(_) => break,
        }
    }
}

#[test]
fn pipeline_picks_up_modifications_and_orders_by_mtime() {
    let repo_dir = init_repo();
    let repo = repo_dir.path().to_path_buf();

    let (ev_tx, ev_rx) = bounded::<WatchEvent>(64);
    let (up_tx, up_rx) = bounded(64);

    let _worker = diff::spawn_worker(repo.clone(), open_repo(&repo), ev_rx, up_tx).expect("worker");

    let mut state = State::new();

    // initial scan emits nothing — clean repo
    drain_into_state(&up_rx, &mut state, Duration::from_millis(300));
    assert_eq!(state.len(), 0, "clean repo: no entries expected");

    // modify a.txt, then b.txt — b should sort first
    fs::write(repo.join("a.txt"), "alpha\nbeta\ngamma\nDELTA\n").unwrap();
    ev_tx
        .send(WatchEvent {
            path: repo.join("a.txt"),
            kind: ChangeHint::Modify,
            at: Instant::now(),
        })
        .unwrap();

    std::thread::sleep(Duration::from_millis(50));

    fs::write(repo.join("b.txt"), "one\ntwo\nthree\nFOUR\n").unwrap();
    ev_tx
        .send(WatchEvent {
            path: repo.join("b.txt"),
            kind: ChangeHint::Modify,
            at: Instant::now(),
        })
        .unwrap();

    drain_into_state(&up_rx, &mut state, Duration::from_millis(500));

    let order: Vec<PathBuf> = state.iter_ordered().map(|u| u.path.clone()).collect();
    assert_eq!(order.len(), 2, "two changed files expected, got {:?}", order);
    assert_eq!(order[0], PathBuf::from("b.txt"), "b should be first (mtime desc)");
    assert_eq!(order[1], PathBuf::from("a.txt"));

    let a = state.get(&PathBuf::from("a.txt")).unwrap();
    assert!(matches!(a.status, ChangeKind::Modified));
    assert_eq!(a.added, 1);
    assert_eq!(a.removed, 0);
    assert!(!a.binary);

    drop(ev_tx);
}

#[test]
fn untracked_file_classified_as_untracked() {
    let repo_dir = init_repo();
    let repo = repo_dir.path().to_path_buf();

    let (ev_tx, ev_rx) = bounded::<WatchEvent>(64);
    let (up_tx, up_rx) = bounded(64);
    let _worker = diff::spawn_worker(repo.clone(), open_repo(&repo), ev_rx, up_tx).expect("worker");

    fs::write(repo.join("new.txt"), "fresh\n").unwrap();
    ev_tx
        .send(WatchEvent {
            path: repo.join("new.txt"),
            kind: ChangeHint::Create,
            at: Instant::now(),
        })
        .unwrap();

    let mut state = State::new();
    drain_into_state(&up_rx, &mut state, Duration::from_millis(500));

    let entry = state
        .get(&PathBuf::from("new.txt"))
        .expect("untracked file should appear in state");
    assert!(matches!(entry.status, ChangeKind::Untracked));
    assert_eq!(entry.added, 1);
    assert_eq!(entry.removed, 0);

    drop(ev_tx);
}

#[test]
fn rescan_signal_drops_entry_after_external_commit() {
    let repo_dir = init_repo();
    let repo = repo_dir.path().to_path_buf();

    let (ev_tx, ev_rx) = bounded::<WatchEvent>(64);
    let (up_tx, up_rx) = bounded(64);
    let _worker = diff::spawn_worker(repo.clone(), open_repo(&repo), ev_rx, up_tx).expect("worker");

    fs::write(repo.join("a.txt"), "alpha\nbeta\ngamma\nNEW\n").unwrap();
    ev_tx
        .send(WatchEvent {
            path: repo.join("a.txt"),
            kind: ChangeHint::Modify,
            at: Instant::now(),
        })
        .unwrap();

    let mut state = State::new();
    drain_into_state(&up_rx, &mut state, Duration::from_millis(500));
    assert_eq!(state.len(), 1, "modification should appear");

    // commit the change with an external git command — no watcher event fires
    // for the worktree file, but the watcher would normally observe .git/HEAD
    // and .git/index changes and emit a Rescan signal. Simulate that here.
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "--quiet", "-m", "external"]);
    ev_tx
        .send(WatchEvent {
            path: PathBuf::new(),
            kind: ChangeHint::Rescan,
            at: Instant::now(),
        })
        .unwrap();

    drain_into_state(&up_rx, &mut state, Duration::from_millis(500));
    assert_eq!(
        state.len(),
        0,
        "rescan should observe the new HEAD and drop the entry"
    );

    drop(ev_tx);
}

#[test]
fn gitignored_paths_are_not_emitted() {
    let repo_dir = init_repo();
    let repo = repo_dir.path().to_path_buf();
    fs::write(repo.join(".gitignore"), "ignored/\n*.log\n").unwrap();
    git(&repo, &["add", ".gitignore"]);
    git(&repo, &["commit", "--quiet", "-m", "ignore"]);

    fs::create_dir(repo.join("ignored")).unwrap();
    fs::write(repo.join("ignored/blob"), "x\n").unwrap();
    fs::write(repo.join("scratch.log"), "spam\n").unwrap();
    fs::write(repo.join("kept.txt"), "kept\n").unwrap();

    let (ev_tx, ev_rx) = bounded::<WatchEvent>(64);
    let (up_tx, up_rx) = bounded(64);
    let _worker =
        diff::spawn_worker(repo.clone(), open_repo(&repo), ev_rx, up_tx).expect("worker");

    let mut state = State::new();
    drain_into_state(&up_rx, &mut state, Duration::from_millis(500));

    let paths: Vec<PathBuf> = state.iter_ordered().map(|u| u.path.clone()).collect();
    assert!(
        paths.contains(&PathBuf::from("kept.txt")),
        "kept.txt should appear, got {:?}",
        paths
    );
    assert!(
        !paths.contains(&PathBuf::from("ignored/blob")),
        "ignored/blob should be filtered, got {:?}",
        paths
    );
    assert!(
        !paths.contains(&PathBuf::from("scratch.log")),
        "scratch.log should be filtered, got {:?}",
        paths
    );

    drop(ev_tx);
}

#[test]
fn revert_drops_entry() {
    let repo_dir = init_repo();
    let repo = repo_dir.path().to_path_buf();

    let (ev_tx, ev_rx) = bounded::<WatchEvent>(64);
    let (up_tx, up_rx) = bounded(64);
    let _worker = diff::spawn_worker(repo.clone(), open_repo(&repo), ev_rx, up_tx).expect("worker");

    fs::write(repo.join("a.txt"), "alpha\nbeta\ngamma\nNEW\n").unwrap();
    ev_tx
        .send(WatchEvent {
            path: repo.join("a.txt"),
            kind: ChangeHint::Modify,
            at: Instant::now(),
        })
        .unwrap();

    let mut state = State::new();
    drain_into_state(&up_rx, &mut state, Duration::from_millis(300));
    assert_eq!(state.len(), 1, "modification should be visible");

    fs::write(repo.join("a.txt"), "alpha\nbeta\ngamma\n").unwrap();
    ev_tx
        .send(WatchEvent {
            path: repo.join("a.txt"),
            kind: ChangeHint::Modify,
            at: Instant::now(),
        })
        .unwrap();

    drain_into_state(&up_rx, &mut state, Duration::from_millis(300));
    assert_eq!(state.len(), 0, "revert should drop entry");

    drop(ev_tx);
}

#[test]
fn gitattributes_marks_text_file_binary() {
    let repo_dir = init_repo();
    let repo = repo_dir.path().to_path_buf();
    fs::write(repo.join(".gitattributes"), "*.lock binary\n").unwrap();
    fs::write(repo.join("vendor.lock"), "this looks like text\n").unwrap();
    git(&repo, &["add", ".gitattributes", "vendor.lock"]);
    git(&repo, &["commit", "--quiet", "-m", "lock"]);

    let (ev_tx, ev_rx) = bounded::<WatchEvent>(64);
    let (up_tx, up_rx) = bounded(64);
    let _worker = diff::spawn_worker(repo.clone(), open_repo(&repo), ev_rx, up_tx).expect("worker");

    fs::write(repo.join("vendor.lock"), "still ascii\nbut changed\n").unwrap();
    ev_tx
        .send(WatchEvent {
            path: repo.join("vendor.lock"),
            kind: ChangeHint::Modify,
            at: Instant::now(),
        })
        .unwrap();

    let mut state = State::new();
    drain_into_state(&up_rx, &mut state, Duration::from_millis(500));

    let entry = state
        .get(&PathBuf::from("vendor.lock"))
        .expect("modified .lock file should appear in state");
    assert!(
        entry.binary,
        "*.lock binary in .gitattributes must override the byte scan"
    );
    assert!(entry.hunks.is_empty(), "binary files have no hunks");

    drop(ev_tx);
}
