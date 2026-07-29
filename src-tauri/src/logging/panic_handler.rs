use std::path::Path;
use tracing::error;

pub fn install_panic_handler(logs_dir: &Path) {
    let crash_dir = logs_dir.to_path_buf();
    std::panic::set_hook(Box::new(move |panic_info| {
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");

        let message = format!(
            "Writ crashed!\n\nTimestamp: {}\nPanic: {}\nLocation: {:?}\n",
            timestamp,
            panic_info,
            panic_info.location(),
        );

        error!("{}", message);

        // The logs directory lives inside the data directory, so a crash
        // caused by that directory being unwritable would otherwise leave
        // no record at all. Written as two attempts rather than a probe:
        // the hook runs while the process is already unwinding, so it does
        // as little as it can get away with.
        let file_name = format!("writ-crash-{}.log", timestamp);
        if std::fs::write(crash_dir.join(&file_name), &message).is_err() {
            std::fs::write(std::env::temp_dir().join(&file_name), &message).ok();
        }
    }));
}
