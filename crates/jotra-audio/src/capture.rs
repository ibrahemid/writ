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
use jotra_core::dictation::{frame_level, samples_for_ms};

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
            .name("jotra-audio-capture".to_string())
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
    let opened = open_stream(stream_control, frames, &failure);
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
        record_failure(&failure, error);
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
    failure: &Arc<Mutex<Option<CaptureError>>>,
) -> Result<Opened, CaptureError> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or(CaptureError::NoInputDevice)?;
    let supported = device.default_input_config()?;
    let format = supported.sample_format();
    check_sample_format(format)?;
    let config: StreamConfig = supported.config();
    let converter = Converter::new(config.sample_rate, 1)?;
    tracing::info!(
        rate = config.sample_rate,
        channels = config.channels,
        format = %format,
        "audio capture: opening the default input"
    );

    let dropped = Arc::new(AtomicU64::new(0));
    let sink = StreamSink {
        dropped: Arc::clone(&dropped),
        control: stream_control,
        failure: Arc::clone(failure),
    };
    let (stream, worker) = spawn_worker_then(converter, frames, |chunks| match format {
        SampleFormat::I8 => build::<i8>(&device, &config, chunks, sink),
        SampleFormat::I16 => build::<i16>(&device, &config, chunks, sink),
        SampleFormat::I24 => build::<cpal::I24>(&device, &config, chunks, sink),
        SampleFormat::I32 => build::<i32>(&device, &config, chunks, sink),
        SampleFormat::I64 => build::<i64>(&device, &config, chunks, sink),
        SampleFormat::U8 => build::<u8>(&device, &config, chunks, sink),
        SampleFormat::U16 => build::<u16>(&device, &config, chunks, sink),
        SampleFormat::U24 => build::<cpal::U24>(&device, &config, chunks, sink),
        SampleFormat::U32 => build::<u32>(&device, &config, chunks, sink),
        SampleFormat::U64 => build::<u64>(&device, &config, chunks, sink),
        SampleFormat::F32 => build::<f32>(&device, &config, chunks, sink),
        SampleFormat::F64 => build::<f64>(&device, &config, chunks, sink),
        other => Err(unsupported_format(other)),
    })?;
    Ok((stream, worker, dropped))
}

/// Fails for a sample format this crate cannot read, before any thread starts.
fn check_sample_format(format: SampleFormat) -> Result<(), CaptureError> {
    match format {
        SampleFormat::I8
        | SampleFormat::I16
        | SampleFormat::I24
        | SampleFormat::I32
        | SampleFormat::I64
        | SampleFormat::U8
        | SampleFormat::U16
        | SampleFormat::U24
        | SampleFormat::U32
        | SampleFormat::U64
        | SampleFormat::F32
        | SampleFormat::F64 => Ok(()),
        other => Err(unsupported_format(other)),
    }
}

fn unsupported_format(format: SampleFormat) -> CaptureError {
    CaptureError::Unsupported(format!("the input uses the {format} sample format"))
}

/// Starts the converter thread, then hands `build` the only sender of its
/// queue. Whatever `build` does not keep is dropped when it returns, so a
/// failed build always ends the thread before it is joined.
fn spawn_worker_then<S>(
    converter: Converter,
    frames: tokio::sync::mpsc::Sender<AudioFrame>,
    build: impl FnOnce(SyncSender<Vec<f32>>) -> Result<S, CaptureError>,
) -> Result<(S, JoinHandle<()>), CaptureError> {
    let (chunk_tx, chunk_rx) = mpsc::sync_channel::<Vec<f32>>(QUEUE_CHUNKS);
    let worker = std::thread::Builder::new()
        .name("jotra-audio-convert".to_string())
        .spawn(move || convert_chunks(chunk_rx, converter, frames))
        .map_err(|e| CaptureError::Backend(e.to_string()))?;
    match build(chunk_tx) {
        Ok(stream) => Ok((stream, worker)),
        Err(error) => {
            let _ = worker.join();
            Err(error)
        }
    }
}

/// What the device callbacks report to.
struct StreamSink {
    dropped: Arc<AtomicU64>,
    control: Sender<Control>,
    failure: Arc<Mutex<Option<CaptureError>>>,
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    chunks: SyncSender<Vec<f32>>,
    sink: StreamSink,
) -> Result<cpal::Stream, CaptureError>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = usize::from(config.channels.max(1));
    let scale = 1.0 / channels as f32;
    let StreamSink {
        dropped,
        control,
        failure,
    } = sink;
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
    let on_error = move |error: cpal::Error| report_stream_error(&error, &failure, &control);
    device
        .build_input_stream::<T, _, _>(*config, on_data, on_error, None)
        .map_err(CaptureError::from)
}

