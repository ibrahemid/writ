use jotra_audio::{downmix, AudioFrame, CaptureError, Converter, Framer, FRAME_SAMPLES};

fn sine(rate: u32, channels: u16, seconds: f32, hz: f32, amp: f32) -> Vec<f32> {
    let frames = (rate as f32 * seconds) as usize;
    let mut out = Vec::with_capacity(frames * usize::from(channels));
    for i in 0..frames {
        let v = (i as f32 / rate as f32 * hz * std::f32::consts::TAU).sin() * amp;
        for _ in 0..channels {
            out.push(v);
        }
    }
    out
}

/// Feeds `input` in device-sized buffers of `buffer_frames` and ends the
/// stream.
fn convert(rate: u32, channels: u16, input: &[f32], buffer_frames: usize) -> Vec<i16> {
    let mut converter = Converter::new(rate, channels).unwrap();
    let mut out = Vec::new();
    for chunk in input.chunks(buffer_frames * usize::from(channels)) {
        out.extend(converter.push(chunk));
    }
    out.extend(converter.finish());
    out
}

fn peak(samples: &[i16]) -> f32 {
    samples
        .iter()
        .map(|s| f32::from(*s).abs())
        .fold(0.0, f32::max)
        / f32::from(i16::MAX)
}

#[test]
fn stereo_48_khz_comes_out_as_16_khz_mono_at_the_same_level() {
    let input = sine(48_000, 2, 1.0, 440.0, 0.5);
    let out = convert(48_000, 2, &input, 512);
    assert!(out.len().abs_diff(16_000) <= FRAME_SAMPLES, "{}", out.len());
    let middle = &out[4_000..12_000];
    let level = peak(middle);
    assert!((level - 0.5).abs() < 0.03, "peak {level}");
}

#[test]
fn mono_44_1_khz_comes_out_as_16_khz_at_the_same_level() {
    let input = sine(44_100, 1, 1.0, 300.0, 0.8);
    let out = convert(44_100, 1, &input, 441);
    assert!(out.len().abs_diff(16_000) <= FRAME_SAMPLES, "{}", out.len());
    let level = peak(&out[4_000..12_000]);
    assert!((level - 0.8).abs() < 0.04, "peak {level}");
}

#[test]
fn the_buffer_size_does_not_change_the_length() {
    let input = sine(48_000, 1, 0.5, 440.0, 0.5);
    let a = convert(48_000, 1, &input, 128);
    let b = convert(48_000, 1, &input, 4_096);
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), 8_000);
}

#[test]
fn audio_at_16_khz_passes_through() {
    let mut converter = Converter::new(16_000, 1).unwrap();
    assert_eq!(converter.push(&[0.0, 0.5, -0.5]), vec![0, 16_384, -16_384]);
    assert!(converter.finish().is_empty());
}

#[test]
fn samples_past_full_scale_are_clamped() {
    let mut converter = Converter::new(16_000, 1).unwrap();
    assert_eq!(
        converter.push(&[1.5, -2.0, 1.0, -1.0, f32::NAN]),
        vec![i16::MAX, -i16::MAX, i16::MAX, -i16::MAX, 0]
    );
}

#[test]
fn downmix_averages_each_frame() {
    assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5, 0.25], 2), vec![0.5, 0.5]);
    assert_eq!(downmix(&[0.3, 0.6, 0.9], 3), vec![0.6]);
    assert_eq!(downmix(&[0.1, 0.2], 1), vec![0.1, 0.2]);
    assert_eq!(downmix(&[0.1, 0.2], 0), vec![0.1, 0.2]);
}

#[test]
fn a_device_without_a_rate_or_channels_is_refused() {
    assert!(matches!(
        Converter::new(0, 1),
        Err(CaptureError::Unsupported(_))
    ));
    assert!(matches!(
        Converter::new(48_000, 0),
        Err(CaptureError::Unsupported(_))
    ));
}

#[test]
fn the_framer_cuts_20_ms_frames_and_keeps_the_rest() {
    let mut framer = Framer::default();
    assert!(framer.push(&[1_000; 300]).is_empty());
    let frames = framer.push(&[1_000; 400]);
    assert_eq!(frames.len(), 2);
    assert!(frames.iter().all(|f| f.samples.len() == FRAME_SAMPLES));
    assert!(frames[0].level > 0.0);
    let last = framer.finish().unwrap();
    assert_eq!(last.samples.len(), 700 - 2 * FRAME_SAMPLES);
    assert!(Framer::default().finish().is_none());
}

#[test]
fn a_silent_frame_has_no_level() {
    assert_eq!(AudioFrame::from_samples(vec![0; FRAME_SAMPLES]).level, 0.0);
    let loud = AudioFrame::from_samples(vec![i16::MAX; FRAME_SAMPLES]);
    assert!(loud.level > 0.99);
}
