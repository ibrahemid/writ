//! What a running session tells the frontend.

use serde::Serialize;

use super::error::DictationFailure;

/// One message on a session's event channel. Exactly one of `Done`,
/// `Cancelled` or `Error` ends every session and nothing follows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DictationEvent {
    /// The on-device model is downloading; `progress` runs 0.0 to 1.0 when
    /// known.
    Preparing {
        /// Download progress, when known.
        progress: Option<f32>,
    },
    /// Capture is running.
    Recording,
    /// The input level, 0.0 to 1.0, at most 20 times a second.
    Level {
        /// The meter value.
        level: f32,
    },
    /// On-device text not yet final; replaces the previous partial.
    Partial {
        /// The volatile text.
        text: String,
    },
    /// Segments closed and not yet answered.
    Transcribing {
        /// How many.
        pending: u32,
    },
    /// Capture has ended and the session is inserting what it heard. Sent
    /// once, on every stop.
    Stopping,
    /// Final text to insert now, in `seq` order.
    Segment {
        /// The segment's order in the session.
        seq: u32,
        /// The text.
        text: String,
    },
    /// Something that did not stop the session.
    Notice {
        /// The sentence shown.
        message: String,
    },
    /// The session ended after inserting everything it heard.
    Done,
    /// The session was canceled; text not yet inserted was discarded.
    Cancelled,
    /// The session failed.
    Error {
        /// What went wrong.
        failure: DictationFailure,
    },
}

impl DictationEvent {
    /// Whether this event ends the session.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done | Self::Cancelled | Self::Error { .. })
    }

    /// The notice shown when clean-up fails and the raw text goes in.
    pub fn clean_up_skipped(reason: &str) -> Self {
        Self::Notice {
            message: format!("Inserted without clean-up: {reason}"),
        }
    }

    /// The notice shown when a session stops because it heard no speech for
    /// a minute.
    pub fn stopped_after_silence() -> Self {
        Self::Notice {
            message: "Dictation stopped after a minute of silence.".to_string(),
        }
    }

    /// The notice shown when capture stops because too much audio waits for
    /// the transcription service.
    pub fn stopped_service_behind() -> Self {
        Self::Notice {
            message: "Dictation stopped because the transcription service could not keep up."
                .to_string(),
        }
    }
}
