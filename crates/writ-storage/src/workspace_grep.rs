//! On-demand content search over the open workspace folder (ADR-026).
//!
//! A parallel walk (the shared [`crate::workspace_search::build_walk`], so the
//! ignore policy matches name search) greps every file for a literal match of
//! the user's query. Each walk visitor owns an [`mpsc::Sender`]; a single
//! collector thread drains the receiver, batches hits, and drives the caller's
//! `on_batch` sink. Keeping the sink on one thread lets the batch boundary be a
//! real timer instead of a per-thread guess, and lets the walk fan out freely.
//!
//! Two independent stop conditions:
//! - **cancellation** — the `cancelled` closure (generation comparison, in the
//!   Tauri layer) is checked between files and between matches; when a newer
//!   query supersedes this one the walk quits and the outcome reports
//!   `cancelled`.
//! - **the result cap** — a separate `cap_reached` flag flips at
//!   `max_results`, quits the walk, and reports `truncated`, not `cancelled`.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::WalkState;
use serde::Serialize;
use writ_core::search::{content_snippet, SnippetSegment};

use crate::errors::{StorageError, StorageResult};
use crate::workspace_search::build_walk;

/// A parameterized content search request.
pub struct GrepRequest {
    /// Canonical workspace root to search under.
    pub root: PathBuf,
    /// Raw user query, matched as a case-insensitive literal.
    pub query: String,
    /// Monotonic generation this search belongs to (for cancellation).
    pub generation: u64,
}

/// Bounds on a single content search.
#[derive(Debug, Clone, Copy)]
pub struct GrepLimits {
    /// Files larger than this are skipped.
    pub max_file_bytes: u64,
    /// Total hits after which the search stops and reports `truncated`.
    pub max_results: usize,
    /// Longest matched-line snippet in bytes; longer lines are elided, not
    /// dropped.
    pub max_line_bytes: usize,
}

impl Default for GrepLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 2 * 1024 * 1024,
            max_results: 500,
            max_line_bytes: 512,
        }
    }
}

/// One content-search match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContentHit {
    /// Workspace-relative path of the file (forward-slash separated).
    pub path: String,
    /// 1-based line number of the match.
    pub line: u64,
    /// Highlighted, length-capped snippet of the matched line.
    pub snippet: Vec<SnippetSegment>,
}

/// Terminal report for a content search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GrepOutcome {
    /// Number of hits delivered (never exceeds `max_results`).
    pub hit_count: usize,
    /// Number of files actually searched.
    pub files_scanned: usize,
    /// `true` when the result cap was hit and matches remain.
    pub truncated: bool,
    /// `true` when a newer query cancelled this one before it finished.
    pub cancelled: bool,
}

/// Flush a batch after this many hits accumulate.
const FLUSH_LEN: usize = 32;
/// Flush a partial batch after this long without reaching [`FLUSH_LEN`].
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);

