use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a git repository: {path}", path = path.display())]
    NotARepo { path: PathBuf },

    #[error("watcher error at {path}: {source}", path = path.display())]
    Watch {
        path: PathBuf,
        #[source]
        source: notify::Error,
    },

    #[error("diff error at {path}: {source}", path = path.display())]
    Diff {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("io error at {path}: {source}", path = path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("terminal error: {source}")]
    Term {
        #[source]
        source: std::io::Error,
    },

    #[error("repository open error at {path}: {source}", path = path.display())]
    RepoOpen {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
