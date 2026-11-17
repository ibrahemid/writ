//! Cutting a stream of 16 kHz samples into segments at pauses.
//!
//! Each frame's RMS level is compared with a noise floor that follows the
//! quietest recent frames: it drops at once to a quieter frame and rises
//! slowly otherwise. A frame is speech when it is clearly above that floor
//! and above an absolute minimum, so steady fan or room noise never counts.
//!
//! A segment opens on speech with the frames just before it, closes after a
//! pause once it holds enough speech, and is cut at its quietest recent
//! frame when it reaches the length limit. A segment with too little speech
//! is dropped.

use std::collections::VecDeque;

use super::level::rms_dbfs;
use super::samples_for_ms;

/// A frame is speech only above this level.
const ABS_MIN_DBFS: f32 = -50.0;
/// A frame is speech only this far above the noise floor.
const MARGIN_DB: f32 = 10.0;
/// The first frame sets the floor, at most this high, so a session that
/// starts mid-word still finds the word.
const FLOOR_START_CAP_DBFS: f32 = -40.0;
/// How fast the floor rises toward louder frames.
const FLOOR_RISE_DB_PER_S: f32 = 3.3;
/// The hard cut looks for its quietest frame in this much recent audio.
const CUT_WINDOW_MS: u32 = 5_000;
/// The shortest frame the segmenter works with.
const MIN_FRAME_MS: u32 = 10;

/// Tuning for [`Segmenter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmenterParams {
    /// Length of one analysis frame.
    pub frame_ms: u32,
    /// A pause this long closes a segment that holds enough speech.
    pub close_after_silence_ms: u32,
    /// A pause closes a segment only once it holds this much speech.
    pub min_speech_to_close_ms: u32,
    /// A segment is cut once it reaches this length.
    pub max_segment_ms: u32,
    /// Audio kept from before the first speech frame.
    pub pre_roll_ms: u32,
    /// Audio kept after the last speech frame.
    pub tail_ms: u32,
    /// A segment with less speech than this is dropped.
    pub min_speech_ms: u32,
}

impl Default for SegmenterParams {
    fn default() -> Self {
        Self {
            frame_ms: 30,
            close_after_silence_ms: 800,
            min_speech_to_close_ms: 3_000,
            max_segment_ms: 30_000,
            pre_roll_ms: 300,
            tail_ms: 300,
            min_speech_ms: 250,
        }
    }
}

/// One piece of speech ready to transcribe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// 16 kHz mono samples.
    pub samples: Vec<i16>,
    /// How much of it is speech.
    pub speech_ms: u32,
}

#[derive(Debug)]
struct Frame {
    samples: Vec<i16>,
    db: f32,
    speech: bool,
}

#[derive(Debug, Default)]
struct OpenSegment {
    frames: Vec<Frame>,
    speech_frames: u32,
    last_speech: usize,
    silence_run: u32,
    skipping: bool,
}

impl OpenSegment {
    fn from_frames(frames: Vec<Frame>) -> Self {
        let speech_frames = frames.iter().filter(|f| f.speech).count() as u32;
        let last_speech = frames.iter().rposition(|f| f.speech).unwrap_or(0);
        let silence_run = frames.iter().rev().take_while(|f| !f.speech).count() as u32;
        Self {
            frames,
            speech_frames,
            last_speech,
            silence_run,
            skipping: false,
        }
    }

    fn push_speech(&mut self, frame: Frame) {
        self.frames.push(frame);
        self.speech_frames += 1;
        self.last_speech = self.frames.len() - 1;
        self.silence_run = 0;
    }
}

/// Turns a stream of samples into [`Segment`]s.
#[derive(Debug)]
pub struct Segmenter {
    params: SegmenterParams,
    frame_len: usize,
    close_frames: u32,
    min_close_speech_frames: u32,
    max_frames: usize,
    pre_roll_frames: usize,
    tail_frames: usize,
    cut_window_frames: usize,
    floor_rise_db: f32,
    pending: Vec<i16>,
    floor_db: Option<f32>,
    pre_roll: VecDeque<Frame>,
    open: Option<OpenSegment>,
    speech_frames_total: u32,
}

fn frames_for(ms: u32, frame_ms: u32) -> u32 {
    ms.div_ceil(frame_ms)
}

impl Segmenter {
    /// A segmenter with `params`; frames shorter than 10 ms are raised to 10.
    pub fn new(params: SegmenterParams) -> Self {
        let frame_ms = params.frame_ms.max(MIN_FRAME_MS);
        let params = SegmenterParams { frame_ms, ..params };
        Self {
            params,
            frame_len: samples_for_ms(frame_ms),
            close_frames: frames_for(params.close_after_silence_ms, frame_ms).max(1),
            min_close_speech_frames: frames_for(params.min_speech_to_close_ms, frame_ms),
            max_frames: frames_for(params.max_segment_ms, frame_ms).max(2) as usize,
            pre_roll_frames: frames_for(params.pre_roll_ms, frame_ms) as usize,
            tail_frames: frames_for(params.tail_ms, frame_ms) as usize,
            cut_window_frames: frames_for(CUT_WINDOW_MS, frame_ms).max(1) as usize,
            floor_rise_db: FLOOR_RISE_DB_PER_S * frame_ms as f32 / 1000.0,
            pending: Vec::new(),
            floor_db: None,
            pre_roll: VecDeque::new(),
            open: None,
            speech_frames_total: 0,
        }
    }

