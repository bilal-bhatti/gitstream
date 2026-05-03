pub mod app;
pub mod diff;
pub mod error;
pub mod render;
pub mod state;
pub mod watcher;

pub use error::{Error, Result};

/// Version string stamped at build time by `build.rs` from `git describe`.
/// Always carries a `v` prefix; falls back to `v{CARGO_PKG_VERSION}` when git
/// is unavailable.
pub const VERSION: &str = env!("GITSTREAM_VERSION");

/// Short commit hash stamped at build time, or empty when git is unavailable.
pub const COMMIT: &str = env!("GITSTREAM_COMMIT");

/// Build timestamp in RFC 3339 (local timezone offset, seconds precision).
pub const BUILT: &str = env!("GITSTREAM_BUILT");
