use std::path::Path;
use tracing::error;

pub fn install_panic_handler(logs_dir: &Path) {
    let crash_dir = logs_dir.to_path_buf();
    std::panic::set_hook(Box::new(move |panic_info| {
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let crash_path = crash_dir.join(format!("crash-{}.log", timestamp));

        let message = format!(
            "Writ crashed!\n\nTimestamp: {}\nPanic: {}\nLocation: {:?}\n",
            timestamp,
            panic_info,
            panic_info.location(),
        );

        error!("{}", message);
        std::fs::write(&crash_path, &message).ok();
    }));
}
