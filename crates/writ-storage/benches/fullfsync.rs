//! What `F_FULLFSYNC` would cost a save, measured rather than assumed.
//!
//! `write_atomic` calls `File::sync_all`, which on macOS is `fsync(2)`: the
//! bytes reach the drive's cache, not the platter. `fcntl(F_FULLFSYNC)` adds
//! the barrier that makes the write survive a power cut, and Apple documents
//! it as much slower. How much slower decides whether Writ can afford it on
//! the autosave path, so this bench measures it. Nothing here is adopted in
//! 0.4 (ADR-028 §8); the numbers are the deliverable.
//!
//! Two passes:
//!
//! * the cadence pass, which writes one file per second the way autosave does
//!   and reports p50/p95/p99 of each write. This is the number that matters:
//!   a barrier hit back to back is not the barrier a user's save hits.
//! * the criterion pass, which reports the mean cost of a single write with
//!   no pause between iterations.
//!
//! Both run against `WRIT_FSYNC_BENCH_DIR` (default: the system temporary
//! directory), so the same binary measures internal APFS, an external USB
//! volume and a network mount. `WRIT_FSYNC_BENCH_SAMPLES` sets the cadence
//! pass sample count (default 120); `0` skips that pass.
//!
//! ```text
//! WRIT_FSYNC_BENCH_DIR=/Volumes/usb cargo bench -p writ-storage --bench fullfsync
//! ```

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use criterion::Criterion;
use tempfile::NamedTempFile;

/// A note-sized payload. Big enough that the write is not free, small enough
/// that the barrier dominates, which is the shape of a real save.
const PAYLOAD_BYTES: usize = 4096;

/// How often autosave writes at most (U10's `AUTOSAVE_DELAY`).
const CADENCE: Duration = Duration::from_secs(1);

/// Cadence-pass sample count when `WRIT_FSYNC_BENCH_SAMPLES` is unset.
///
/// A p99 needs at least a hundred samples to be a p99 rather than the largest
/// sample under another name, and the tail is the number the barrier decision
/// turns on. At one write per second this is a two-minute pass per barrier.
const DEFAULT_SAMPLES: usize = 120;

/// Whether the barrier is asked for on top of the ordinary flush.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Barrier {
    /// `File::sync_all`, which is what `write_atomic` does today.
    SyncAll,
    /// `File::sync_all` plus `fcntl(F_FULLFSYNC)`.
    FullFsync,
}

impl Barrier {
    fn label(self) -> &'static str {
        match self {
            Barrier::SyncAll => "sync_all",
            Barrier::FullFsync => "sync_all + F_FULLFSYNC",
        }
    }
}

/// Asks the drive to flush its own cache.
///
/// Only macOS has `F_FULLFSYNC`. Elsewhere the call is a no-op and the two
/// modes measure the same thing, which the report says out loud rather than
/// pretending to a number.
#[cfg(target_os = "macos")]
fn full_fsync(file: &File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;

    // SAFETY: the descriptor is owned by `file` and outlives the call.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn full_fsync(_file: &File) -> io::Result<()> {
    Ok(())
}

/// `write_atomic`'s shape, with the barrier under test.
///
/// Copied rather than called so the two modes differ in exactly one `fcntl`.
/// The permission inheritance `write_atomic` does is left out: it reads
/// metadata off the destination and would put a `stat` inside the timed
/// region without changing what is being compared.
fn write_atomic_with(target: &Path, bytes: &[u8], barrier: Barrier) -> io::Result<()> {
    let dir = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?;

    let mut tmp = NamedTempFile::new_in(dir)?;
    tmp.as_file_mut().write_all(bytes)?;
    tmp.as_file_mut().sync_all()?;
    if barrier == Barrier::FullFsync {
        full_fsync(tmp.as_file())?;
    }

    tmp.persist(target).map_err(|e| e.error)?;

    if let Ok(dir_handle) = File::open(dir) {
        let _ = dir_handle.sync_all();
        if barrier == Barrier::FullFsync {
            let _ = full_fsync(&dir_handle);
        }
    }

    Ok(())
}

