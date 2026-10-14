//! The panic hook keeps the panic's message out of the crash file and the log
//! (ADR-045). One test in its own binary: the hook is process-wide.

use std::io::Write;
use std::sync::{Arc, Mutex};

use writ_core::crash::DIGEST_PREFIX_LEN;
use writ_core::hash::sha256_hex;
use writ_tauri_lib::logging::panic_handler::install_panic_handler;

const SENTINEL: &str = "SENTINEL-note-text-9f2c41";

#[derive(Clone, Default)]
struct CapturedLog(Arc<Mutex<Vec<u8>>>);

impl Write for CapturedLog {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn panic_quoting_the_sentinel() -> usize {
    panic!("{SENTINEL}")
}

#[test]
fn a_panic_message_never_reaches_the_crash_file_or_the_log() {
    let logs = tempfile::tempdir().unwrap();
    let log = CapturedLog::default();
    let writer = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();

    let previous = std::panic::take_hook();
    install_panic_handler(logs.path());
    let note = format!("é{SENTINEL}");
    let outcomes = tracing::subscriber::with_default(subscriber, || {
        [
            // A slicing panic quotes the string it sliced: the case the hook exists for.
            std::panic::catch_unwind(|| std::hint::black_box(&note[0..1]).len()),
            std::panic::catch_unwind(panic_quoting_the_sentinel),
            // The same panic again, as a contained renderer would repeat it.
            std::panic::catch_unwind(panic_quoting_the_sentinel),
        ]
    });
    std::panic::set_hook(previous);
    assert!(outcomes.iter().all(Result::is_err));

    let files: Vec<_> = std::fs::read_dir(logs.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(files.len(), 2, "one file per distinct panic: {files:?}");
    for file in &files {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("writ-crash-") && name.ends_with(".log"),
            "{name}"
        );
        let report = std::fs::read_to_string(file).unwrap();
        assert!(!report.contains(SENTINEL), "{report}");
        assert!(report.contains("Location: "), "{report}");
        assert!(report.contains("Payload: String message, "), "{report}");
        assert!(report.contains(
            "Anchor: writ_tauri_lib::logging::panic_handler::install_panic_handler at 0x"
        ));
        assert!(report.contains("Backtrace:\n"), "{report}");
    }

    let logged = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    assert!(!logged.contains(SENTINEL), "{logged}");
    assert_eq!(logged.matches("panic at ").count(), 3, "{logged}");
    assert!(
        logged.contains(&format!(
            "{} bytes, sha256 {}",
            SENTINEL.len(),
            &sha256_hex(SENTINEL.as_bytes())[..DIGEST_PREFIX_LEN]
        )),
        "{logged}"
    );
}
