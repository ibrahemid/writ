//! How loud a frame of 16-bit samples is.

/// The quietest level a frame reports, in dBFS. Digital silence reads as this.
pub const FLOOR_DBFS: f32 = -100.0;

/// The level shown as an empty meter, in dBFS.
const METER_MIN_DBFS: f32 = -60.0;

/// RMS level of `samples` in dBFS, at least [`FLOOR_DBFS`].
pub fn rms_dbfs(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return FLOOR_DBFS;
    }
    let sum: f64 = samples
        .iter()
        .map(|s| {
            let v = f64::from(*s) / f64::from(i16::MAX);
            v * v
        })
        .sum();
    let rms = (sum / samples.len() as f64).sqrt();
    if rms <= 0.0 {
        return FLOOR_DBFS;
    }
    let db = (20.0 * rms.log10()) as f32;
    db.max(FLOOR_DBFS)
}

/// The meter value for a frame: its RMS level from -60 dBFS to 0 dBFS mapped
/// onto 0.0 to 1.0.
pub fn frame_level(samples: &[i16]) -> f32 {
    let db = rms_dbfs(samples);
    ((db - METER_MIN_DBFS) / -METER_MIN_DBFS).clamp(0.0, 1.0)
}

/// Whether every sample is zero, which is what a microphone the system will
/// not let the app hear delivers.
pub fn is_digital_silence(samples: &[i16]) -> bool {
    samples.iter().all(|s| *s == 0)
}
