//! Telling speech from the noise around it, one frame at a time.
//!
//! Each frame's RMS level is compared with a noise floor that follows the
//! quietest recent frames: it drops at once to a quieter frame and rises
//! slowly otherwise. A frame is speech when it is clearly above that floor
//! and above an absolute minimum, so steady fan or room noise never counts.
//! A frame of digital silence (all zeros, as a device delivers while it
//! starts) or near it (zeros with a bit of dither) is never speech and leaves
//! the floor where it is.

use super::level::rms_dbfs;

/// A frame is speech only above this level.
const ABS_MIN_DBFS: f32 = -50.0;
/// A frame is speech only this far above the noise floor.
const MARGIN_DB: f32 = 10.0;
/// The first frame sets the floor, at most this high, so a session that
/// starts mid-word still finds the word.
const FLOOR_START_CAP_DBFS: f32 = -40.0;
/// How fast the floor rises toward louder frames.
const FLOOR_RISE_DB_PER_S: f32 = 3.3;
/// A frame below this level is digital silence, or zeros with dither of a
/// bit or two; no microphone's own noise is this quiet.
const NEAR_SILENCE_DBFS: f32 = -85.0;

/// Classifies frames as speech against a noise floor it keeps.
#[derive(Debug, Clone)]
pub struct SpeechDetector {
    floor_db: Option<f32>,
    floor_rise_db: f32,
}

impl SpeechDetector {
    /// A detector for frames `frame_ms` long; the floor rises at the same rate
    /// per second whatever the frame length.
    pub fn new(frame_ms: u32) -> Self {
        Self {
            floor_db: None,
            floor_rise_db: FLOOR_RISE_DB_PER_S * frame_ms as f32 / 1000.0,
        }
    }

    /// The frame's level in dBFS and whether it is speech.
    pub fn classify(&mut self, samples: &[i16]) -> (f32, bool) {
        let db = rms_dbfs(samples);
        if db < NEAR_SILENCE_DBFS {
            return (db, false);
        }
        let floor = self.floor_db.unwrap_or(db.min(FLOOR_START_CAP_DBFS));
        let speech = db > ABS_MIN_DBFS.max(floor + MARGIN_DB);
        let next_floor = if db < floor {
            db
        } else {
            (floor + self.floor_rise_db).min(db)
        };
        self.floor_db = Some(next_floor);
        (db, speech)
    }
}
