pub mod panic_handler;

use std::path::Path;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub fn init_logging(logs_dir: &Path) {
    std::fs::create_dir_all(logs_dir).ok();

    let file_appender = rolling::daily(logs_dir, "writ.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    std::mem::forget(guard);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("info,writ_core=debug,writ_storage=debug,writ_tauri_lib=debug")
    });

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stdout).with_target(true))
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
        .init();
}
