use anyhow::{Context, Result};
use clap::Parser;
use std::path::{Path, PathBuf};
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
    let filter =
        EnvFilter::try_from_env("GITSTREAM_LOG").unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

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

fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut cur: &Path = start;
    loop {
        if cur.join(".git").exists() {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
}
