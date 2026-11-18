//! Records from the real default input device. Run with `--ignored` on a
//! machine with a microphone; on macOS the first run asks for microphone
//! access, and a refused prompt yields frames of silence.

use std::time::Duration;

use jotra_audio::{Recorder, FRAME_SAMPLES};

#[test]
#[ignore = "needs a microphone"]
fn one_second_from_the_default_device_arrives_as_20_ms_frames() {
    let (recorder, mut frames) = Recorder::start().expect("the default input opens");
    std::thread::sleep(Duration::from_secs(1));
    recorder.stop();
    let mut received = Vec::new();
    while let Some(frame) = frames.blocking_recv() {
        received.push(frame);
    }
    let samples: usize = received.iter().map(|f| f.samples.len()).sum();
    assert!(samples >= 12_000, "{samples} samples in one second");
    assert!(samples <= 24_000, "{samples} samples in one second");
    let (last, full) = received.split_last().expect("frames arrived");
    assert!(full.iter().all(|f| f.samples.len() == FRAME_SAMPLES));
    assert!(last.samples.len() <= FRAME_SAMPLES);
    assert!(received.iter().all(|f| (0.0..=1.0).contains(&f.level)));
}
