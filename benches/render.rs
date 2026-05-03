use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use gitstream::render::bench::render_lines;
use gitstream::state::{ChangeKind, DiffUpdate, Hunk, HunkLine, State};
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const DIFF_WIDTH: u16 = 120;

/// One synthetic file with `n_lines` diff lines (mostly Context, sprinkled
/// Added/Removed). Mirrors the rough shape of a typical edit hunk.
fn make_update(path: &str, n_lines: usize, mtime: SystemTime) -> DiffUpdate {
    let mut lines = Vec::with_capacity(n_lines);
    let mut added = 0u32;
    let mut removed = 0u32;
    for i in 0..n_lines {
        if i % 17 == 0 {
            lines.push(HunkLine::Added(format!("added line {i:05}")));
            added += 1;
        } else if i % 23 == 0 {
            lines.push(HunkLine::Removed(format!("removed line {i:05}")));
            removed += 1;
        } else {
            lines.push(HunkLine::Context(format!("context line {i:05}")));
        }
    }
    DiffUpdate {
        path: PathBuf::from(path),
        mtime,
        status: ChangeKind::Modified,
        hunks: vec![Hunk {
            old_range: (0, n_lines as u32),
            new_range: (0, n_lines as u32),
            lines,
        }],
        added,
        removed,
        binary: false,
    }
}

fn populate(n_files: usize, lines_per_file: usize) -> State {
    let mut state = State::new();
    let base = SystemTime::UNIX_EPOCH;
    for i in 0..n_files {
        let mtime = base + Duration::from_secs(i as u64);
        state.apply(make_update(
            &format!("file_{i:03}.rs"),
            lines_per_file,
            mtime,
        ));
    }
    state
}

fn bench_render_lines_few_files(c: &mut Criterion) {
    // small/typical: 5 files, varying line counts
    let mut group = c.benchmark_group("render_lines/few-files");
    for &lines in &[50usize, 500, 2000] {
        let state = populate(5, lines);
        let total = 5 * lines;
        group.throughput(criterion::Throughput::Elements(total as u64));
        group.bench_function(BenchmarkId::from_parameter(lines), |b| {
            b.iter(|| {
                let out = render_lines(black_box(&state), black_box(DIFF_WIDTH));
                black_box(out);
            });
        });
    }
    group.finish();
}

fn bench_render_lines_many_files(c: &mut Criterion) {
    // wide repo: lots of small diffs (rescan-burst shape)
    let mut group = c.benchmark_group("render_lines/many-files");
    for &n_files in &[20usize, 100, 500] {
        let state = populate(n_files, 30);
        group.throughput(criterion::Throughput::Elements((n_files * 30) as u64));
        group.bench_function(BenchmarkId::from_parameter(n_files), |b| {
            b.iter(|| {
                let out = render_lines(black_box(&state), black_box(DIFF_WIDTH));
                black_box(out);
            });
        });
    }
    group.finish();
}

fn bench_render_lines_one_giant(c: &mut Criterion) {
    // pathological: one huge diff (the case the reviewer worried about)
    let mut group = c.benchmark_group("render_lines/one-giant");
    for &lines in &[5_000usize, 20_000] {
        let state = populate(1, lines);
        group.throughput(criterion::Throughput::Elements(lines as u64));
        group.bench_function(BenchmarkId::from_parameter(lines), |b| {
            b.iter(|| {
                let out = render_lines(black_box(&state), black_box(DIFF_WIDTH));
                black_box(out);
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_render_lines_few_files,
    bench_render_lines_many_files,
    bench_render_lines_one_giant
);
criterion_main!(benches);