/// Escapes regex metacharacters so the query matches as a literal.
fn escape_literal(query: &str) -> String {
    const META: &str = r"\.^$|()[]{}*+?";
    let mut out = String::with_capacity(query.len() * 2);
    for c in query.chars() {
        if META.contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Searches `req.root` for `req.query`, streaming batches of hits to `on_batch`
/// and returning the terminal [`GrepOutcome`]. See the module docs for the
/// collector shape and stop conditions.
pub fn search_workspace_content(
    req: GrepRequest,
    limits: GrepLimits,
    cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
    on_batch: Box<dyn FnMut(Vec<ContentHit>) + Send>,
) -> StorageResult<GrepOutcome> {
    run_search(req, limits, cancelled, on_batch, FLUSH_LEN, FLUSH_INTERVAL)
}

fn run_search(
    req: GrepRequest,
    limits: GrepLimits,
    cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
    on_batch: Box<dyn FnMut(Vec<ContentHit>) + Send>,
    flush_len: usize,
    flush_interval: Duration,
) -> StorageResult<GrepOutcome> {
    if req.query.trim().is_empty() {
        return Ok(GrepOutcome {
            hit_count: 0,
            files_scanned: 0,
            truncated: false,
            cancelled: cancelled(),
        });
    }

    let matcher = RegexMatcherBuilder::new()
        .case_insensitive(true)
        .line_terminator(Some(b'\n'))
        .build(&escape_literal(&req.query))
        .map_err(|e| StorageError::Search(e.to_string()))?;

    let (tx, rx) = mpsc::channel::<ContentHit>();
    let cap_reached = Arc::new(AtomicBool::new(false));
    let files_scanned = Arc::new(AtomicUsize::new(0));
    let max_results = limits.max_results;

    let collector = {
        let cap_reached = cap_reached.clone();
        let cancelled = cancelled.clone();
        let mut on_batch = on_batch;
        std::thread::spawn(move || {
            run_collector(
                rx,
                max_results,
                flush_len,
                flush_interval,
                cap_reached,
                cancelled,
                on_batch.as_mut(),
            )
        })
    };

    let root = req.root.clone();
    let query = req.query.clone();
    let max_file_bytes = limits.max_file_bytes;
    let max_line_bytes = limits.max_line_bytes;

    build_walk(&req.root).build_parallel().run(|| {
        let tx = tx.clone();
        let cancelled = cancelled.clone();
        let cap_reached = cap_reached.clone();
        let files_scanned = files_scanned.clone();
        let matcher = matcher.clone();
        let root = root.clone();
        let query = query.clone();
        let mut searcher = build_searcher();
        Box::new(move |result| {
            if cancelled() || cap_reached.load(Ordering::Relaxed) {
                return WalkState::Quit;
            }
            let Ok(entry) = result else {
                return WalkState::Continue;
            };
            match entry.file_type() {
                Some(ft) if ft.is_file() => {}
                _ => return WalkState::Continue,
            }
            if let Ok(md) = entry.metadata() {
                if md.len() > max_file_bytes {
                    return WalkState::Continue;
                }
            }
            let Some(rel) = relative_display(&root, entry.path()) else {
                return WalkState::Continue;
            };
            files_scanned.fetch_add(1, Ordering::Relaxed);

            let mut sink = HitSink {
                path: rel,
                query: &query,
                max_line_bytes,
                tx: &tx,
                cancelled: cancelled.as_ref(),
                cap_reached: &cap_reached,
            };
            let _ = searcher.search_path(&matcher, entry.path(), &mut sink);

            if cancelled() || cap_reached.load(Ordering::Relaxed) {
                WalkState::Quit
            } else {
                WalkState::Continue
            }
        })
    });

    drop(tx);
    let (hit_count, truncated) = collector.join().unwrap_or((0, false));

    Ok(GrepOutcome {
        hit_count,
        files_scanned: files_scanned.load(Ordering::Relaxed),
        truncated,
        cancelled: cancelled(),
    })
}

fn build_searcher() -> Searcher {
    SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
        .build()
}

/// Workspace-relative, forward-slash display path for a hit.
fn relative_display(root: &std::path::Path, path: &std::path::Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    if rel.as_os_str().is_empty() {
        return None;
    }
    let mut s = rel.to_string_lossy().into_owned();
    if std::path::MAIN_SEPARATOR == '\\' {
        s = s.replace('\\', "/");
    }
    Some(s)
}

/// Per-file grep sink: turns each matched line into a [`ContentHit`] and sends
/// it to the collector. Stops the file early when cancelled or the cap is hit.
struct HitSink<'a> {
    path: String,
    query: &'a str,
    max_line_bytes: usize,
    tx: &'a mpsc::Sender<ContentHit>,
    cancelled: &'a (dyn Fn() -> bool + Send + Sync),
    cap_reached: &'a AtomicBool,
}

impl Sink for HitSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, io::Error> {
        if (self.cancelled)() || self.cap_reached.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let line = mat.line_number().unwrap_or(0);
        let text = String::from_utf8_lossy(mat.bytes());
        let text = text.trim_end_matches(['\n', '\r']);
        let hit = ContentHit {
            path: self.path.clone(),
            line,
            snippet: content_snippet(text, self.query, self.max_line_bytes),
        };
        // A closed receiver means the collector stopped; end this file.
        Ok(self.tx.send(hit).is_ok())
    }
}

