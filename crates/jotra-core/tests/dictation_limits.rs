use jotra_core::dictation::{
    samples_for_ms, DictationEvent, QuietWatch, SessionLimits, SpeechDetector, QUEUE_LIMIT_MS,
    QUIET_STOP_MS,
};

const PER_MS: usize = 16;

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

#[test]
fn the_limits_are_a_minute_of_quiet_and_five_minutes_of_queue() {
    assert_eq!(QUIET_STOP_MS, 60_000);
    assert_eq!(QUEUE_LIMIT_MS, 300_000);
    assert_eq!(
        SessionLimits::default(),
        SessionLimits {
            quiet_stop_ms: 60_000,
            queue_ms: 300_000,
        }
    );
}

#[test]
fn the_queue_is_full_at_its_limit_of_audio() {
    let limits = SessionLimits::default();
    assert!(!limits.is_queue_full(samples_for_ms(300_000) - 1));
    assert!(limits.is_queue_full(samples_for_ms(300_000)));
    assert!(limits.is_queue_full(samples_for_ms(400_000)));
}

#[test]
fn a_minute_of_silence_fires_the_quiet_watch() {
    let mut watch = QuietWatch::new(60_000);
    assert!(!watch.push(&silence(59_970)));
    assert!(watch.push(&silence(30)));
}

#[test]
fn steady_fan_noise_counts_as_quiet() {
    let mut watch = SessionLimits::default().quiet_watch();
    assert!(!watch.push(&fan(30_000)));
    assert!(watch.push(&fan(30_000)));
}

#[test]
fn speech_starts_the_minute_again() {
    let mut watch = QuietWatch::new(60_000);
    assert!(!watch.push(&silence(50_000)));
    assert!(!watch.push(&tone(1_000)));
    assert!(!watch.push(&silence(50_000)));
    assert!(watch.push(&silence(10_000)));
}

#[test]
fn the_chunk_size_does_not_change_when_the_watch_fires() {
    let audio: Vec<i16> = [tone(500), silence(2_000)].concat();
    let fired_at = |chunk: usize| {
        let mut watch = QuietWatch::new(1_000);
        let mut fed = 0;
        for part in audio.chunks(chunk) {
            fed += part.len();
            if watch.push(part) {
                return Some(fed / (30 * PER_MS));
            }
        }
        None
    };
    let whole_frames = fired_at(30 * PER_MS);
    assert!(whole_frames.is_some());
    assert_eq!(fired_at(7), whole_frames);
    assert_eq!(fired_at(13), whole_frames);
}

#[test]
fn a_tone_after_digital_silence_is_speech() {
    let mut detector = SpeechDetector::new(30);
    assert!(!detector.classify(&silence(30)).1);
    assert!(detector.classify(&tone(30)).1);
}

#[test]
fn the_stop_notices_name_why_the_session_stopped() {
    assert_eq!(
        DictationEvent::stopped_after_silence(),
        DictationEvent::Notice {
            message: "Dictation stopped after a minute of silence.".to_string(),
        }
    );
    assert_eq!(
        DictationEvent::stopped_service_behind(),
        DictationEvent::Notice {
            message: "Dictation stopped because the transcription service could not keep up."
                .to_string(),
        }
    );
    assert!(!DictationEvent::stopped_after_silence().is_terminal());
    assert!(!DictationEvent::stopped_service_behind().is_terminal());
}
