//! Dictation: speech in, text at the cursor.
//!
//! The policy half of dictation lives here and nowhere else: where a stream
//! of 16 kHz samples is cut into segments ([`segmenter`]), how loud a frame is
//! ([`level`]), the WAV a segment is sent as ([`wav`]), the languages and the
//! on-device locale ([`language`]), the transcription presets ([`presets`]),
//! which key a request carries ([`key`]), how a transcript is tidied
//! ([`text`]), what counts as speech ([`speech`]), how long a session runs
//! unattended ([`limits`]), and what a session reports ([`event`], [`error`],
//! [`on_device`]). Capture is `writ-audio`, HTTP is `writ-ai`, and the session
//! itself runs in the Tauri crate.

pub mod error;
pub mod event;
pub mod key;
pub mod language;
pub mod level;
pub mod limits;
pub mod on_device;
pub mod platform;
pub mod presets;
pub mod segmenter;
pub mod speech;
pub mod text;
pub mod wav;

pub use crate::config::dictation::{
    effective_engine, DictationConfig, DictationEngine, DictationProvider,
};
pub use error::{DictationAction, DictationError, DictationErrorCode, DictationFailure};
pub use event::DictationEvent;
pub use key::{key_account, KeyAccount};
pub use language::{language, on_device_locale, Language, LANGUAGES};
pub use level::{frame_level, is_digital_silence};
pub use limits::{QuietWatch, SessionLimits, QUEUE_LIMIT_MS, QUIET_STOP_MS};
pub use on_device::error_from_bridge;
pub use presets::{DictationPreset, PRESETS};
pub use segmenter::{Segment, Segmenter, SegmenterParams};
pub use speech::SpeechDetector;
pub use text::{clean_transcript, prompt_tail, CLEAN_UP_INSTRUCTION};
pub use wav::encode_wav_16k_mono;

/// The sample rate every dictation stage works at.
pub const SAMPLE_RATE: u32 = 16_000;

/// Samples in `ms` milliseconds at [`SAMPLE_RATE`].
pub const fn samples_for_ms(ms: u32) -> usize {
    (SAMPLE_RATE as usize / 1000) * ms as usize
}