    /// Feeds `samples` and returns the segments they closed, in order.
    pub fn push(&mut self, samples: &[i16]) -> Vec<Segment> {
        let mut out = Vec::new();
        let mut rest = samples;
        if !self.pending.is_empty() {
            let take = (self.frame_len - self.pending.len()).min(rest.len());
            self.pending.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.pending.len() == self.frame_len {
                let frame = std::mem::take(&mut self.pending);
                self.process(frame, &mut out);
            }
        }
        for chunk in rest.chunks(self.frame_len) {
            if chunk.len() == self.frame_len {
                self.process(chunk.to_vec(), &mut out);
            } else {
                self.pending.extend_from_slice(chunk);
            }
        }
        out
    }

    /// Ends the stream and returns the speech still open, trimmed to its last
    /// speech frame and the tail.
    pub fn finish(mut self) -> Option<Segment> {
        let pending = std::mem::take(&mut self.pending);
        let mut open = self.open.take()?;
        if !pending.is_empty() {
            let frame = self.classify(pending);
            if frame.speech {
                if open.skipping {
                    open.frames.extend(self.pre_roll.drain(..));
                    open.skipping = false;
                }
                open.push_speech(frame);
            } else if !open.skipping {
                open.frames.push(frame);
            }
        }
        let end = (open.last_speech + 1 + self.tail_frames).min(open.frames.len());
        open.frames.truncate(end);
        self.segment_from(open.frames)
    }

    /// Whether the stream has held at least the minimum amount of speech.
    pub fn heard_speech(&self) -> bool {
        self.speech_frames_total * self.params.frame_ms >= self.params.min_speech_ms.max(1)
    }

    fn classify(&mut self, samples: Vec<i16>) -> Frame {
        let db = rms_dbfs(&samples);
        let floor = self.floor_db.unwrap_or(db.min(FLOOR_START_CAP_DBFS));
        let speech = db > ABS_MIN_DBFS.max(floor + MARGIN_DB);
        let next_floor = if db < floor {
            db
        } else {
            (floor + self.floor_rise_db).min(db)
        };
        self.floor_db = Some(next_floor);
        if speech {
            self.speech_frames_total += 1;
        }
        Frame {
            samples,
            db,
            speech,
        }
    }

    fn process(&mut self, samples: Vec<i16>, out: &mut Vec<Segment>) {
        let frame = self.classify(samples);
        let Some(mut open) = self.open.take() else {
            if frame.speech {
                let mut frames: Vec<Frame> = self.pre_roll.drain(..).collect();
                frames.push(frame);
                self.open = Some(OpenSegment::from_frames(frames));
            } else {
                self.remember(frame);
            }
            return;
        };

        if frame.speech {
            if open.skipping {
                open.frames.extend(self.pre_roll.drain(..));
                open.skipping = false;
            }
            open.push_speech(frame);
        } else {
            open.silence_run += 1;
            let can_close = open.speech_frames >= self.min_close_speech_frames;
            if !can_close && open.silence_run > self.close_frames {
                open.skipping = true;
                self.remember(frame);
                self.open = Some(open);
                return;
            }
            open.frames.push(frame);
            if can_close && open.silence_run >= self.close_frames {
                self.close(open, out);
                return;
            }
        }

        if open.frames.len() >= self.max_frames {
            self.cut(open, out);
        } else {
            self.open = Some(open);
        }
    }

    fn remember(&mut self, frame: Frame) {
        if self.pre_roll_frames == 0 {
            return;
        }
        self.pre_roll.push_back(frame);
        while self.pre_roll.len() > self.pre_roll_frames {
            self.pre_roll.pop_front();
        }
    }

    fn close(&mut self, mut open: OpenSegment, out: &mut Vec<Segment>) {
        let end = (open.last_speech + 1 + self.tail_frames).min(open.frames.len());
        let rest = open.frames.split_off(end);
        out.extend(self.segment_from(open.frames));
        for frame in rest {
            self.remember(frame);
        }
    }

    fn cut(&mut self, mut open: OpenSegment, out: &mut Vec<Segment>) {
        let len = open.frames.len();
        let start = len.saturating_sub(self.cut_window_frames).max(1);
        let at = (start..len)
            .min_by(|a, b| open.frames[*a].db.total_cmp(&open.frames[*b].db))
            .unwrap_or(len);
        let rest = open.frames.split_off(at);
        out.extend(self.segment_from(open.frames));
        let rest = OpenSegment::from_frames(rest);
        if rest.speech_frames == 0 {
            for frame in rest.frames {
                self.remember(frame);
            }
        } else {
            self.open = Some(rest);
        }
    }

    fn segment_from(&self, frames: Vec<Frame>) -> Option<Segment> {
        let speech_ms = frames.iter().filter(|f| f.speech).count() as u32 * self.params.frame_ms;
        if speech_ms < self.params.min_speech_ms || speech_ms == 0 {
            return None;
        }
        let samples = frames.into_iter().flat_map(|f| f.samples).collect();
        Some(Segment { samples, speech_ms })
    }
}
