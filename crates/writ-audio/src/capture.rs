//! The default input device, read on its own thread.
//!
//! The device callback downmixes each buffer to mono and hands it to a
//! bounded queue without blocking. A worker thread resamples to 16 kHz, cuts
//! 20 ms frames and sends them on. The cpal stream stays on the thread that
//! built it, since it cannot move between threads on every platform.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample, StreamConfig};
use writ_core::dictation::{frame_level, samples_for_ms};

use crate::convert::Converter;
use crate::error::{is_fatal_stream_error, CaptureError};

/// Length of one [`AudioFrame`].
pub const FRAME_MS: u32 = 20;
/// Samples in one full [`AudioFrame`] at 16 kHz.
pub const FRAME_SAMPLES: usize = samples_for_ms(FRAME_MS);

/// Device buffers held while the worker is busy, about 20 s of audio.
const QUEUE_CHUNKS: usize = 1_000;
/// Frames held for a consumer that has not read them yet.
const FRAME_QUEUE: usize = 1_000;

/// 20 ms of 16 kHz mono audio and its level for a meter.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFrame {
    /// 16 kHz mono samples; the last frame of a recording can be shorter.
    pub samples: Vec<i16>,
    /// Loudness from 0 (silence) to 1 (full scale).
    pub level: f32,
}

impl AudioFrame {
    /// A frame holding `samples`, with its level computed.
    pub fn from_samples(samples: Vec<i16>) -> Self {
        let level = frame_level(&samples);
        Self { samples, level }
    }
}

/// Cuts a stream of 16 kHz samples into [`AudioFrame`]s.
#[derive(Debug, Default)]
pub struct Framer {
    pending: Vec<i16>,
}

impl Framer {
    /// Adds `samples` and returns the full frames they complete.
    pub fn push(&mut self, samples: &[i16]) -> Vec<AudioFrame> {
        self.pending.extend_from_slice(samples);
        let full = self.pending.len() / FRAME_SAMPLES * FRAME_SAMPLES;
        let frames = self
            .pending
            .drain(..full)
            .collect::<Vec<_>>()
            .chunks(FRAME_SAMPLES)
            .map(|chunk| AudioFrame::from_samples(chunk.to_vec()))
            .collect();
        frames
    }

    /// The samples left over, as one short frame.
    pub fn finish(self) -> Option<AudioFrame> {
        (!self.pending.is_empty()).then(|| AudioFrame::from_samples(self.pending))
    }
}

enum Control {
    Stop,
    Failed(CaptureError),
}

/// A recording from the system's default input device. Dropping it stops
/// the recording; the frame channel then closes after the last frame.
pub struct Recorder {
    control: Sender<Control>,
    thread: Option<JoinHandle<()>>,
    failure: Arc<Mutex<Option<CaptureError>>>,
}

impl std::fmt::Debug for Recorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recorder")
            .field("running", &self.thread.is_some())
            .finish()
    }
}

impl Recorder {
    /// Opens the default input device and starts recording. Blocks until the
    /// stream runs or fails, so async callers run it off the async runtime.
    pub fn start() -> Result<(Recorder, tokio::sync::mpsc::Receiver<AudioFrame>), CaptureError> {
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), CaptureError>>(1);
        let (control_tx, control_rx) = mpsc::channel::<Control>();
        let (frame_tx, frame_rx) = tokio::sync::mpsc::channel(FRAME_QUEUE);
        let failure = Arc::new(Mutex::new(None));

        let stream_control = control_tx.clone();
        let thread_failure = Arc::clone(&failure);
        let thread = std::thread::Builder::new()
            .name("writ-audio-capture".to_string())
            .spawn(move || {
                run_capture(
                    ready_tx,
                    control_rx,
                    stream_control,
                    frame_tx,
                    thread_failure,
                )
            })
            .map_err(|e| CaptureError::Backend(e.to_string()))?;