/// Stores a fatal stream error before anything else, since a backend can
/// drop the data callback (closing the frame channel) right after it.
fn report_stream_error(
    error: &cpal::Error,
    failure: &Mutex<Option<CaptureError>>,
    control: &Sender<Control>,
) {
    if !is_fatal_stream_error(error.kind()) {
        tracing::debug!(kind = ?error.kind(), "audio capture: stream notice");
        return;
    }
    let error = CaptureError::from_stream(error);
    if record_failure(failure, error.clone()) {
        let _ = control.send(Control::Failed(error));
    }
}

/// Keeps the first failure; returns whether `error` was the first.
fn record_failure(failure: &Mutex<Option<CaptureError>>, error: CaptureError) -> bool {
    let mut held = failure.lock().unwrap_or_else(PoisonError::into_inner);
    if held.is_some() {
        return false;
    }
    *held = Some(error);
    true
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use cpal::{Error, ErrorKind};

    use super::*;

    #[test]
    fn a_format_this_crate_cannot_read_is_refused_before_any_thread_starts() {
        for format in [
            SampleFormat::DsdU8,
            SampleFormat::DsdU16,
            SampleFormat::DsdU32,
        ] {
            assert!(
                matches!(
                    check_sample_format(format),
                    Err(CaptureError::Unsupported(_))
                ),
                "{format}"
            );
        }
        for format in [
            SampleFormat::I64,
            SampleFormat::U24,
            SampleFormat::U64,
            SampleFormat::I16,
            SampleFormat::F32,
        ] {
            assert_eq!(check_sample_format(format), Ok(()), "{format}");
        }
    }

    #[test]
    fn a_failed_build_ends_the_converter_thread() {
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (frames, _frame_rx) = tokio::sync::mpsc::channel(FRAME_QUEUE);
            let converter = Converter::new(48_000, 1).expect("converter");
            let result = spawn_worker_then(converter, frames, |_chunks| {
                Err::<(), _>(unsupported_format(SampleFormat::DsdU8))
            });
            let _ = done_tx.send(result.map(|_| ()));
        });
        let result = done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the failed build returns instead of waiting on the converter");
        assert!(matches!(result, Err(CaptureError::Unsupported(_))));
    }

    #[test]
    fn a_built_stream_feeds_the_converter() {
        let (frames, mut frame_rx) = tokio::sync::mpsc::channel(FRAME_QUEUE);
        let converter = Converter::new(48_000, 1).expect("converter");
        let (chunks, worker) =
            spawn_worker_then(converter, frames, Ok::<_, CaptureError>).expect("starts");
        chunks.send(vec![0.25; 48_000]).expect("queued");
        drop(chunks);
        worker.join().expect("the converter ends");
        let mut samples = 0;
        while let Ok(frame) = frame_rx.try_recv() {
            samples += frame.samples.len();
        }
        assert!((15_000..=16_500).contains(&samples), "{samples} samples");
    }

    #[test]
    fn a_fatal_stream_error_is_stored_when_it_is_reported() {
        let failure = Mutex::new(None);
        let (control, control_rx) = mpsc::channel();
        report_stream_error(
            &Error::new(ErrorKind::DeviceNotAvailable),
            &failure,
            &control,
        );
        assert!(matches!(
            *failure.lock().unwrap(),
            Some(CaptureError::DeviceLost(_))
        ));
        assert!(matches!(
            control_rx.try_recv(),
            Ok(Control::Failed(CaptureError::DeviceLost(_)))
        ));

        report_stream_error(
            &Error::with_message(ErrorKind::BackendError, "later"),
            &failure,
            &control,
        );
        assert!(matches!(
            *failure.lock().unwrap(),
            Some(CaptureError::DeviceLost(_))
        ));
        assert!(control_rx.try_recv().is_err());
    }

    #[test]
    fn a_stream_notice_is_not_a_failure() {
        let failure = Mutex::new(None);
        let (control, control_rx) = mpsc::channel();
        for kind in [ErrorKind::Xrun, ErrorKind::DeviceChanged] {
            report_stream_error(&Error::new(kind), &failure, &control);
        }
        assert_eq!(*failure.lock().unwrap(), None);
        assert!(control_rx.try_recv().is_err());
    }

    #[test]
    fn the_first_failure_is_kept() {
        let failure = Mutex::new(None);
        assert!(record_failure(&failure, CaptureError::PermissionDenied));
        assert!(!record_failure(
            &failure,
            CaptureError::Backend("later".to_string())
        ));
        assert_eq!(
            *failure.lock().unwrap(),
            Some(CaptureError::PermissionDenied)
        );
    }
}
