// Stamp the binary with a version string read from git, mirroring the
// magefile.go ldflags approach used by alchemainhub/felix:
//
//   -X main.version=$(git describe --tags) -X main.commitHash=$(git rev-parse --short HEAD)
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
    let version = git(&["describe", "--tags", "--always", "--dirty"])
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_default();

    println!("cargo:rustc-env=GITSTREAM_VERSION={version}");
    println!("cargo:rustc-env=GITSTREAM_COMMIT={commit}");
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}
