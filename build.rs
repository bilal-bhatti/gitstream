// Stamp the binary with version / commit / build date read from git and the
// system clock, mirroring the magefile.go ldflags approach used by
// alchemainhub/felix:
//
//   -X main.version=$(git describe --tags)
//   -X main.commitHash=$(git rev-parse --short HEAD)
//   -X main.buildDate=$(date +RFC3339)
//
// In Rust the equivalent is `cargo:rustc-env=NAME=VALUE` from a build script;
// consumers read the values at compile time via `env!(...)`.
//
// We deliberately do not emit any `cargo:rerun-if-changed` directives — that
// keeps cargo's default change-detection (re-run on any package file mtime
// change), which catches both source edits (dirty-state) and post-commit
// rebuilds. Emitting any rerun-if-* directive replaces the default scan with
// just the listed paths, which would miss working-tree edits.

use std::process::Command;

fn main() {
    // The git tag IS the version. `--abbrev=0` strips the `-N-gSHA` suffix
    // git describe normally appends after the tag, so a repo at v0.1.0+3
    // commits still reports v0.1.0. `--dirty` keeps the working-tree marker
    // so an uncommitted edit shows e.g. v0.1.0-dirty.
    let raw_tag = git(&["describe", "--tags", "--abbrev=0", "--dirty"]);
    let version = raw_tag
        .filter(|t| !t.is_empty())
        .map(prefix_v)
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let built = build_date();

    println!("cargo:rustc-env=GITSTREAM_VERSION={version}");
    println!("cargo:rustc-env=GITSTREAM_COMMIT={commit}");
    println!("cargo:rustc-env=GITSTREAM_BUILT={built}");
}

fn prefix_v(s: String) -> String {
    if s.starts_with('v') || s.starts_with('V') {
        s
    } else {
        format!("v{s}")
    }
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// RFC 3339 timestamp with local timezone offset: `2026-05-02T21:30:10-05:00`.
/// Shells out to `date(1)` (present on every Unix we care about) and
/// post-processes `%z`'s `+0500` form into `+05:00` because BSD date and GNU
/// date disagree on whether to emit the colon.
fn build_date() -> String {
    let raw = Command::new("date")
        .args(["+%Y-%m-%dT%H:%M:%S%z"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into());

    let bytes = raw.as_bytes();
    let len = bytes.len();
    if len >= 5
        && (bytes[len - 5] == b'+' || bytes[len - 5] == b'-')
        && bytes[len - 4..].iter().all(|b| b.is_ascii_digit())
    {
        let (head, tz) = raw.split_at(len - 5);
        let sign = &tz[..1];
        let hh = &tz[1..3];
        let mm = &tz[3..5];
        return format!("{head}{sign}{hh}:{mm}");
    }
    raw
}
