//! One dictation session, from its first frame to the event that ends it, on
//! either engine.
//!
//! A driver owns the capture, the requests or the recognition, and the event
//! channel. Its loop waits on the cancel and stop tokens, the next frame, the
//! answer in flight and, on device, the bridge's next event, and reports what
//! happened as [`DictationEvent`]s. Exactly one of `Done`, `Cancelled` or
//! `Error` ends every session.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use writ_ai::connection::{build_client_for, OnDeviceRequest, PreparedRequest, StreamEvent};
use writ_ai::transcription::{
    prepare_clean_up, run_clean_up, CleanUpRequest, PreparedTranscription,
};
use writ_audio::{AudioFrame, CaptureError, Recorder};
use writ_core::config::AiConfig;
use writ_core::dictation::{
    clean_transcript, encode_wav_16k_mono, error_from_bridge, is_digital_silence, prompt_tail,
    samples_for_ms, DictationError, DictationEvent, Segment, Segmenter, SessionLimits,
};

use super::bridge::{BridgeEvent, Recognition};
use crate::poison::recover_poison;

/// Level events go out at most once per this many samples (50 ms).
const LEVEL_EVERY: usize = samples_for_ms(50);
/// Unbroken digital silence this long from the start means the system handed
/// the app a muted microphone (1.5 s).
const SILENCE_LIMIT: usize = samples_for_ms(1_500);
/// The most audio held while the on-device model gets ready (5 s).
const PRE_READY_LIMIT: usize = samples_for_ms(5_000);
/// Partials go out at most this often.
const PARTIAL_EVERY: Duration = Duration::from_millis(100);
/// How much of the text already heard each request carries as its prompt.
const PROMPT_CHARS: usize = 200;

/// The tokens that end a session: `stop` keeps what was heard, `cancel`
/// discards what is not inserted yet.
#[derive(Debug, Clone, Default)]
pub struct Controls {
    /// Ends capture; the session inserts what it heard, then ends.
    pub stop: CancellationToken,
    /// Ends the session at once.
    pub cancel: CancellationToken,
}

/// A future a driver holds across loop turns.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// The running capture. Dropping it stops capture; the frames already taken
/// still arrive, then the frame channel closes.
pub trait Capture: Send {
    /// Why capture ended, when it failed.
    fn failure(&self) -> Option<CaptureError>;
}

impl Capture for Recorder {
    fn failure(&self) -> Option<CaptureError> {
        Recorder::failure(self)
    }
}

/// Turns one segment into text.
pub trait Transcribe: Send + Sync {
    /// The transcript of `wav`, `None` when it has no words or `cancel` fired.
    /// `prompt` is the end of the text already heard.
    fn transcribe(
        &self,
        wav: Vec<u8>,
        prompt: String,
        cancel: CancellationToken,
    ) -> BoxFuture<Result<Option<String>, DictationError>>;
}

/// The clean-up pass over one transcript.
pub trait CleanUp: Send + Sync {
    /// The cleaned text, or the reason the pass failed.
    fn clean_up(
        &self,
        text: String,
        cancel: CancellationToken,
    ) -> BoxFuture<Result<String, String>>;
}

/// A running on-device recognition.
pub trait Recognizer: Send {
    /// Feeds 16 kHz mono samples.
    fn feed(&self, samples: &[i16]);
    /// Ends the input; the remaining text arrives, then `Finished`.
    fn finish(&self);
    /// Stops at once.
    fn stop(&self);
}

impl Recognizer for Recognition {
    fn feed(&self, samples: &[i16]) {
        Recognition::feed(self, samples);
    }

    fn finish(&self) {
        Recognition::finish(self);
    }

    fn stop(&self) {
        Recognition::stop(self);
    }
}

/// Where a session's events go. Nothing is sent after the event that ends
/// the session.
pub struct Emitter {
    send: Box<dyn FnMut(DictationEvent) + Send>,
    ended: bool,
}

impl Emitter {
    /// An emitter that hands each event to `send`.
    pub fn new(send: impl FnMut(DictationEvent) + Send + 'static) -> Self {
        Self {
            send: Box::new(send),
            ended: false,
        }
    }

    /// An emitter that hands each event to `send` and logs the first send of
    /// session `session_id` that fails, not every one after it.
    pub fn logging_failures<E: std::fmt::Display>(
        session_id: u32,
        mut send: impl FnMut(DictationEvent) -> Result<(), E> + Send + 'static,
    ) -> Self {
        let mut failed = false;
        Self::new(move |event| {
            if let Err(error) = send(event) {
                if !std::mem::replace(&mut failed, true) {
                    tracing::warn!(
                        session = session_id,
                        error = %error,
                        "dictation events stopped reaching the window"
                    );
                }
            }
        })
    }

    fn emit(&mut self, event: DictationEvent) {
        if self.ended {
            return;
        }
        self.ended = event.is_terminal();
        (self.send)(event);
    }

    fn fail(&mut self, error: DictationError) {
        self.emit(DictationEvent::Error {
            failure: error.failure(),
        });
    }
}

/// Lets the capture go without waiting on it: dropping a recorder joins its
/// thread, which waits until its last frames are read.
fn release(capture: Option<Box<dyn Capture>>) {
    if let Some(capture) = capture {
        drop(tokio::task::spawn_blocking(move || drop(capture)));
    }
}

/// The loudest frame since the last level event, once enough audio passed.
#[derive(Default)]
struct LevelMeter {
    peak: f32,
    samples: usize,
}

impl LevelMeter {
    fn push(&mut self, frame: &AudioFrame) -> Option<f32> {
        self.peak = self.peak.max(frame.level);
        self.samples += frame.samples.len();
        if self.samples < LEVEL_EVERY {
            return None;
        }
        let level = self.peak;
        self.peak = 0.0;
        self.samples = 0;
        Some(level)
    }
}

/// Watches for a microphone that delivers only zeros from the start.
struct SilenceWatch {
    watching: bool,
    silent: usize,
}

impl SilenceWatch {
    fn new(enabled: bool) -> Self {
        Self {
            watching: enabled,
            silent: 0,
        }
    }

    /// Whether the silence has lasted long enough to call the input muted.
    fn is_muted(&mut self, samples: &[i16]) -> bool {
        if !self.watching {
            return false;
        }
        if !is_digital_silence(samples) {
            self.watching = false;
            return false;
        }
        self.silent += samples.len();
        self.silent >= SILENCE_LIMIT
    }
}

/// A transcript ready to insert, and the reason its clean-up failed.
struct Heard {
    text: String,
    clean_up_failed: Option<String>,
}

/// Runs `text` through the clean-up when there is one; on a failure the text
/// goes in as heard.
async fn clean(
    clean_up: Option<Arc<dyn CleanUp>>,
    text: String,
    cancel: CancellationToken,
) -> Heard {
    let Some(clean_up) = clean_up else {
        return Heard {
            text,
            clean_up_failed: None,
        };
    };
    match clean_up.clean_up(text.clone(), cancel).await {
        Ok(cleaned) => Heard {
            text: cleaned,
            clean_up_failed: None,
        },
        Err(reason) => Heard {
            text,
            clean_up_failed: Some(reason),
        },
    }
}

/// The segments a session inserted.
#[derive(Default)]
struct Inserted {
    count: u32,
    noticed: bool,
}

