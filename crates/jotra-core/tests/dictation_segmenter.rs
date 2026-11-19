use jotra_core::dictation::{Segment, Segmenter, SegmenterParams};

const PER_MS: usize = 16;
const FRAME: usize = 30 * PER_MS;

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

/// A tone that drops to a murmur for 60 ms in every 400 ms, like the gaps
/// between syllables.
fn speech(ms: usize) -> Vec<i16> {
    (0..ms * PER_MS)
        .map(|i| {
            let t = i as f32 / 16_000.0;
            let amp = if (i / PER_MS) % 400 >= 340 { 0.03 } else { 0.3 };
            ((t * 220.0 * std::f32::consts::TAU).sin() * amp * f32::from(i16::MAX)) as i16
        })
        .collect()
}

/// Uniform noise at about -45 dBFS RMS from a fixed generator.
fn fan(ms: usize) -> Vec<i16> {
    let mut state: u32 = 0x2545_f491;
    (0..ms * PER_MS)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let unit = (state >> 8) as f32 / (1u32 << 24) as f32;
            ((unit * 2.0 - 1.0) * 319.0) as i16
        })
        .collect()
}

/// Zeros with dither of one bit: each sample -1, 0 or 1, about -92 dBFS RMS.
fn dither(ms: usize) -> Vec<i16> {
    let mut state: u32 = 0x9e37_79b9;
    (0..ms * PER_MS)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((state >> 16) % 3) as i16 - 1
        })
        .collect()
}

fn feed(segmenter: &mut Segmenter, parts: &[Vec<i16>]) -> Vec<Segment> {
    parts.iter().flat_map(|p| segmenter.push(p)).collect()
}

fn new() -> Segmenter {
    Segmenter::new(SegmenterParams::default())
}

#[test]
fn the_defaults_are_the_documented_tuning() {
    let p = SegmenterParams::default();
    assert_eq!(p.frame_ms, 30);
    assert_eq!(p.close_after_silence_ms, 800);
    assert_eq!(p.min_speech_to_close_ms, 3_000);
    assert_eq!(p.max_segment_ms, 30_000);
    assert_eq!(p.pre_roll_ms, 300);
    assert_eq!(p.tail_ms, 300);
    assert_eq!(p.min_speech_ms, 250);
}

#[test]
fn silence_alone_gives_nothing() {
    let mut s = new();
    assert!(feed(&mut s, &[silence(10_000)]).is_empty());
    assert!(!s.heard_speech());
    assert_eq!(s.finish(), None);
}

#[test]
fn a_pause_closes_four_seconds_of_speech_after_about_800_ms() {
    let mut s = new();
    // Ends on a voiced frame: the last 60 ms of each 400 are a murmur.
    assert!(feed(&mut s, &[silence(990), speech(3_940)]).is_empty());
    let mut closed_at = None;
    let mut segments = Vec::new();
    for ms in 1..=1_000 {
        let out = s.push(&silence(1));
        if !out.is_empty() && closed_at.is_none() {
            closed_at = Some(ms);
        }
        segments.extend(out);
    }
    let closed_at = closed_at.expect("the pause closes the segment");
    assert!(
        (780..=850).contains(&closed_at),
        "closed after {closed_at} ms"
    );
    assert_eq!(segments.len(), 1);
    let len_ms = segments[0].samples.len() / PER_MS;
    assert!((4_300..=4_700).contains(&len_ms), "{len_ms} ms");
    assert!(segments[0].speech_ms >= 3_000);
    assert!(s.heard_speech());
    assert_eq!(s.finish(), None);
}

#[test]
fn the_pre_roll_and_tail_are_kept() {
    let mut s = new();
    let out = feed(&mut s, &[silence(990), tone(3_990), silence(2_000)]);
    assert_eq!(out.len(), 1);
    let samples = &out[0].samples;
    assert_eq!(samples.len(), (10 + 133 + 10) * FRAME);
    assert!(samples[..10 * FRAME].iter().all(|v| *v == 0));
    assert!(samples[10 * FRAME..11 * FRAME].iter().any(|v| *v != 0));
    assert!(samples[samples.len() - 10 * FRAME..]
        .iter()
        .all(|v| *v == 0));
    assert_eq!(out[0].speech_ms, 133 * 30);
}

#[test]
fn two_short_phrases_with_a_pause_stay_one_segment() {
    let mut s = new();
    let out = feed(
        &mut s,
        &[
            silence(990),
            tone(1_980),
            silence(1_500),
            tone(1_980),
            silence(2_000),
        ],
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].speech_ms, 132 * 30);
    assert_eq!(s.finish(), None);
}

