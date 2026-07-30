pub mod panic_handler;

use std::path::Path;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

pub fn init_logging(logs_dir: &Path) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("info,writ_core=debug,writ_storage=debug,writ_tauri_lib=debug")
    });

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stdout).with_target(true))
        .with(file_layer(logs_dir))
        .init();
}

/// Builds the rolling-file layer, or `None` when the logs directory cannot
/// take a log file.
///
/// The convenience constructor (`rolling::daily`) panics in that case, which
/// aborted the process before Writ could report why — and the likeliest
/// reason for it is the same unwritable `~/.writ` that makes startup fail.
fn file_layer<S>(logs_dir: &Path) -> Option<impl Layer<S>>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    let appender = build_appender(logs_dir)?;
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);
    std::mem::forget(guard);
    Some(fmt::layer().with_writer(non_blocking).with_ansi(false))
}

/// Opens `writ.log.<date>` in `logs_dir`, returning `None` on any failure.
fn build_appender(logs_dir: &Path) -> Option<RollingFileAppender> {
    std::fs::create_dir_all(logs_dir).ok()?;
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("writ.log")
        .build(logs_dir)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::build_appender;

    #[test]
    fn appender_opens_a_writable_logs_dir_that_does_not_exist_yet() {
        let parent = tempfile::tempdir().unwrap();
        let logs = parent.path().join("logs");

        assert!(build_appender(&logs).is_some());
        assert!(logs.is_dir());
    }

    #[test]
    #[cfg(unix)]
    fn appender_returns_none_instead_of_panicking_on_an_unwritable_logs_dir() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().unwrap();
        let logs = parent.path().join("logs");
        std::fs::create_dir(&logs).unwrap();
        std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o555)).unwrap();

        assert!(build_appender(&logs).is_none());

        std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
