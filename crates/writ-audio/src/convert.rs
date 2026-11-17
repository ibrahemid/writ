//! Device samples to 16 kHz mono `i16`: downmix, resample, convert.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use writ_core::dictation::SAMPLE_RATE;

use crate::error::CaptureError;

/// The resampler works on input chunks this long.
const CHUNK_MS: u32 = 20;

/// The mean of each frame's channels. A trailing partial frame is dropped.
pub fn downmix(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let channels = usize::from(channels.max(1));
    if channels == 1 {
        return interleaved.to_vec();
    }
    let scale = 1.0 / channels as f32;
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() * scale)
        .collect()
}

fn to_i16(sample: f32) -> i16 {
    if sample.is_nan() {
        return 0;
    }
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

/// Converts one device's interleaved `f32` samples to 16 kHz mono `i16`.
pub struct Converter {
    input_rate: u32,
    channels: u16,
    resampler: Option<Fft<f32>>,
    pending: Vec<f32>,
    output: Vec<f32>,
    delay_left: usize,
    input_total: u64,
    output_total: u64,
}

impl std::fmt::Debug for Converter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Converter")
            .field("input_rate", &self.input_rate)
            .field("channels", &self.channels)
            .field("resampling", &self.resampler.is_some())
            .finish()
    }
}

impl Converter {
    /// A converter for audio at `input_rate` Hz with `channels` interleaved
    /// channels.
    pub fn new(input_rate: u32, channels: u16) -> Result<Self, CaptureError> {
        if input_rate == 0 || channels == 0 {
            return Err(CaptureError::Unsupported(format!(
                "the input reports {input_rate} Hz with {channels} channels"
            )));
        }
        let resampler = if input_rate == SAMPLE_RATE {
            None
        } else {
            let chunk = (input_rate * CHUNK_MS / 1000).max(1) as usize;
            let resampler = Fft::<f32>::new(
                input_rate as usize,
                SAMPLE_RATE as usize,
                chunk,
                1,
                FixedSync::Input,
            )
            .map_err(|e| CaptureError::Unsupported(e.to_string()))?;
            Some(resampler)
        };
        let (output, delay_left) = match &resampler {
            Some(r) => (vec![0.0; r.output_frames_max()], r.output_delay()),
            None => (Vec::new(), 0),
        };
        Ok(Self {
            input_rate,
            channels,
            resampler,
            pending: Vec::new(),
            output,
            delay_left,
            input_total: 0,
            output_total: 0,
        })
    }

    /// Converts `interleaved` and returns the 16 kHz samples ready so far.
    pub fn push(&mut self, interleaved: &[f32]) -> Vec<i16> {
        let mono = downmix(interleaved, self.channels);
        self.input_total += mono.len() as u64;
        if self.resampler.is_none() {
            self.output_total += mono.len() as u64;
            return mono.into_iter().map(to_i16).collect();
        }
        self.pending.extend_from_slice(&mono);
        let mut out = Vec::new();
        while let Some(needed) = self.resampler.as_ref().map(|r| r.input_frames_next()) {
            if self.pending.len() < needed {
                break;
            }
            if !self.run_chunk(needed, needed, &mut out) {
                break;
            }
        }
        out
    }

    /// Ends the stream and returns the samples still held, so the whole
    /// input comes out at 16 kHz.
    pub fn finish(&mut self) -> Vec<i16> {
        let mut out = Vec::new();
        if self.resampler.is_none() {
            return out;
        }
        let expected = (self.input_total * u64::from(SAMPLE_RATE) + u64::from(self.input_rate) / 2)
            / u64::from(self.input_rate);
        // Each pass yields at least one chunk of output; the bound only guards
        // against a resampler that stops producing.
        for _ in 0..64 {
            if self.output_total >= expected {
                break;
            }
            let Some(needed) = self.resampler.as_ref().map(|r| r.input_frames_next()) else {
                break;
            };
            let available = self.pending.len().min(needed);
            if !self.run_chunk(needed, available, &mut out) {
                break;
            }
        }
        let surplus = self.output_total.saturating_sub(expected) as usize;
        out.truncate(out.len().saturating_sub(surplus));
        self.output_total = self.output_total.min(expected);
        out
    }

    /// Resamples `available` pending frames as one chunk of `needed`, padding
    /// with silence, and appends the result past the resampler's delay.
    fn run_chunk(&mut self, needed: usize, available: usize, out: &mut Vec<i16>) -> bool {
        let Some(resampler) = self.resampler.as_mut() else {
            return false;
        };
        let mut chunk = vec![0.0f32; needed];
        chunk[..available].copy_from_slice(&self.pending[..available]);
        let Ok(input) = InterleavedSlice::new(&chunk[..], 1, needed) else {
            return false;
        };
        let frames_out = self.output.len();
        let Ok(mut output) = InterleavedSlice::new_mut(&mut self.output[..], 1, frames_out) else {
            return false;
        };
        let Ok((_, written)) = resampler.process_into_buffer(&input, &mut output, None) else {
            return false;
        };
        self.pending.drain(..available);
        let skip = self.delay_left.min(written);
        self.delay_left -= skip;
        let produced = &self.output[skip..written];
        self.output_total += produced.len() as u64;
        out.extend(produced.iter().copied().map(to_i16));
        true
    }
}