        let ready = ready_rx.recv();
        match ready {
            Ok(Ok(())) => Ok((
                Recorder {
                    control: control_tx,
                    thread: Some(thread),
                    failure,
                },
                frame_rx,
            )),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(CaptureError::Backend(
                    "the capture thread ended before the stream started".to_string(),
                ))
            }
        }
    }

    /// Stops the recording. The frames captured so far are still delivered.
    pub fn stop(self) {
        drop(self);
    }

    /// The failure that ended the recording early, once the frame channel
    /// has closed without a stop.
    pub fn failure(&self) -> Option<CaptureError> {
        self.failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.control.send(Control::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_capture(
    ready: SyncSender<Result<(), CaptureError>>,
    control: Receiver<Control>,
    stream_control: Sender<Control>,
    frames: tokio::sync::mpsc::Sender<AudioFrame>,
    failure: Arc<Mutex<Option<CaptureError>>>,
) {
    let opened = open_stream(stream_control, frames);
    let (stream, worker, dropped) = match opened {
        Ok(parts) => parts,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    if let Err(error) = stream.play() {
        drop(stream);
        let _ = worker.join();
        let _ = ready.send(Err(error.into()));
        return;
    }
    let _ = ready.send(Ok(()));

    if let Ok(Control::Failed(error)) = control.recv() {
        tracing::warn!(error = %error, "audio capture: the stream failed");
        *failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
    }
    drop(stream);
    let _ = worker.join();
    let dropped = dropped.load(Ordering::Relaxed);
    if dropped > 0 {
        tracing::warn!(
            dropped,
            "audio capture: buffers dropped while the worker was busy"
        );
    }
}

type Opened = (cpal::Stream, JoinHandle<()>, Arc<AtomicU64>);

fn open_stream(
    stream_control: Sender<Control>,
    frames: tokio::sync::mpsc::Sender<AudioFrame>,
) -> Result<Opened, CaptureError> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or(CaptureError::NoInputDevice)?;
    let supported = device.default_input_config()?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.config();
    let converter = Converter::new(config.sample_rate, 1)?;
    tracing::info!(
        rate = config.sample_rate,
        channels = config.channels,
        format = %format,
        "audio capture: opening the default input"
    );

    let (chunk_tx, chunk_rx) = mpsc::sync_channel::<Vec<f32>>(QUEUE_CHUNKS);
    let worker = std::thread::Builder::new()
        .name("writ-audio-convert".to_string())
        .spawn(move || convert_chunks(chunk_rx, converter, frames))
        .map_err(|e| CaptureError::Backend(e.to_string()))?;
    let dropped = Arc::new(AtomicU64::new(0));

    let built = match format {
        SampleFormat::I8 => build::<i8>(&device, &config, chunk_tx, &dropped, stream_control),
        SampleFormat::I16 => build::<i16>(&device, &config, chunk_tx, &dropped, stream_control),
        SampleFormat::I24 => {
            build::<cpal::I24>(&device, &config, chunk_tx, &dropped, stream_control)
        }
        SampleFormat::I32 => build::<i32>(&device, &config, chunk_tx, &dropped, stream_control),
        SampleFormat::U8 => build::<u8>(&device, &config, chunk_tx, &dropped, stream_control),
        SampleFormat::U16 => build::<u16>(&device, &config, chunk_tx, &dropped, stream_control),
        SampleFormat::U32 => build::<u32>(&device, &config, chunk_tx, &dropped, stream_control),
        SampleFormat::F32 => build::<f32>(&device, &config, chunk_tx, &dropped, stream_control),
        SampleFormat::F64 => build::<f64>(&device, &config, chunk_tx, &dropped, stream_control),
        other => Err(CaptureError::Unsupported(format!(
            "the input uses the {other} sample format"
        ))),
    };
    match built {
        Ok(stream) => Ok((stream, worker, dropped)),
        Err(error) => {
            let _ = worker.join();
            Err(error)
        }
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    chunks: SyncSender<Vec<f32>>,
    dropped: &Arc<AtomicU64>,
    stream_control: Sender<Control>,
) -> Result<cpal::Stream, CaptureError>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = usize::from(config.channels.max(1));
    let scale = 1.0 / channels as f32;
    let dropped = Arc::clone(dropped);
    let on_data = move |data: &[T], _: &cpal::InputCallbackInfo| {
        let mono: Vec<f32> = data
            .chunks_exact(channels)
            .map(|frame| frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() * scale)
            .collect();
        match chunks.try_send(mono) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    };
    let on_error = move |error: cpal::Error| {
        if is_fatal_stream_error(error.kind()) {
            let _ = stream_control.send(Control::Failed(CaptureError::from_stream(&error)));
        } else {
            tracing::debug!(kind = ?error.kind(), "audio capture: stream notice");
        }
    };
    device
        .build_input_stream::<T, _, _>(*config, on_data, on_error, None)
        .map_err(CaptureError::from)
}

fn convert_chunks(
    chunks: Receiver<Vec<f32>>,
    mut converter: Converter,
    frames: tokio::sync::mpsc::Sender<AudioFrame>,
) {
    let mut framer = Framer::default();
    let mut delivered: u64 = 0;
    for chunk in chunks.iter() {
        for frame in framer.push(&converter.push(&chunk)) {
            if frames.blocking_send(frame).is_err() {
                return;
            }
            delivered += 1;
        }
    }
    let mut last = framer.push(&converter.finish());
    last.extend(framer.finish());
    for frame in last {
        if frames.blocking_send(frame).is_err() {
            return;
        }
        delivered += 1;
    }
    tracing::debug!(
        ms = delivered * u64::from(FRAME_MS),
        "audio capture: recording ended"
    );
}
