//! What a panic report may say (ADR-045).
//!
//! A panic message can quote the text Writ was working on: a string-slicing
//! panic carries up to 256 bytes of the string it sliced, and that string is
//! often a note. The report keeps what identifies a panic (where it happened,
//! on which thread, what kind of message it carried, how long the message was
//! and a digest prefix that groups repeats of the same message) and never the
//! message itself. The adapter supplies the facts and writes the file.

use std::any::Any;

use crate::hash::sha256_hex;

/// Hex characters of the message digest a report keeps.
pub const DIGEST_PREFIX_LEN: usize = 16;

/// The kind of value a panic carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadKind {
    /// A `&'static str`, from `panic!("literal")`.
    StaticStr,
    /// A `String`, from a formatted `panic!` and from the standard library's
    /// own panics, a slicing panic among them.
    String,
    /// Any other value, from `std::panic::panic_any`.
    Other,
}

impl PayloadKind {
    /// The label a report prints.
    pub fn label(self) -> &'static str {
        match self {
            PayloadKind::StaticStr => "&str",
            PayloadKind::String => "String",
            PayloadKind::Other => "other",
        }
    }
}

/// A panic, described without its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicSummary {
    /// `file:line:column` of the panic, when the runtime knows it.
    pub location: Option<String>,
    /// The panicking thread's name, when it has one.
    pub thread: Option<String>,
    /// The kind of value the panic carried.
    pub payload_kind: PayloadKind,
    /// The message's length in bytes; `None` for a payload that is not text.
    pub payload_len: Option<usize>,
    /// The first [`DIGEST_PREFIX_LEN`] hex characters of the message's
    /// SHA-256; `None` for a payload that is not text.
    pub payload_digest: Option<String>,
}

impl PanicSummary {
    /// Describes the panic that carried `payload`.
    pub fn new(
        payload: &(dyn Any + Send),
        location: Option<String>,
        thread: Option<String>,
    ) -> Self {
        let (payload_kind, message) = if let Some(text) = payload.downcast_ref::<&'static str>() {
            (PayloadKind::StaticStr, Some(*text))
        } else if let Some(text) = payload.downcast_ref::<String>() {
            (PayloadKind::String, Some(text.as_str()))
        } else {
            (PayloadKind::Other, None)
        };
        Self {
            location,
            thread,
            payload_kind,
            payload_len: message.map(str::len),
            payload_digest: message.map(digest_prefix),
        }
    }

    /// One line for the rolling log.
    pub fn log_line(&self) -> String {
        format!(
            "panic at {} on thread {}: {}",
            self.location.as_deref().unwrap_or("an unknown location"),
            self.thread.as_deref().unwrap_or("<unnamed>"),
            self.payload_facts(),
        )
    }

    fn payload_facts(&self) -> String {
        match (self.payload_len, &self.payload_digest) {
            (Some(len), Some(digest)) => format!(
                "{} message, {len} bytes, sha256 {digest}",
                self.payload_kind.label()
            ),
            _ => format!("{} payload", self.payload_kind.label()),
        }
    }
}

/// A function's address at run time, named so the report's frame addresses
/// can be mapped back to the symbol files of the build that wrote it: the
/// difference between this address and the function's address in the symbol
/// file is the slide the loader applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddressAnchor {
    /// The function's path, as the symbol file names it.
    pub symbol: &'static str,
    /// Where the function sits in this process.
    pub address: usize,
}

/// Renders the crash file.
///
/// `backtrace` is the captured backtrace in its full form, frame addresses
/// included; it names code locations, not data.
pub fn format_crash_report(
    summary: &PanicSummary,
    timestamp: &str,
    anchor: AddressAnchor,
    backtrace: &str,
) -> String {
    let mut out = String::from("Writ panic report\n\n");
    out.push_str(&format!("Time: {timestamp}\n"));
    out.push_str(&format!(
        "Thread: {}\n",
        summary.thread.as_deref().unwrap_or("<unnamed>")
    ));
    out.push_str(&format!(
        "Location: {}\n",
        summary.location.as_deref().unwrap_or("unknown")
    ));
    out.push_str(&format!("Payload: {}\n", summary.payload_facts()));
    out.push_str(&format!(
        "Anchor: {} at {:#x}\n\n",
        anchor.symbol, anchor.address
    ));
    out.push_str("Backtrace:\n");
    out.push_str(backtrace);
    if !backtrace.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Crash files one process writes at most.
pub const MAX_CRASH_FILES: usize = 8;

/// Which panics of a process get a crash file.
///
/// The first panic at each location with each message digest gets one, up to
/// [`MAX_CRASH_FILES`]. A contained panic that repeats on every render writes
/// one file, and a different panic after it, which may be the one that ends
/// the process, still writes its own.
#[derive(Debug, Default)]
pub struct CrashFiles {
    written: Vec<(Option<String>, Option<String>)>,
}

impl CrashFiles {
    /// The sequence number of the file this panic gets, counted from 1, or
    /// `None` when it gets none.
    pub fn admit(&mut self, summary: &PanicSummary) -> Option<usize> {
        let key = (summary.location.clone(), summary.payload_digest.clone());
        if self.written.len() >= MAX_CRASH_FILES || self.written.contains(&key) {
            return None;
        }
        self.written.push(key);
        Some(self.written.len())
    }
}

/// The file a crash report is written to: `writ-crash-<timestamp>-<n>.log`,
/// where `n` is the report's sequence number in its process, so two panics
/// within one second keep both files.
pub fn crash_report_file_name(timestamp: &str, sequence: usize) -> String {
    format!("writ-crash-{timestamp}-{sequence}.log")
}

fn digest_prefix(message: &str) -> String {
    let mut digest = sha256_hex(message.as_bytes());
    digest.truncate(DIGEST_PREFIX_LEN);
    digest
}