fn bench_dir() -> PathBuf {
    std::env::var_os("WRIT_FSYNC_BENCH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn payload() -> Vec<u8> {
    b"writ fullfsync criterion payload\n"
        .iter()
        .copied()
        .cycle()
        .take(PAYLOAD_BYTES)
        .collect()
}

/// Linear interpolation between the two samples the rank falls between, the
/// way `numpy.percentile` and the usual latency tooling do it.
///
/// Rounding to the nearest sample instead reports the largest sample as the
/// p99 whenever there are fewer than a hundred of them, which reads as a
/// tail measurement and is not one.
fn percentile(sorted: &[Duration], fraction: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let last = sorted.len() - 1;
    let rank = fraction.clamp(0.0, 1.0) * last as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        return sorted[lo.min(last)];
    }
    let low = sorted[lo.min(last)];
    let high = sorted[hi.min(last)];
    low + Duration::from_secs_f64((high - low).as_secs_f64() * (rank - lo as f64))
}

/// Writes `samples` files one second apart and returns every write's duration.
fn cadence_samples(dir: &Path, barrier: Barrier, samples: usize) -> io::Result<Vec<Duration>> {
    let bytes = payload();
    let target = dir.join("writ-fullfsync-bench.tmp");
    let mut out = Vec::with_capacity(samples);

    for i in 0..samples {
        if i > 0 {
            std::thread::sleep(CADENCE);
        }
        let started = Instant::now();
        write_atomic_with(&target, &bytes, barrier)?;
        out.push(started.elapsed());
    }

    let _ = std::fs::remove_file(&target);
    out.sort_unstable();
    Ok(out)
}

fn report_cadence() {
    let samples = std::env::var("WRIT_FSYNC_BENCH_SAMPLES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(DEFAULT_SAMPLES);
    if samples == 0 {
        return;
    }

    let dir = bench_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        println!("fullfsync: cannot write in {}", dir.display());
        return;
    }

    println!(
        "\nfullfsync cadence pass: {} samples, one write per {:?}, in {}",
        samples,
        CADENCE,
        dir.display()
    );
    if cfg!(not(target_os = "macos")) {
        println!("fullfsync: F_FULLFSYNC exists on macOS only; both rows measure sync_all here");
    }

    for barrier in [Barrier::SyncAll, Barrier::FullFsync] {
        match cadence_samples(&dir, barrier, samples) {
            Ok(sorted) => println!(
                "  {:<22}  p50 {:>9.3?}  p95 {:>9.3?}  p99 {:>9.3?}  max {:>9.3?}",
                barrier.label(),
                percentile(&sorted, 0.50),
                percentile(&sorted, 0.95),
                percentile(&sorted, 0.99),
                sorted.last().copied().unwrap_or(Duration::ZERO),
            ),
            Err(error) => println!("  {:<22}  failed: {error}", barrier.label()),
        }
    }
    println!();
}

fn bench_write_atomic(c: &mut Criterion) {
    let dir = bench_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let bytes = payload();
    let target = dir.join("writ-fullfsync-criterion.tmp");

    let mut group = c.benchmark_group("write_atomic");
    // A barrier costs milliseconds, so the default hundred samples would run
    // for minutes and wear the drive for no extra signal.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(5));

    for barrier in [Barrier::SyncAll, Barrier::FullFsync] {
        group.bench_function(barrier.label(), |b| {
            b.iter(|| write_atomic_with(&target, &bytes, barrier).expect("write"));
        });
    }
    group.finish();

    let _ = std::fs::remove_file(&target);
}

fn main() {
    let skip_cadence = std::env::args().any(|arg| arg == "--test" || arg == "--list");
    if !skip_cadence {
        report_cadence();
    }

    let mut criterion = Criterion::default().configure_from_args();
    bench_write_atomic(&mut criterion);
    criterion.final_summary();
}