/// Drains `rx`, batching hits and invoking `on_batch`. Flushes on `flush_len`
/// hits or after `flush_interval`, whichever comes first, using a moving
/// deadline so a steady stream still flushes on time. Stops forwarding once
/// `max_results` hits are counted (setting `cap_reached` and `truncated`) but
/// keeps draining so producers never block. Returns `(hit_count, truncated)`.
fn run_collector(
    rx: mpsc::Receiver<ContentHit>,
    max_results: usize,
    flush_len: usize,
    flush_interval: Duration,
    cap_reached: Arc<AtomicBool>,
    cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
    on_batch: &mut dyn FnMut(Vec<ContentHit>),
) -> (usize, bool) {
    let mut batch: Vec<ContentHit> = Vec::new();
    let mut hit_count = 0usize;
    let mut truncated = false;
    let mut deadline = Instant::now() + flush_interval;

    loop {
        if cancelled() {
            break;
        }
        let timeout = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(timeout) {
            Ok(hit) => {
                if hit_count >= max_results {
                    truncated = true;
                    cap_reached.store(true, Ordering::Relaxed);
                    continue;
                }
                batch.push(hit);
                hit_count += 1;
                if hit_count >= max_results {
                    truncated = true;
                    cap_reached.store(true, Ordering::Relaxed);
                }
                if batch.len() >= flush_len {
                    on_batch(std::mem::take(&mut batch));
                    deadline = Instant::now() + flush_interval;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !batch.is_empty() {
                    on_batch(std::mem::take(&mut batch));
                }
                deadline = Instant::now() + flush_interval;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if !batch.is_empty() {
                    on_batch(std::mem::take(&mut batch));
                }
                break;
            }
        }
    }

    (hit_count, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::TempDir;

    fn write(dir: &std::path::Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    /// Collects every batch into a flat list of hits, with the outcome.
    fn run(
        root: &std::path::Path,
        query: &str,
        limits: GrepLimits,
    ) -> (Vec<ContentHit>, GrepOutcome) {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let sink_hits = hits.clone();
        let outcome = search_workspace_content(
            GrepRequest {
                root: root.to_path_buf(),
                query: query.to_string(),
                generation: 1,
            },
            limits,
            Arc::new(|| false),
            Box::new(move |batch| sink_hits.lock().unwrap().extend(batch)),
        )
        .unwrap();
        let collected = Arc::try_unwrap(hits).unwrap().into_inner().unwrap();
        (collected, outcome)
    }

    #[test]
    fn escape_literal_neutralizes_regex_metacharacters() {
        assert_eq!(escape_literal("a.b*c"), r"a\.b\*c");
        assert_eq!(escape_literal("fn main()"), r"fn main\(\)");
        assert_eq!(escape_literal("plain"), "plain");
    }

    #[test]
    fn finds_a_literal_match_with_line_number() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "a.rs", "one\nlet token = 1;\nthree");
        let (hits, outcome) = run(dir.path(), "token", GrepLimits::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "a.rs");
        assert_eq!(hits[0].line, 2);
        assert!(!outcome.cancelled);
        assert!(!outcome.truncated);
        assert_eq!(outcome.hit_count, 1);
    }

    #[test]
    fn query_is_matched_as_a_literal_not_a_regex() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "a.txt", "value a.c here\nvalue axc here");
        // "a.c" as a literal matches only the first line, not "axc".
        let (hits, _) = run(dir.path(), "a.c", GrepLimits::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 1);
    }

    #[test]
    fn gitignored_and_writ_ignored_files_are_skipped() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), ".gitignore", "secret.txt\n");
        write(dir.path(), "secret.txt", "needle here");
        write(dir.path(), "node_modules/x.js", "needle here");
        write(dir.path(), "keep.rs", "needle here");
        let (hits, _) = run(dir.path(), "needle", GrepLimits::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "keep.rs");
    }

    #[test]
    fn dotfiles_are_searched() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), ".env", "SECRET=needle");
        let (hits, _) = run(dir.path(), "needle", GrepLimits::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, ".env");
    }

    #[test]
    fn binary_files_are_skipped() {
        let dir = TempDir::new().unwrap();
        let mut body = b"needle".to_vec();
        body.push(0);
        body.extend_from_slice(b"needle");
        fs::write(dir.path().join("blob.bin"), &body).unwrap();
        write(dir.path(), "text.txt", "needle");
        let (hits, _) = run(dir.path(), "needle", GrepLimits::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "text.txt");
    }

    #[test]
    fn oversize_files_are_skipped() {
        let dir = TempDir::new().unwrap();
        let big = format!("needle {}", "x".repeat(4096));
        write(dir.path(), "big.txt", &big);
        write(dir.path(), "small.txt", "needle");
        let limits = GrepLimits {
            max_file_bytes: 64,
            ..GrepLimits::default()
        };
        let (hits, _) = run(dir.path(), "needle", limits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "small.txt");
    }

    #[test]
    fn long_matched_line_is_elided_not_dropped() {
        let dir = TempDir::new().unwrap();
        let line = format!("{} needle tail", "x".repeat(4000));
        write(dir.path(), "wide.txt", &line);
        let (hits, _) = run(dir.path(), "needle", GrepLimits::default());
        assert_eq!(hits.len(), 1, "the hit must not be dropped");
        let text: String = hits[0].snippet.iter().map(|s| s.text.as_str()).collect();
        assert!(text.contains("needle"));
        assert!(text.len() <= 512 + "…".len() * 2);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escaping_the_root_is_not_followed() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        write(outside.path(), "secret.txt", "needle outside");
        symlink(outside.path(), dir.path().join("link")).unwrap();
        write(dir.path(), "inside.txt", "needle inside");

        let (hits, _) = run(dir.path(), "needle", GrepLimits::default());
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "inside.txt");
    }

    #[test]
    fn result_cap_reports_truncated() {
        let dir = TempDir::new().unwrap();
        for i in 0..20 {
            write(dir.path(), &format!("f{i}.txt"), "needle");
        }
        let limits = GrepLimits {
            max_results: 5,
            ..GrepLimits::default()
        };
        let (hits, outcome) = run(dir.path(), "needle", limits);
        assert_eq!(hits.len(), 5);
        assert!(outcome.truncated);
        assert!(!outcome.cancelled);
        assert_eq!(outcome.hit_count, 5);
    }

    #[test]
    fn cancellation_mid_walk_stops_and_reports_cancelled() {
        let dir = TempDir::new().unwrap();
        for i in 0..200 {
            write(dir.path(), &format!("f{i}.txt"), "needle");
        }
        // Cancel after the first file is scanned.
        let seen = Arc::new(AtomicUsize::new(0));
        let seen_c = seen.clone();
        let cancelled: Arc<dyn Fn() -> bool + Send + Sync> =
            Arc::new(move || seen_c.fetch_add(1, Ordering::Relaxed) >= 1);
        let hits = Arc::new(Mutex::new(Vec::new()));
        let sink_hits = hits.clone();
        let outcome = search_workspace_content(
            GrepRequest {
                root: dir.path().to_path_buf(),
                query: "needle".to_string(),
                generation: 2,
            },
            GrepLimits::default(),
            cancelled,
            Box::new(move |batch| sink_hits.lock().unwrap().extend(batch)),
        )
        .unwrap();
        assert!(outcome.cancelled, "a cancelled walk must report cancelled");
        let collected = Arc::try_unwrap(hits).unwrap().into_inner().unwrap();
        assert!(
            collected.len() < 200,
            "cancellation must stop before every file is scanned, got {}",
            collected.len()
        );
    }

    #[test]
    fn empty_query_scans_nothing() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "a.txt", "content");
        let (hits, outcome) = run(dir.path(), "   ", GrepLimits::default());
        assert!(hits.is_empty());
        assert_eq!(outcome.files_scanned, 0);
    }

    #[test]
    fn collector_batches_by_count_under_concurrent_producers() {
        // Four producers, 25 hits each = 100 total. With a huge interval, every
        // flush is count-driven: batches of 32, 32, 32, then 4 at disconnect.
        let (tx, rx) = mpsc::channel::<ContentHit>();
        let mut producers = Vec::new();
        for p in 0..4 {
            let tx = tx.clone();
            producers.push(std::thread::spawn(move || {
                for i in 0..25 {
                    tx.send(ContentHit {
                        path: format!("p{p}-{i}.txt"),
                        line: 1,
                        snippet: Vec::new(),
                    })
                    .unwrap();
                }
            }));
        }
        drop(tx);

        let batches = Arc::new(Mutex::new(Vec::<usize>::new()));
        let batches_sink = batches.clone();
        let mut on_batch = move |batch: Vec<ContentHit>| {
            batches_sink.lock().unwrap().push(batch.len());
        };
        let (hit_count, truncated) = run_collector(
            rx,
            1000,
            32,
            Duration::from_secs(3600),
            Arc::new(AtomicBool::new(false)),
            Arc::new(|| false),
            &mut on_batch,
        );
        for producer in producers {
            producer.join().unwrap();
        }

        assert_eq!(hit_count, 100);
        assert!(!truncated);
        let sizes = batches.lock().unwrap().clone();
        assert_eq!(sizes.iter().sum::<usize>(), 100);
        // Every full batch is exactly 32; only the final flush is smaller.
        for (idx, &len) in sizes.iter().enumerate() {
            if idx + 1 < sizes.len() {
                assert_eq!(len, 32, "non-final batch {idx} was {len}");
            } else {
                assert!(len <= 32);
            }
        }
    }

    #[test]
    fn collector_flushes_partial_batch_on_disconnect() {
        let (tx, rx) = mpsc::channel::<ContentHit>();
        for i in 0..5 {
            tx.send(ContentHit {
                path: format!("{i}.txt"),
                line: 1,
                snippet: Vec::new(),
            })
            .unwrap();
        }
        drop(tx);
        let batches = Arc::new(Mutex::new(Vec::<usize>::new()));
        let batches_sink = batches.clone();
        let mut on_batch = move |batch: Vec<ContentHit>| {
            batches_sink.lock().unwrap().push(batch.len());
        };
        let (hit_count, _) = run_collector(
            rx,
            1000,
            32,
            Duration::from_secs(3600),
            Arc::new(AtomicBool::new(false)),
            Arc::new(|| false),
            &mut on_batch,
        );
        assert_eq!(hit_count, 5);
        assert_eq!(batches.lock().unwrap().clone(), vec![5]);
    }
}