#[test]
fn a_long_pause_after_short_speech_is_not_kept_whole() {
    let mut s = new();
    let out = feed(
        &mut s,
        &[
            silence(990),
            tone(1_980),
            silence(20_000),
            tone(1_980),
            silence(2_000),
        ],
    );
    assert_eq!(out.len(), 1);
    let len_ms = out[0].samples.len() / PER_MS;
    assert!(len_ms < 6_000, "{len_ms} ms kept");
}

#[test]
fn continuous_speech_is_cut_at_its_quietest_frame_within_30_s() {
    let mut s = new();
    let mut audio = speech(45_000);
    // One frame of digital silence 27 s in, on a frame boundary.
    let quiet = 27_000 * PER_MS;
    audio[quiet..quiet + FRAME].fill(0);
    let mut out = feed(&mut s, &[silence(990), audio, silence(2_000)]);
    out.extend(s.finish());
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].samples.len(), (10 + 900) * FRAME);
    for segment in &out {
        assert!(segment.samples.len() <= 30_000 * PER_MS);
    }
    let total: usize = out.iter().map(|seg| seg.samples.len()).sum();
    assert!(total >= 45_000 * PER_MS);
}

#[test]
fn finish_returns_the_open_speech() {
    let mut s = new();
    assert!(feed(&mut s, &[silence(990), tone(1_980), silence(300)]).is_empty());
    let segment = s.finish().expect("open speech");
    assert_eq!(segment.samples.len(), (10 + 66 + 10) * FRAME);
    assert_eq!(segment.speech_ms, 66 * 30);
}

#[test]
fn finish_keeps_a_trailing_partial_frame_of_speech() {
    let mut s = new();
    assert!(feed(&mut s, &[silence(990), tone(1_995)]).is_empty());
    let segment = s.finish().expect("open speech");
    assert_eq!(segment.samples.len(), 10 * FRAME + 1_995 * PER_MS);
}

#[test]
fn steady_fan_noise_is_not_speech() {
    let mut s = new();
    assert!(feed(&mut s, &[fan(10_000)]).is_empty());
    assert!(!s.heard_speech());
    assert_eq!(s.finish(), None);
}

#[test]
fn digital_silence_before_fan_noise_does_not_make_the_fan_speech() {
    let mut s = new();
    assert!(feed(&mut s, &[silence(60), fan(10_000)]).is_empty());
    assert!(!s.heard_speech());
    assert_eq!(s.finish(), None);
}

#[test]
fn digital_silence_mid_stream_does_not_make_the_fan_speech() {
    let mut s = new();
    assert!(feed(&mut s, &[fan(2_000), silence(90), fan(10_000)]).is_empty());
    assert!(!s.heard_speech());
    assert_eq!(s.finish(), None);
}

#[test]
fn dithered_silence_before_fan_noise_does_not_make_the_fan_speech() {
    let mut s = new();
    assert!(feed(&mut s, &[dither(60), fan(10_000)]).is_empty());
    assert!(!s.heard_speech());
    assert_eq!(s.finish(), None);
}

#[test]
fn dithered_silence_mid_stream_does_not_make_the_fan_speech() {
    let mut s = new();
    assert!(feed(&mut s, &[fan(2_000), dither(90), fan(10_000)]).is_empty());
    assert!(!s.heard_speech());
    assert_eq!(s.finish(), None);
}

#[test]
fn speech_after_dithered_silence_is_found() {
    let mut s = new();
    let out = feed(&mut s, &[dither(990), speech(4_000), dither(2_000)]);
    assert_eq!(out.len(), 1);
    assert!(out[0].speech_ms >= 3_000);
}

#[test]
fn speech_over_fan_noise_is_found() {
    let mut s = new();
    let mut voiced = speech(4_000);
    for (v, n) in voiced.iter_mut().zip(fan(4_000)) {
        *v = v.saturating_add(n);
    }
    let out = feed(&mut s, &[fan(2_000), voiced, fan(2_000)]);
    assert_eq!(out.len(), 1);
    assert!(out[0].speech_ms >= 3_000);
}

#[test]
fn a_blip_under_250_ms_is_dropped() {
    let mut s = new();
    assert!(feed(&mut s, &[silence(990), tone(150), silence(3_000)]).is_empty());
    assert!(!s.heard_speech());
    assert_eq!(s.finish(), None);
}

#[test]
fn the_chunk_size_does_not_change_the_segments() {
    let audio: Vec<i16> = [silence(990), speech(4_000), silence(1_500), speech(2_000)].concat();
    let mut whole = new();
    let mut whole_out = whole.push(&audio);
    whole_out.extend(whole.finish());

    let mut chunked = new();
    let mut chunked_out: Vec<Segment> = audio.chunks(7).flat_map(|c| chunked.push(c)).collect();
    chunked_out.extend(chunked.finish());

    assert_eq!(whole_out, chunked_out);
    assert_eq!(whole_out.len(), 2);
}
