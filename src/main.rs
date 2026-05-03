use anyhow::{Context, Result};
use clap::Parser;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "gitstream",
    about = "real-time git diff watcher — scrolling diffs ordered by mtime"
)]
struct Cli {
    /// Repository root. Defaults to walking up from the current directory.
    #[arg(default_value = ".")]
    path: PathBuf,
}

fn main() -> Result<()> {
    init_tracing()?;

    let cli = Cli::parse();
    let start = cli
        .path
        .canonicalize()
        .with_context(|| format!("resolving path {}", cli.path.display()))?;
    let repo_root = find_repo_root(&start)
        .with_context(|| format!("not inside a git repository: {}", start.display()))?;

    tracing::info!(repo = %repo_root.display(), "starting");
    gitstream::app::run(repo_root)?;
    Ok(())
}

/// Tracing always writes to a log file, never to stderr — stderr writes corrupt
/// the ratatui alternate screen. Default file is /tmp/gitstream.log; override
/// with `GITSTREAM_LOG_FILE`. Filter via `GITSTREAM_LOG` (defaults to `off`).
fn init_tracing() -> Result<()> {
    let filter = EnvFilter::try_from_env("GITSTREAM_LOG").unwrap_or_else(|_| EnvFilter::new("off"));
    let path = std::env::var("GITSTREAM_LOG_FILE")
        .unwrap_or_else(|_| "/tmp/gitstream.log".to_string());
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening log file {path}"))?;

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(Mutex::new(file))
        .with_ansi(false)
        .init();
    Ok(())
}

fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut cur: &Path = start;
    loop {
        if cur.join(".git").exists() {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
}