impl Inserted {
    /// Sends `heard` as the next segment; the first failed clean-up of the
    /// session follows it with a notice.
    fn insert(&mut self, out: &mut Emitter, heard: Heard) {
        out.emit(DictationEvent::Segment {
            seq: self.count,
            text: heard.text,
        });
        self.count += 1;
        if let Some(reason) = heard.clean_up_failed {
            if !self.noticed {
                self.noticed = true;
                out.emit(DictationEvent::clean_up_skipped(&reason));
            }
        }
    }

    /// Ends a session that ran out of input. A session the quiet stop ended
    /// has already said why, so nothing heard is not an error there.
    fn end(&self, out: &mut Emitter, quiet_stopped: bool) {
        if self.count == 0 && !quiet_stopped {
            out.fail(DictationError::NoSpeech);
        } else {
            out.emit(DictationEvent::Done);
        }
    }
}

async fn recv<T>(receiver: &mut Option<mpsc::Receiver<T>>) -> Option<T> {
    match receiver {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

async fn recv_unbounded<T>(receiver: &mut Option<mpsc::UnboundedReceiver<T>>) -> Option<T> {
    match receiver {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

async fn settle<T>(future: &mut Option<BoxFuture<T>>) -> T {
    match future {
        Some(future) => future.as_mut().await,
        None => std::future::pending().await,
    }
}

async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Keeps the end of the text heard so far, which the next request carries.
fn remember(heard: &mut String, text: &str) {
    if !heard.is_empty() {
        heard.push(' ');
    }
    heard.push_str(text);
    let cut = heard.len() - prompt_tail(heard, PROMPT_CHARS).len();
    heard.drain(..cut);
}

/// A session on the transcription API.
pub struct ApiSession {
    /// The session's id, for the log.
    pub session_id: u32,
    /// The capture's frames.
    pub frames: mpsc::Receiver<AudioFrame>,
    /// The capture.
    pub capture: Box<dyn Capture>,
    /// The requests.
    pub transcriber: Arc<dyn Transcribe>,
    /// The clean-up pass, when it is on.
    pub clean_up: Option<Arc<dyn CleanUp>>,
    /// Cuts the audio at pauses.
    pub segmenter: Segmenter,
    /// Whether digital silence from the start ends the session.
    pub silence_check: bool,
    /// When the session stops by itself.
    pub limits: SessionLimits,
}

type Answer = Result<Option<Heard>, DictationError>;

/// Sends segments, one at a time and in order.
struct Requests {
    session_id: u32,
    transcriber: Arc<dyn Transcribe>,
    clean_up: Option<Arc<dyn CleanUp>>,
    cancel: CancellationToken,
}

impl Requests {
    fn send(&self, segment: &Segment, prompt: &str) -> BoxFuture<Answer> {
        let wav = encode_wav_16k_mono(&segment.samples);
        tracing::info!(
            session = self.session_id,
            segment_ms = segment.samples.len() / samples_for_ms(1),
            speech_ms = segment.speech_ms,
            bytes = wav.len(),
            "dictation: sending a segment"
        );
        let transcriber = Arc::clone(&self.transcriber);
        let clean_up = self.clean_up.clone();
        let cancel = self.cancel.clone();
        let prompt = prompt.to_string();
        Box::pin(async move {
            let Some(text) = transcriber.transcribe(wav, prompt, cancel.clone()).await? else {
                return Ok(None);
            };
            let Some(text) = clean_transcript(&text) else {
                return Ok(None);
            };
            Ok(Some(clean(clean_up, text, cancel).await))
        })
    }
}

/// Runs an API session to its end.
///
/// Frames go to the segmenter; each closed segment is queued and sent once
/// the one before it is answered, so segments arrive in order. A stop ends
/// capture and drains the queue; a cancel drops it and the request in flight.
/// A minute without speech, or a queue the service cannot keep up with, stops
/// the session as a stop does, with a notice.
pub async fn run_api(session: ApiSession, controls: Controls, mut out: Emitter) {
    let ApiSession {
        session_id,
        frames,
        capture,
        transcriber,
        clean_up,
        segmenter,
        silence_check,
        limits,
    } = session;
    let requests = Requests {
        session_id,
        transcriber,
        clean_up,
        cancel: controls.cancel.clone(),
    };
    let mut frames = Some(frames);
    let mut capture = Some(capture);
    let mut segmenter = Some(segmenter);
    let mut queue: VecDeque<Segment> = VecDeque::new();
    let mut in_flight: Option<BoxFuture<Answer>> = None;
    let mut reported = 0u32;
    let mut heard = String::new();
    let mut inserted = Inserted::default();
    let mut meter = LevelMeter::default();
    let mut silence = SilenceWatch::new(silence_check);
    let mut quiet = limits.quiet_watch();
    let mut quiet_stopped = false;
    let mut recording = false;
    let mut stopping = false;

    loop {
        if in_flight.is_none() {
            if let Some(segment) = queue.pop_front() {
                in_flight = Some(requests.send(&segment, &heard));
            }
        }
        let pending = u32::try_from(queue.len())
            .unwrap_or(u32::MAX)
            .saturating_add(u32::from(in_flight.is_some()));
        if pending != reported {
            reported = pending;
            out.emit(DictationEvent::Transcribing { pending });
        }
        if frames.is_none() && in_flight.is_none() {
            inserted.end(&mut out, quiet_stopped);
            return;
        }

        tokio::select! {
            biased;
            () = controls.cancel.cancelled() => {
                release(capture.take());
                out.emit(DictationEvent::Cancelled);
                return;
            }
            () = controls.stop.cancelled(), if !stopping => {
                stopping = true;
                release(capture.take());
            }
            frame = recv(&mut frames) => match frame {
                Some(frame) => {
                    if !stopping {
                        if !recording {
                            recording = true;
                            out.emit(DictationEvent::Recording);
                        }
                        if silence.is_muted(&frame.samples) {
                            release(capture.take());
                            out.fail(DictationError::Silence);
                            return;
                        }
                        if let Some(level) = meter.push(&frame) {
                            out.emit(DictationEvent::Level { level });
                        }
                        if quiet.push(&frame.samples) {
                            stopping = true;
                            quiet_stopped = true;
                            release(capture.take());
                            tracing::info!(session = session_id, "dictation: stopped after a minute without speech");
                            out.emit(DictationEvent::stopped_after_silence());
                        }
                    }
                    if let Some(segmenter) = segmenter.as_mut() {
                        queue.extend(segmenter.push(&frame.samples));
                    }
                    let queued: usize = queue.iter().map(|segment| segment.samples.len()).sum();
                    if !stopping && limits.is_queue_full(queued) {
                        stopping = true;
                        release(capture.take());
                        tracing::warn!(
                            session = session_id,
                            queued_ms = queued / samples_for_ms(1),
                            "dictation: the transcription service fell behind"
                        );
                        out.emit(DictationEvent::stopped_service_behind());
                    }
                }
                None => {
                    frames = None;
                    let failure = capture
                        .as_ref()
                        .filter(|_| !stopping)
                        .and_then(|capture| capture.failure());
                    release(capture.take());
                    if let Some(error) = failure {
                        out.fail(error.into());
                        return;
                    }
                    queue.extend(segmenter.take().and_then(Segmenter::finish));
                }
            },
            answer = settle(&mut in_flight) => {
                in_flight = None;
                match answer {
                    Ok(Some(text)) => {
                        remember(&mut heard, &text.text);
                        inserted.insert(&mut out, text);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!(
                            session = session_id,
                            code = ?error.code(),
                            "dictation: a segment failed"
                        );
                        release(capture.take());
                        out.fail(error);
                        return;
                    }
                }
            }
        }
    }
}

/// A session on Apple's on-device speech analyzer.
pub struct OnDeviceSession {
    /// The session's id, for the log.
    pub session_id: u32,
    /// The capture's frames.
    pub frames: mpsc::Receiver<AudioFrame>,
    /// The capture.
    pub capture: Box<dyn Capture>,
    /// The recognition.
    pub recognizer: Box<dyn Recognizer>,
    /// The recognition's events.
    pub events: mpsc::UnboundedReceiver<BridgeEvent>,
    /// The clean-up pass, when it is on.
    pub clean_up: Option<Arc<dyn CleanUp>>,
    /// The locale recognized, which names a failed model download.
    pub locale: String,
    /// Whether digital silence from the start ends the session.
    pub silence_check: bool,
    /// When the session stops by itself.
    pub limits: SessionLimits,
}

/// Holds audio that arrives before the recognition is ready; the oldest goes
/// first once the limit is reached.
fn hold(early: &mut VecDeque<i16>, samples: &[i16]) {
    early.extend(samples);
    let over = early.len().saturating_sub(PRE_READY_LIMIT);
    early.drain(..over);
}

/// Runs an on-device session to its end.
///
/// Capture runs from the start; audio taken while the model gets ready is
/// held and fed once it is. Partials go out at most every 100 ms; finals go
/// through the clean-up, in order, when it is on. A stop ends capture and
/// finishes the recognition, whose remaining text still goes in. A minute
/// without speech once recording starts stops the session the same way, with
/// a notice.
pub async fn run_on_device(session: OnDeviceSession, controls: Controls, mut out: Emitter) {
    let OnDeviceSession {
        session_id,
        frames,
        capture,
        recognizer,
        events,
        clean_up,
        locale,
        silence_check,
        limits,
    } = session;
    let mut frames = Some(frames);
    let mut events = Some(events);
    let mut capture = Some(capture);
    let mut early: VecDeque<i16> = VecDeque::new();
    let mut ready = false;
    let mut finishing = false;
    let mut finished = false;
    let mut partial: Option<String> = None;
    let mut partial_due: Option<Instant> = None;
    let mut last_partial: Option<Instant> = None;
    let mut finals: VecDeque<String> = VecDeque::new();
    let mut cleaning: Option<BoxFuture<Heard>> = None;
    let mut inserted = Inserted::default();
    let mut meter = LevelMeter::default();
    let mut silence = SilenceWatch::new(silence_check);
    let mut quiet = limits.quiet_watch();
    let mut quiet_stopped = false;
    let mut stopping = false;

    loop {
        if cleaning.is_none() {
            if let (Some(clean_up), Some(text)) = (clean_up.as_ref(), finals.front()) {
                let text = text.clone();
                finals.pop_front();
                cleaning = Some(Box::pin(clean(
                    Some(Arc::clone(clean_up)),
                    text,
                    controls.cancel.clone(),
                )));
            }
        }
        if finished && cleaning.is_none() {
            release(capture.take());
            inserted.end(&mut out, quiet_stopped);
            return;
        }

        tokio::select! {
            biased;
            () = controls.cancel.cancelled() => {
                recognizer.stop();
                release(capture.take());
                out.emit(DictationEvent::Cancelled);
                return;
            }
            () = controls.stop.cancelled(), if !stopping => {
                stopping = true;
                release(capture.take());
            }
            frame = recv(&mut frames) => match frame {
                Some(frame) => {
                    if !stopping && silence.is_muted(&frame.samples) {
                        recognizer.stop();
                        release(capture.take());
                        out.fail(DictationError::Silence);
                        return;
                    }
                    if ready {
                        recognizer.feed(&frame.samples);
                        if !stopping {
                            if let Some(level) = meter.push(&frame) {
                                out.emit(DictationEvent::Level { level });
                            }
                            if quiet.push(&frame.samples) {
                                stopping = true;
                                quiet_stopped = true;
                                release(capture.take());
                                tracing::info!(session = session_id, "dictation: stopped after a minute without speech");
                                out.emit(DictationEvent::stopped_after_silence());
                            }
                        }
                    } else {
                        hold(&mut early, &frame.samples);
                    }
                }
                None => {
                    frames = None;
                    let failure = capture
                        .as_ref()
                        .filter(|_| !stopping)
                        .and_then(|capture| capture.failure());
                    release(capture.take());
                    if let Some(error) = failure {
                        recognizer.stop();
                        out.fail(error.into());
                        return;
                    }
                    if ready && !finishing {
                        finishing = true;
                        recognizer.finish();
                    }
                }
            },
            event = recv_unbounded(&mut events) => match event {
                Some(BridgeEvent::Preparing(progress)) => {
                    if !ready {
                        out.emit(DictationEvent::Preparing { progress });
                    }
                }
                Some(BridgeEvent::Ready) => {
                    if !ready {
                        ready = true;
                        if !stopping {
                            out.emit(DictationEvent::Recording);
                        }
                        let held: Vec<i16> = early.drain(..).collect();
                        recognizer.feed(&held);
                        if frames.is_none() && !finishing {
                            finishing = true;
                            recognizer.finish();
                        }
                    }
                }
                Some(BridgeEvent::Partial(text)) => {
                    let now = Instant::now();
                    match last_partial.map(|at| at + PARTIAL_EVERY).filter(|due| *due > now) {
                        Some(due) => {
                            partial = Some(text);
                            partial_due = Some(due);
                        }
                        None => {
                            partial = None;
                            partial_due = None;
                            last_partial = Some(now);
                            out.emit(DictationEvent::Partial { text });
                        }
                    }
                }
                Some(BridgeEvent::Final(text)) => {
                    partial = None;
                    partial_due = None;
                    if let Some(text) = clean_transcript(&text) {
                        if clean_up.is_some() {
                            finals.push_back(text);
                        } else {
                            inserted.insert(&mut out, Heard { text, clean_up_failed: None });
                        }
                    }
                }
                Some(BridgeEvent::Failed(code)) => {
                    tracing::warn!(session = session_id, code, "dictation: the recognition failed");
                    release(capture.take());
                    out.fail(error_from_bridge(code, &locale));
                    return;
                }
                Some(BridgeEvent::Stopped) => {
                    release(capture.take());
                    out.emit(DictationEvent::Cancelled);
                    return;
                }
                Some(BridgeEvent::Finished) => {
                    finished = true;
                    events = None;
                }
                None => {
                    tracing::warn!(session = session_id, "dictation: the recognition ended without a word");
                    release(capture.take());
                    out.fail(DictationError::EngineFailed { code: 0 });
                    return;
                }
            },
            () = wait_until(partial_due) => {
                partial_due = None;
                if let Some(text) = partial.take() {
                    last_partial = Some(Instant::now());
                    out.emit(DictationEvent::Partial { text });
                }
            }
            heard = settle(&mut cleaning) => {
                cleaning = None;
                inserted.insert(&mut out, heard);
            }
        }
    }
}

/// The transcription API's requests for one session.
pub struct ApiTranscriber {
    client: reqwest::Client,
    prepared: Arc<PreparedTranscription>,
}

impl ApiTranscriber {
    /// Requests for `prepared` over `client`.
    pub fn new(client: reqwest::Client, prepared: PreparedTranscription) -> Self {
        Self {
            client,
            prepared: Arc::new(prepared),
        }
    }
}

impl Transcribe for ApiTranscriber {
    fn transcribe(
        &self,
        wav: Vec<u8>,
        prompt: String,
        cancel: CancellationToken,
    ) -> BoxFuture<Result<Option<String>, DictationError>> {
        let client = self.client.clone();
        let prepared = Arc::clone(&self.prepared);
        Box::pin(async move {
            writ_ai::transcription::transcribe(&client, &prepared, wav, Some(&prompt), &cancel)
                .await
        })
    }
}

/// A clean-up request ready to run.
enum PreparedCleanUp {
    Hosted(PreparedRequest, reqwest::Client),
    OnDevice(OnDeviceRequest),
}

/// The clean-up pass over the AI connection, as the session started with it.
pub struct AiCleanUp {
    ai: AiConfig,
    key: Option<String>,
    client: Mutex<Option<(bool, reqwest::Client)>>,
}

impl AiCleanUp {
    /// The pass over `ai`, with the connection's key read at the start.
    pub fn new(ai: AiConfig, key: Option<String>) -> Self {
        Self {
            ai,
            key,
            client: Mutex::new(None),
        }
    }

    fn client(&self, is_private_network: bool) -> Result<reqwest::Client, String> {
        let mut cached =
            recover_poison(self.client.lock(), "dictation::session::AiCleanUp::client");
        if let Some((private, client)) = cached.as_ref() {
            if *private == is_private_network {
                return Ok(client.clone());
            }
        }
        let client = build_client_for(is_private_network)?;
        *cached = Some((is_private_network, client.clone()));
        Ok(client)
    }

    fn prepare(&self, text: &str) -> Result<PreparedCleanUp, String> {
        match prepare_clean_up(&self.ai, text, |_| self.key.clone()).map_err(|e| e.to_string())? {
            CleanUpRequest::Hosted(request) => {
                let client = self.client(request.is_private_network())?;
                Ok(PreparedCleanUp::Hosted(request, client))
            }
            CleanUpRequest::OnDevice(request) => Ok(PreparedCleanUp::OnDevice(request)),
        }
    }
}

impl CleanUp for AiCleanUp {
    fn clean_up(
        &self,
        text: String,
        cancel: CancellationToken,
    ) -> BoxFuture<Result<String, String>> {
        let prepared = self.prepare(&text);
        Box::pin(async move {
            match prepared? {
                PreparedCleanUp::Hosted(request, client) => {
                    run_clean_up(&client, &request, &cancel).await
                }
                PreparedCleanUp::OnDevice(request) => clean_up_on_device(&request, &cancel).await,
            }
        })
    }
}

/// [`run_clean_up`] on Apple's on-device model.
async fn clean_up_on_device(
    request: &OnDeviceRequest,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let mut text = String::new();
    let mut ended: Option<Result<(), String>> = None;
    crate::on_device::run_rewrite(request, cancel, |event| match event {
        StreamEvent::Chunk(chunk) => text.push_str(&chunk),
        StreamEvent::Done => ended = Some(Ok(())),
        StreamEvent::Error(message) => ended = Some(Err(message)),
    })
    .await;
    match ended {
        Some(Ok(())) => {
            clean_transcript(&text).ok_or_else(|| "The clean-up returned no text.".to_string())
        }
        Some(Err(message)) => Err(message),
        None => Err("The clean-up was stopped.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use writ_ai::transcription::prepare_transcription;
    use writ_core::config::{DictationConfig, DictationEngine, DictationProvider};
    use writ_core::dictation::on_device::BRIDGE_ASSETS_FAILED;
    use writ_core::dictation::{
        DictationErrorCode, SegmenterParams, QUEUE_LIMIT_MS, QUIET_STOP_MS,
    };

    use super::*;

    const PER_MS: usize = 16;
    const WAIT: Duration = Duration::from_secs(10);

    fn silence(ms: usize) -> Vec<i16> {
        vec![0; ms * PER_MS]
    }

    /// A steady 220 Hz tone at about -13.5 dBFS.
    fn tone(ms: usize) -> Vec<i16> {
        (0..ms * PER_MS)
            .map(|i| {
                let t = i as f32 / 16_000.0;
                ((t * 220.0 * std::f32::consts::TAU).sin() * 0.3 * f32::from(i16::MAX)) as i16
            })
            .collect()
    }

    /// `parts` in order, cut into 20 ms frames.
    fn frames_of(parts: &[Vec<i16>]) -> Vec<AudioFrame> {
        parts
            .concat()
            .chunks(20 * PER_MS)
            .map(|chunk| AudioFrame::from_samples(chunk.to_vec()))
            .collect()
    }

    fn two_phrases() -> Vec<AudioFrame> {
        frames_of(&[tone(400), silence(400), tone(400), silence(400)])
    }

    fn one_phrase() -> Vec<AudioFrame> {
        frames_of(&[tone(400), silence(400)])
    }

    fn quick_segmenter() -> Segmenter {
        Segmenter::new(SegmenterParams {
            frame_ms: 20,
            close_after_silence_ms: 200,
            min_speech_to_close_ms: 200,
            max_segment_ms: 30_000,
            pre_roll_ms: 60,
            tail_ms: 60,
            min_speech_ms: 100,
        })
    }

    struct FakeCapture {
        _frames: Option<mpsc::Sender<AudioFrame>>,
        failure: Option<CaptureError>,
    }

    impl Capture for FakeCapture {
        fn failure(&self) -> Option<CaptureError> {
            self.failure.clone()
        }
    }

    /// A channel holding `frames`. With `open` the capture holds the sender,
    /// so the channel closes once the session lets the capture go; otherwise
    /// it closes once the frames are read.
    fn capture(
        frames: Vec<AudioFrame>,
        open: bool,
        failure: Option<CaptureError>,
    ) -> (mpsc::Receiver<AudioFrame>, Box<dyn Capture>) {
        let (sender, receiver) = mpsc::channel(frames.len().max(1));
        for frame in frames {
            sender.try_send(frame).expect("the channel has room");
        }
        let capture = FakeCapture {
            _frames: open.then_some(sender),
            failure,
        };
        (receiver, Box::new(capture))
    }

    enum Reply {
        Text(&'static str),
        Fail(DictationError),
        /// Answers only after the cancel, which the session must ignore.
        Late,
    }

    #[derive(Default)]
    struct FakeTranscriber {
        replies: Mutex<VecDeque<Reply>>,
        prompts: Mutex<Vec<String>>,
    }

    impl FakeTranscriber {
        fn replying(replies: Vec<Reply>) -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(replies.into()),
                prompts: Mutex::default(),
            })
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts.lock().expect("prompts").clone()
        }
    }

    impl Transcribe for FakeTranscriber {
        fn transcribe(
            &self,
            _wav: Vec<u8>,
            prompt: String,
            cancel: CancellationToken,
        ) -> BoxFuture<Result<Option<String>, DictationError>> {
            self.prompts.lock().expect("prompts").push(prompt);
            let reply = self
                .replies
                .lock()
                .expect("replies")
                .pop_front()
                .unwrap_or(Reply::Text("more"));
            Box::pin(async move {
                match reply {
                    Reply::Text(text) => Ok(Some(text.to_string())),
                    Reply::Fail(error) => Err(error),
                    Reply::Late => {
                        cancel.cancelled().await;
                        Ok(Some("late".to_string()))
                    }
                }
            })
        }
    }

    enum FakeCleanUp {
        Upper,
        Fail(&'static str),
    }

    impl CleanUp for FakeCleanUp {
        fn clean_up(
            &self,
            text: String,
            _cancel: CancellationToken,
        ) -> BoxFuture<Result<String, String>> {
            let answer = match self {
                Self::Upper => Ok(text.to_uppercase()),
                Self::Fail(reason) => Err((*reason).to_string()),
            };
            Box::pin(async move { answer })
        }
    }

    fn api(
        frames: Vec<AudioFrame>,
        open: bool,
        failure: Option<CaptureError>,
        transcriber: Arc<FakeTranscriber>,
        clean_up: Option<FakeCleanUp>,
    ) -> ApiSession {
        let (frames, capture) = capture(frames, open, failure);
        ApiSession {
            session_id: 1,
            frames,
            capture,
            transcriber,
            clean_up: clean_up.map(|c| Arc::new(c) as Arc<dyn CleanUp>),
            segmenter: quick_segmenter(),
            silence_check: false,
            limits: SessionLimits::default(),
        }
    }

    /// Limits that stop a session after `quiet_ms` without speech or once
    /// `queue_ms` of audio waits.
    fn short_limits(quiet_ms: u32, queue_ms: u32) -> SessionLimits {
        SessionLimits {
            quiet_stop_ms: quiet_ms,
            queue_ms,
        }
    }

    fn events_channel() -> (Emitter, mpsc::UnboundedReceiver<DictationEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let emitter = Emitter::new(move |event| {
            let _ = sender.send(event);
        });
        (emitter, receiver)
    }

    fn start_api(
        session: ApiSession,
        controls: Controls,
    ) -> mpsc::UnboundedReceiver<DictationEvent> {
        let (emitter, receiver) = events_channel();
        tokio::spawn(run_api(session, controls, emitter));
        receiver
    }

    /// Every event until the session ends and drops its emitter.
    async fn rest(receiver: &mut mpsc::UnboundedReceiver<DictationEvent>) -> Vec<DictationEvent> {
        tokio::time::timeout(WAIT, async {
            let mut all = Vec::new();
            while let Some(event) = receiver.recv().await {
                all.push(event);
            }
            all
        })
        .await
        .expect("the session ends")
    }

    /// Events until one matching `wanted`, which is included.
    async fn until(
        receiver: &mut mpsc::UnboundedReceiver<DictationEvent>,
        wanted: impl Fn(&DictationEvent) -> bool,
    ) -> Vec<DictationEvent> {
        tokio::time::timeout(WAIT, async {
            let mut seen = Vec::new();
            while let Some(event) = receiver.recv().await {
                let found = wanted(&event);
                seen.push(event);
                if found {
                    return seen;
                }
            }
            panic!("the session ended first: {seen:?}");
        })
        .await
        .expect("the event arrives")
    }

    /// Short names for the events, with runs of levels folded into one.
    fn names(events: &[DictationEvent]) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for event in events {
            let name = match event {
                DictationEvent::Preparing { .. } => "preparing".to_string(),
                DictationEvent::Recording => "recording".to_string(),
                DictationEvent::Level { .. } => "level".to_string(),
                DictationEvent::Partial { text } => format!("partial {text}"),
                DictationEvent::Transcribing { pending } => format!("transcribing {pending}"),
                DictationEvent::Segment { seq, text } => format!("segment {seq} {text}"),
                DictationEvent::Notice { message } => format!("notice {message}"),
                DictationEvent::Done => "done".to_string(),
                DictationEvent::Cancelled => "cancelled".to_string(),
                DictationEvent::Error { failure } => format!("error {:?}", failure.code),
            };
            if name == "level" && names.last().is_some_and(|last| last == "level") {
                continue;
            }
            names.push(name);
        }
        names
    }

    fn without_progress(events: &[DictationEvent]) -> Vec<DictationEvent> {
        events
            .iter()
            .filter(|event| !matches!(event, DictationEvent::Transcribing { .. }))
            .cloned()
            .collect()
    }

    /// The names of the events that carry text or end the session.
    fn words(events: &[DictationEvent]) -> Vec<String> {
        events
            .iter()
            .filter(|event| {
                !matches!(
                    event,
                    DictationEvent::Recording
                        | DictationEvent::Level { .. }
                        | DictationEvent::Transcribing { .. }
                )
            })
            .flat_map(|event| names(std::slice::from_ref(event)))
            .collect()
    }

    fn levels(events: &[DictationEvent]) -> usize {
        events
            .iter()
            .filter(|event| matches!(event, DictationEvent::Level { .. }))
            .count()
    }

    #[tokio::test]
    async fn segments_arrive_in_order_then_done() {
        let transcriber =
            FakeTranscriber::replying(vec![Reply::Text("first"), Reply::Text("second")]);
        let mut events = start_api(
            api(two_phrases(), false, None, Arc::clone(&transcriber), None),
            Controls::default(),
        );
        let all = rest(&mut events).await;

        assert_eq!(
            names(&without_progress(&all)),
            [
                "recording",
                "level",
                "segment 0 first",
                "segment 1 second",
                "done"
            ]
        );
        assert_eq!(transcriber.prompts(), ["", "first"]);
        assert!(names(&all).contains(&"transcribing 1".to_string()));
        assert_eq!(
            names(&all).iter().rev().nth(1).map(String::as_str),
            Some("transcribing 0"),
            "the count goes back to zero before the end"
        );
    }

    #[tokio::test]
    async fn levels_go_out_at_most_twenty_times_a_second() {
        let transcriber = FakeTranscriber::replying(vec![]);
        let frames = two_phrases();
        let ms: usize = frames.iter().map(|f| f.samples.len()).sum::<usize>() / PER_MS;
        let mut events = start_api(
            api(frames, false, None, transcriber, None),
            Controls::default(),
        );
        let all = rest(&mut events).await;

        let count = levels(&all);
        assert!(count > 0);
        assert!(count <= ms / 50, "{count} levels in {ms} ms");
    }

    #[tokio::test]
    async fn a_cancel_while_transcribing_ends_without_a_later_segment() {
        let transcriber = FakeTranscriber::replying(vec![Reply::Late]);
        let controls = Controls::default();
        let mut events = start_api(
            api(one_phrase(), true, None, transcriber, None),
            controls.clone(),
        );
        until(&mut events, |e| {
            matches!(e, DictationEvent::Transcribing { pending: 1 })
        })
        .await;
        controls.cancel.cancel();
        let after = names(&rest(&mut events).await);

        assert_eq!(after.last().map(String::as_str), Some("cancelled"));
        assert!(
            after.iter().all(|name| !name.starts_with("segment")),
            "{after:?}"
        );
    }

    #[tokio::test]
    async fn a_stop_after_silence_only_is_no_speech() {
        let transcriber = FakeTranscriber::replying(vec![]);
        let controls = Controls::default();
        let mut events = start_api(
            api(
                frames_of(&[silence(1_000)]),
                true,
                None,
                Arc::clone(&transcriber),
                None,
            ),
            controls.clone(),
        );
        until(&mut events, |e| matches!(e, DictationEvent::Recording)).await;
        controls.stop.cancel();
        let after = names(&rest(&mut events).await);

        assert_eq!(after.last().map(String::as_str), Some("error NoSpeech"));
        assert!(transcriber.prompts().is_empty(), "nothing was sent");
    }

    #[tokio::test]
    async fn a_stop_mid_phrase_sends_what_was_heard() {
        let transcriber = FakeTranscriber::replying(vec![Reply::Text("cut short")]);
        let controls = Controls::default();
        let mut events = start_api(
            api(frames_of(&[tone(400)]), true, None, transcriber, None),
            controls.clone(),
        );
        until(&mut events, |e| matches!(e, DictationEvent::Recording)).await;
        controls.stop.cancel();
        let after = words(&rest(&mut events).await);

        assert!(
            after.ends_with(&["segment 0 cut short".to_string(), "done".to_string()]),
            "{after:?}"
        );
    }

    #[tokio::test]
    async fn a_minute_without_speech_stops_and_inserts_what_was_heard() {
        let transcriber = FakeTranscriber::replying(vec![Reply::Text("first")]);
        let mut session = api(
            frames_of(&[tone(400), silence(1_400)]),
            true,
            None,
            transcriber,
            None,
        );
        session.limits = short_limits(1_000, QUEUE_LIMIT_MS);
        let mut events = start_api(session, Controls::default());
        let all = words(&rest(&mut events).await);

        assert_eq!(
            all,
            [
                "notice Dictation stopped after a minute of silence.",
                "segment 0 first",
                "done"
            ]
        );
    }

    #[tokio::test]
    async fn a_quiet_stop_with_nothing_heard_is_done_not_an_error() {
        let transcriber = FakeTranscriber::replying(vec![]);
        let mut session = api(
            frames_of(&[silence(1_400)]),
            true,
            None,
            Arc::clone(&transcriber),
            None,
        );
        session.limits = short_limits(1_000, QUEUE_LIMIT_MS);
        let mut events = start_api(session, Controls::default());
        let all = words(&rest(&mut events).await);

        assert_eq!(
            all,
            [
                "notice Dictation stopped after a minute of silence.",
                "done"
            ]
        );
        assert!(transcriber.prompts().is_empty(), "nothing was sent");
    }

    #[tokio::test]
    async fn a_full_queue_stops_capture_and_still_inserts_every_segment() {
        let transcriber = FakeTranscriber::replying(vec![
            Reply::Text("one"),
            Reply::Text("two"),
            Reply::Text("three"),
            Reply::Text("four"),
            Reply::Text("five"),
        ]);
        let phrases: Vec<Vec<i16>> = (0..5).flat_map(|_| [tone(400), silence(400)]).collect();
        let mut session = api(frames_of(&phrases), true, None, transcriber, None);
        session.limits = short_limits(QUIET_STOP_MS, 1_000);
        let mut events = start_api(session, Controls::default());
        let all = words(&rest(&mut events).await);

        assert_eq!(
            all,
            [
                "notice Dictation stopped because the transcription service could not keep up.",
                "segment 0 one",
                "segment 1 two",
                "segment 2 three",
                "segment 3 four",
                "segment 4 five",
                "done"
            ]
        );
    }

    #[tokio::test]
    async fn a_network_failure_keeps_the_segment_before_it() {
        let transcriber = FakeTranscriber::replying(vec![
            Reply::Text("first"),
            Reply::Fail(DictationError::Network {
                host: "api.example.com".to_string(),
            }),
        ]);
        let mut events = start_api(
            api(two_phrases(), false, None, transcriber, None),
            Controls::default(),
        );
        let all = words(&rest(&mut events).await);

        assert_eq!(all, ["segment 0 first", "error Network"]);
        assert!(!all.contains(&"done".to_string()));
    }

    #[tokio::test]
    async fn a_failed_clean_up_inserts_the_raw_text_with_one_notice() {
        let transcriber =
            FakeTranscriber::replying(vec![Reply::Text("first"), Reply::Text("second")]);
        let mut events = start_api(
            api(
                two_phrases(),
                false,
                None,
                transcriber,
                Some(FakeCleanUp::Fail("the model is busy")),
            ),
            Controls::default(),
        );
        let all = words(&rest(&mut events).await);

        assert_eq!(
            all,
            [
                "segment 0 first",
                "notice Inserted without clean-up: the model is busy",
                "segment 1 second",
                "done"
            ]
        );
    }

    #[tokio::test]
    async fn a_clean_up_replaces_the_text() {
        let transcriber = FakeTranscriber::replying(vec![Reply::Text("first")]);
        let mut events = start_api(
            api(
                one_phrase(),
                false,
                None,
                transcriber,
                Some(FakeCleanUp::Upper),
            ),
            Controls::default(),
        );
        let all = words(&rest(&mut events).await);

        assert_eq!(all, ["segment 0 FIRST", "done"]);
    }

    #[tokio::test]
    async fn a_transcript_without_words_is_skipped() {
        let transcriber =
            FakeTranscriber::replying(vec![Reply::Text(" ... "), Reply::Text("second")]);
        let mut events = start_api(
            api(two_phrases(), false, None, transcriber, None),
            Controls::default(),
        );
        let all = words(&rest(&mut events).await);

        assert_eq!(all, ["segment 0 second", "done"]);
    }

    #[tokio::test]
    async fn a_capture_that_fails_ends_the_session_with_its_error() {
        let transcriber = FakeTranscriber::replying(vec![]);
        let mut events = start_api(
            api(
                frames_of(&[tone(200)]),
                false,
                Some(CaptureError::DeviceLost("the input went away".to_string())),
                Arc::clone(&transcriber),
                None,
            ),
            Controls::default(),
        );
        let all = names(&rest(&mut events).await);

        assert_eq!(all.last().map(String::as_str), Some("error CaptureFailed"));
        assert!(transcriber.prompts().is_empty(), "the queue was dropped");
    }

    #[tokio::test]
    async fn digital_silence_from_the_start_means_a_muted_microphone() {
        let transcriber = FakeTranscriber::replying(vec![]);
        let mut session = api(frames_of(&[silence(1_600)]), true, None, transcriber, None);
        session.silence_check = true;
        let mut events = start_api(session, Controls::default());
        let all = names(&rest(&mut events).await);

        assert_eq!(all.last().map(String::as_str), Some("error Silence"));
    }

    #[tokio::test]
    async fn sound_before_the_silence_is_a_working_microphone() {
        let transcriber = FakeTranscriber::replying(vec![Reply::Text("first")]);
        let mut session = api(
            frames_of(&[tone(400), silence(2_000)]),
            false,
            None,
            transcriber,
            None,
        );
        session.silence_check = true;
        let mut events = start_api(session, Controls::default());
        let all = words(&rest(&mut events).await);

        assert_eq!(all, ["segment 0 first", "done"]);
    }

    #[test]
    fn the_prompt_keeps_the_end_of_what_was_heard() {
        let mut heard = String::new();
        remember(&mut heard, "first");
        remember(&mut heard, "second");
        assert_eq!(heard, "first second");
        remember(&mut heard, &"é".repeat(300));
        assert_eq!(heard.chars().count(), PROMPT_CHARS);
    }

    #[test]
    fn nothing_follows_the_event_that_ends_a_session() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let mut out = Emitter::new(move |event| sink.lock().expect("seen").push(event));
        out.emit(DictationEvent::Recording);
        out.emit(DictationEvent::Cancelled);
        out.emit(DictationEvent::Done);
        out.fail(DictationError::NoSpeech);
        assert_eq!(
            *seen.lock().expect("seen"),
            [DictationEvent::Recording, DictationEvent::Cancelled]
        );
    }

    /// A log sink the tests read back.
    #[derive(Clone, Default)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("log").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_window_that_stops_listening_is_logged_once_per_session() {
        let log = LogBuffer::default();
        let sink = log.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || sink.clone())
            .with_ansi(false)
            .finish();
        let attempts = Arc::new(Mutex::new(0u32));
        let counted = Arc::clone(&attempts);
        tracing::subscriber::with_default(subscriber, || {
            let mut out = Emitter::logging_failures(9, move |_event| {
                *counted.lock().expect("attempts") += 1;
                Err("the page is gone")
            });
            for _ in 0..3 {
                out.emit(DictationEvent::Recording);
            }
        });

        assert_eq!(
            *attempts.lock().expect("attempts"),
            3,
            "every event is tried"
        );
        let text = String::from_utf8(log.0.lock().expect("log").clone()).expect("utf-8");
        assert_eq!(
            text.matches("dictation events stopped reaching the window")
                .count(),
            1,
            "{text}"
        );
        assert!(text.contains("session=9"), "{text}");
    }

    /// What the session did to the recognition.
    #[derive(Debug, Default)]
    struct RecognizerLog {
        fed: usize,
        finished: bool,
        stopped: bool,
    }

    /// A recognition that answers a finish with `on_finish`, then `Finished`,
    /// and a stop with `Stopped`, as the bridge does.
    struct FakeRecognizer {
        log: Arc<Mutex<RecognizerLog>>,
        events: Option<mpsc::UnboundedSender<BridgeEvent>>,
        on_finish: Vec<BridgeEvent>,
    }

    impl Recognizer for FakeRecognizer {
        fn feed(&self, samples: &[i16]) {
            self.log.lock().expect("log").fed += samples.len();
        }

        fn finish(&self) {
            self.log.lock().expect("log").finished = true;
            if let Some(events) = &self.events {
                for event in &self.on_finish {
                    let _ = events.send(event.clone());
                }
                let _ = events.send(BridgeEvent::Finished);
            }
        }

        fn stop(&self) {
            self.log.lock().expect("log").stopped = true;
            if let Some(events) = &self.events {
                let _ = events.send(BridgeEvent::Stopped);
            }
        }
    }

    struct OnDeviceRun {
        events: mpsc::UnboundedReceiver<DictationEvent>,
        bridge: mpsc::UnboundedSender<BridgeEvent>,
        log: Arc<Mutex<RecognizerLog>>,
        controls: Controls,
    }

    fn start_on_device(
        frames: Vec<AudioFrame>,
        open: bool,
        on_finish: Vec<BridgeEvent>,
        clean_up: Option<FakeCleanUp>,
        answers: bool,
    ) -> OnDeviceRun {
        let (frames, capture) = capture(frames, open, None);
        start_on_device_with(
            frames,
            capture,
            on_finish,
            clean_up,
            answers,
            SessionLimits::default(),
        )
    }

    fn start_on_device_with(
        frames: mpsc::Receiver<AudioFrame>,
        capture: Box<dyn Capture>,
        on_finish: Vec<BridgeEvent>,
        clean_up: Option<FakeCleanUp>,
        answers: bool,
        limits: SessionLimits,
    ) -> OnDeviceRun {
        let (bridge, bridge_events) = mpsc::unbounded_channel();
        let log = Arc::new(Mutex::new(RecognizerLog::default()));
        let recognizer = FakeRecognizer {
            log: Arc::clone(&log),
            events: answers.then(|| bridge.clone()),
            on_finish,
        };
        let session = OnDeviceSession {
            session_id: 2,
            frames,
            capture,
            recognizer: Box::new(recognizer),
            events: bridge_events,
            clean_up: clean_up.map(|c| Arc::new(c) as Arc<dyn CleanUp>),
            locale: "en-US".to_string(),
            silence_check: false,
            limits,
        };
        let controls = Controls::default();
        let (emitter, events) = events_channel();
        tokio::spawn(run_on_device(session, controls.clone(), emitter));
        OnDeviceRun {
            events,
            bridge,
            log,
            controls,
        }
    }

    #[tokio::test]
    async fn on_device_audio_waits_for_the_model_then_text_goes_in() {
        let frames = frames_of(&[tone(800)]);
        let mut run = start_on_device(
            frames,
            true,
            vec![BridgeEvent::Final("again".into())],
            None,
            true,
        );
        run.bridge
            .send(BridgeEvent::Preparing(Some(0.5)))
            .expect("send");
        until(&mut run.events, |e| {
            matches!(e, DictationEvent::Preparing { .. })
        })
        .await;
        assert_eq!(
            run.log.lock().expect("log").fed,
            0,
            "nothing is fed before ready"
        );

        run.bridge.send(BridgeEvent::Ready).expect("send");
        until(&mut run.events, |e| matches!(e, DictationEvent::Recording)).await;
        assert_eq!(
            run.log.lock().expect("log").fed,
            800 * PER_MS,
            "the held audio is fed"
        );

        for event in [
            BridgeEvent::Partial("hel".into()),
            BridgeEvent::Partial("hello".into()),
            BridgeEvent::Final("hello".into()),
        ] {
            run.bridge.send(event).expect("send");
        }
        let heard = names(
            &until(&mut run.events, |e| {
                matches!(e, DictationEvent::Segment { .. })
            })
            .await,
        );
        assert_eq!(
            heard,
            ["partial hel", "segment 0 hello"],
            "the second partial was folded"
        );

        run.controls.stop.cancel();
        let after = names(&rest(&mut run.events).await);
        assert_eq!(after, ["segment 1 again", "done"]);
        let log = run.log.lock().expect("log");
        assert!(log.finished && !log.stopped);
    }

    #[tokio::test]
    async fn a_partial_folded_away_still_goes_out_after_the_pause() {
        let mut run = start_on_device(Vec::new(), true, Vec::new(), None, true);
        run.bridge.send(BridgeEvent::Ready).expect("send");
        run.bridge
            .send(BridgeEvent::Partial("one".into()))
            .expect("send");
        run.bridge
            .send(BridgeEvent::Partial("one two".into()))
            .expect("send");
        let partials = names(
            &until(
                &mut run.events,
                |e| matches!(e, DictationEvent::Partial { text } if text == "one two"),
            )
            .await,
        );
        assert_eq!(partials, ["recording", "partial one", "partial one two"]);
        run.controls.cancel.cancel();
        rest(&mut run.events).await;
    }

    #[tokio::test]
    async fn a_stop_while_the_model_prepares_finishes_once_it_is_ready() {
        let mut run = start_on_device(frames_of(&[tone(200)]), true, Vec::new(), None, true);
        run.bridge.send(BridgeEvent::Preparing(None)).expect("send");
        until(&mut run.events, |e| {
            matches!(e, DictationEvent::Preparing { .. })
        })
        .await;
        run.controls.stop.cancel();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!run.log.lock().expect("log").finished, "not before ready");

        run.bridge.send(BridgeEvent::Ready).expect("send");
        let after = names(&rest(&mut run.events).await);
        assert_eq!(after, ["error NoSpeech"], "no recording state after a stop");
        let log = run.log.lock().expect("log");
        assert_eq!(log.fed, 200 * PER_MS);
        assert!(log.finished);
    }

    #[tokio::test]
    async fn audio_held_before_ready_keeps_only_the_last_five_seconds() {
        let mut run = start_on_device(frames_of(&[tone(6_000)]), true, Vec::new(), None, true);
        tokio::time::sleep(Duration::from_millis(50)).await;
        run.bridge.send(BridgeEvent::Ready).expect("send");
        until(&mut run.events, |e| matches!(e, DictationEvent::Recording)).await;
        assert_eq!(run.log.lock().expect("log").fed, PRE_READY_LIMIT);
        run.controls.cancel.cancel();
        rest(&mut run.events).await;
    }

    #[tokio::test]
    async fn an_on_device_cancel_stops_the_recognition() {
        let mut run = start_on_device(Vec::new(), true, Vec::new(), None, true);
        run.bridge.send(BridgeEvent::Ready).expect("send");
        until(&mut run.events, |e| matches!(e, DictationEvent::Recording)).await;
        run.controls.cancel.cancel();
        assert_eq!(names(&rest(&mut run.events).await), ["cancelled"]);
        assert!(run.log.lock().expect("log").stopped);
    }

    #[tokio::test]
    async fn a_failed_model_download_names_the_language() {
        let mut run = start_on_device(Vec::new(), true, Vec::new(), None, true);
        run.bridge
            .send(BridgeEvent::Failed(BRIDGE_ASSETS_FAILED))
            .expect("send");
        let all = rest(&mut run.events).await;
        let Some(DictationEvent::Error { failure }) = all.last() else {
            panic!("{all:?}");
        };
        assert_eq!(failure.code, DictationErrorCode::AssetsFailed);
        assert!(failure.message.contains("English"), "{}", failure.message);
    }

    #[tokio::test]
    async fn a_recognition_that_ends_without_a_word_is_an_engine_failure() {
        let mut run = start_on_device(Vec::new(), true, Vec::new(), None, false);
        let OnDeviceRun { events, bridge, .. } = &mut run;
        drop(std::mem::replace(bridge, mpsc::unbounded_channel().0));
        let all = names(&rest(events).await);
        assert_eq!(all, ["error EngineFailed"]);
    }

    #[tokio::test]
    async fn on_device_finals_go_through_the_clean_up_in_order() {
        let mut run = start_on_device(
            Vec::new(),
            true,
            Vec::new(),
            Some(FakeCleanUp::Fail("no connection")),
            true,
        );
        run.bridge.send(BridgeEvent::Ready).expect("send");
        until(&mut run.events, |e| matches!(e, DictationEvent::Recording)).await;
        for event in [
            BridgeEvent::Final("one".into()),
            BridgeEvent::Final("two".into()),
        ] {
            run.bridge.send(event).expect("send");
        }
        run.controls.stop.cancel();
        let all = names(&rest(&mut run.events).await);
        assert_eq!(
            all,
            [
                "segment 0 one",
                "notice Inserted without clean-up: no connection",
                "segment 1 two",
                "done"
            ]
        );
    }

    #[tokio::test]
    async fn a_minute_without_speech_finishes_the_on_device_recognition() {
        let (sender, frames) = mpsc::channel(256);
        let capture = Box::new(FakeCapture {
            _frames: Some(sender.clone()),
            failure: None,
        });
        let mut run = start_on_device_with(
            frames,
            capture,
            vec![BridgeEvent::Final("still heard".into())],
            None,
            true,
            short_limits(1_000, QUEUE_LIMIT_MS),
        );
        run.bridge.send(BridgeEvent::Ready).expect("send");
        until(&mut run.events, |e| matches!(e, DictationEvent::Recording)).await;
        for frame in frames_of(&[silence(1_400)]) {
            sender.send(frame).await.expect("send");
        }
        drop(sender);
        let after = words(&rest(&mut run.events).await);

        assert_eq!(
            after,
            [
                "notice Dictation stopped after a minute of silence.",
                "segment 0 still heard",
                "done"
            ]
        );
        let log = run.log.lock().expect("log");
        assert!(log.finished && !log.stopped);
    }

    /// A transcription server on a local port that answers each request, in
    /// order, with the next of `bodies`, and keeps what it was sent.
    fn transcription_server(bodies: Vec<&'static str>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        std::thread::spawn(move || {
            for body in bodies {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut request = Vec::new();
                let mut chunk = [0u8; 64 * 1024];
                while let Ok(n) = stream.read(&mut chunk) {
                    request.extend_from_slice(&chunk[..n]);
                    if n == 0 || request.ends_with(b"--\r\n") {
                        break;
                    }
                }
                recorder
                    .lock()
                    .expect("seen")
                    .push(String::from_utf8_lossy(&request).to_string());
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{port}/v1"), seen)
    }

    #[tokio::test]
    async fn a_session_against_a_local_server_inserts_its_answers() {
        let (base_url, seen) = transcription_server(vec![
            r#"{"text":"hello there"}"#,
            r#"{"text":"general kenobi"}"#,
        ]);
        let d = DictationConfig {
            enabled: true,
            engine: Some(DictationEngine::Api),
            provider: DictationProvider::Custom,
            base_url,
            model: "whisper-1".to_string(),
            ..DictationConfig::default()
        };
        let prepared = prepare_transcription(&d, &AiConfig::default(), false, |_| None)
            .expect("a local endpoint needs no consent or key");
        let client = build_client_for(prepared.is_private_network()).expect("client");
        let (frames, capture) = capture(two_phrases(), false, None);
        let session = ApiSession {
            session_id: 3,
            frames,
            capture,
            transcriber: Arc::new(ApiTranscriber::new(client, prepared)),
            clean_up: None,
            segmenter: quick_segmenter(),
            silence_check: false,
            limits: SessionLimits::default(),
        };
        let mut events = start_api(session, Controls::default());
        let all = words(&rest(&mut events).await);

        assert_eq!(
            all,
            ["segment 0 hello there", "segment 1 general kenobi", "done"]
        );
        let seen = seen.lock().expect("seen");
        assert_eq!(seen.len(), 2);
        assert!(
            seen[0].starts_with("POST /v1/audio/transcriptions"),
            "{}",
            seen[0]
        );
        assert!(seen[0].contains("RIFF"), "the segment goes as WAV");
        assert!(
            seen[1].contains("hello there"),
            "the second request carries the first as its prompt"
        );
    }
}
