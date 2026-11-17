//! How long a session runs without anyone attending to it.
//!
//! A session that hears no speech for a minute stops by itself, and an API
//! session stops capturing once five minutes of audio wait for the
//! transcription service. Either way what was heard is still inserted.

use super::samples_for_ms;
use super::speech::SpeechDetector;

/// A session that hears no speech this long stops (60 s).
pub const QUIET_STOP_MS: u32 = 60_000;
/// Capture stops once this much audio waits for the transcription service
/// (5 min).
pub const QUEUE_LIMIT_MS: u32 = 300_000;
/// The frame the quiet watch listens in.
const QUIET_FRAME_MS: u32 = 30;

/// The limits a session runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLimits {
    /// No speech for this long stops the session.
    pub quiet_stop_ms: u32,
    /// This much audio waiting for the service stops capture.
    pub queue_ms: u32,
}

impl Default for SessionLimits {
    fn default() -> Self {
        Self {
            quiet_stop_ms: QUIET_STOP_MS,
            queue_ms: QUEUE_LIMIT_MS,
        }
    }
}

impl SessionLimits {
    /// Whether `queued` 16 kHz samples waiting for the service fill the queue.
    pub fn is_queue_full(&self, queued: usize) -> bool {
        queued >= samples_for_ms(self.queue_ms)
    }

    /// A watch for this session's quiet stop.
    pub fn quiet_watch(&self) -> QuietWatch {
        QuietWatch::new(self.quiet_stop_ms)
    }
}

/// Counts the audio since the last speech.
#[derive(Debug, Clone)]
pub struct QuietWatch {
    detector: SpeechDetector,
    frame_len: usize,
    limit: usize,
    quiet: usize,
    pending: Vec<i16>,
}

impl QuietWatch {
    /// A watch that fires after `limit_ms` of audio with no speech in it.
    pub fn new(limit_ms: u32) -> Self {
        Self {
            detector: SpeechDetector::new(QUIET_FRAME_MS),
            frame_len: samples_for_ms(QUIET_FRAME_MS),
            limit: samples_for_ms(limit_ms),
            quiet: 0,
            pending: Vec::new(),
        }
    }

    /// Feeds 16 kHz samples; true once the limit has passed with no speech.
    pub fn push(&mut self, samples: &[i16]) -> bool {
        self.pending.extend_from_slice(samples);
        let whole = self.pending.len() - self.pending.len() % self.frame_len;
        for frame in self.pending[..whole].chunks(self.frame_len) {
            let (_, speech) = self.detector.classify(frame);
            self.quiet = if speech { 0 } else { self.quiet + frame.len() };
        }
        self.pending.drain(..whole);
        self.quiet >= self.limit
    }
}
