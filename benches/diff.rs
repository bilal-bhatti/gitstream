use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use gitstream::diff::bench::{BenchEngine, open};
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::process::Command;

fn baseline(n_lines: usize) -> String {
    (0..n_lines)
        .map(|i| format!("line {i:05}\n"))
        .collect::<String>()
}

fn one_line_changed(n_lines: usize) -> String {
    (0..n_lines)
        .map(|i| {
            if i == n_lines / 2 {
                format!("CHANGED line {i:05}\n")
            } else {
                format!("line {i:05}\n")
            }
        })
        .collect::<String>()
}

fn scattered_changes(n_lines: usize) -> String {
    (0..n_lines)
        .map(|i| {
            if i % 50 == 0 {
                format!("CHANGED line {i:05}\n")
            } else {
                format!("line {i:05}\n")
            }
        })
        .collect::<String>()
}

fn make_repo(committed: &str) -> (tempfile::TempDir, PathBuf, BenchEngine) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().to_path_buf();
    fn run(repo: &PathBuf, args: &[&str]) {
        let s = Command::new("git")
            .current_dir(repo)
            .args(args)
            .status()
            .expect("git invoked");
        assert!(s.success(), "git {args:?} failed");
    }
    run(&repo, &["init", "--quiet", "-b", "main"]);
    run(&repo, &["config", "user.email", "bench@bench"]);
    run(&repo, &["config", "user.name", "bench"]);
    let file = repo.join("file.txt");
    fs::write(&file, committed).unwrap();
    run(&repo, &["add", "."]);
    run(&repo, &["commit", "--quiet", "-m", "init"]);
    let engine = open(&repo).expect("open engine");
    (dir, file, engine)
}

fn bench_single_line(c: &mut Criterion) {
    let mut group = c.benchmark_group("recompute/single-line");
    for &n in &[50usize, 500, 5000] {
        let committed = baseline(n);
        let modified = one_line_changed(n);
        let (dir, file, mut engine) = make_repo(&committed);
        fs::write(&file, &modified).unwrap();
        group.throughput(criterion::Throughput::Elements(n as u64));
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            b.iter(|| {
                let out = engine.recompute(black_box(&file)).expect("recompute");
                black_box(out);
            });
        });
        drop(dir);
    }
    group.finish();
}

fn bench_scattered(c: &mut Criterion) {
    let mut group = c.benchmark_group("recompute/scattered");
    for &n in &[500usize, 5000] {
        let committed = baseline(n);
        let modified = scattered_changes(n);
        let (dir, file, mut engine) = make_repo(&committed);
        fs::write(&file, &modified).unwrap();
        group.throughput(criterion::Throughput::Elements(n as u64));
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            b.iter(|| {
                let out = engine.recompute(black_box(&file)).expect("recompute");
                black_box(out);
            });
        });
        drop(dir);
    }
    group.finish();
}

fn bench_clean(c: &mut Criterion) {
    // Worktree matches HEAD — fast path: bytes equal, no imara-diff invocation.
    let mut group = c.benchmark_group("recompute/clean");
    for &n in &[500usize, 5000] {
        let committed = baseline(n);
        let (dir, file, mut engine) = make_repo(&committed);
        // file already matches HEAD; no modification.
        group.bench_function(BenchmarkId::from_parameter(n), |b| {
            b.iter(|| {
                let out = engine.recompute(black_box(&file)).expect("recompute");
                black_box(out);
            });
        });
        drop(dir);
    }
    group.finish();
}

criterion_group!(benches, bench_single_line, bench_scattered, bench_clean);
criterion_main!(benches);
