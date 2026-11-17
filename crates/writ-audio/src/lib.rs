//! Microphone capture for the Writ editor, with no Tauri in it.
//!
//! [`Recorder`] opens the system's default input device and delivers
//! [`AudioFrame`]s of 16 kHz mono samples. [`Converter`] does the downmix,
//! resample and sample conversion, and [`CaptureError`] names what went wrong
//! in terms the dictation session maps to its own errors.

mod capture;
mod convert;
mod error;

pub use capture::{AudioFrame, Framer, Recorder, FRAME_MS, FRAME_SAMPLES};
pub use convert::{downmix, Converter};
pub use error::{is_fatal_stream_error, CaptureError};
