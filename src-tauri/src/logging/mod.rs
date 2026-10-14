pub mod panic_handler;

use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Daily log files kept in the logs folder; older ones are deleted when a new
/// one opens (ADR-045).
const MAX_LOG_FILES: usize = 14;

/// What a release build logs. Debug lines name notes and folders, so they
/// stay behind `RUST_LOG`.
const RELEASE_DIRECTIVES: &str = "info";

/// What a debug build logs.
const DEBUG_DIRECTIVES: &str = "info,writ_core=debug,writ_storage=debug,writ_tauri_lib=debug";

/// Holds the log file writer open. Dropping it, or calling [`LogGuard::flush`],
/// writes the lines still queued.
///
/// The event loop ends the process with `exit`, which runs no destructors, so
/// the run loop calls `flush` on its way out.
pub struct LogGuard(Option<WorkerGuard>);

impl LogGuard {
    /// Writes the queued lines and closes the file writer. Lines logged after
    /// this are dropped.
    pub fn flush(&mut self) {
        self.0.take();
    }
}

pub fn init_logging(logs_dir: &Path) -> LogGuard {
    let directives = filter_directives(
        std::env::var("RUST_LOG").ok().as_deref(),
        !cfg!(debug_assertions),
    );
    let env_filter = EnvFilter::try_new(&directives)
        .unwrap_or_else(|_| EnvFilter::new(default_directives(!cfg!(debug_assertions))));

    let (file_layer, guard) = match file_layer(logs_dir) {
        Some((layer, guard)) => (Some(layer), Some(guard)),
        None => (None, None),
    };

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stdout).with_target(true))
        .with(file_layer)
        .init();

    LogGuard(guard)
}

/// The filter a build starts with when `RUST_LOG` is unset.
fn default_directives(release: bool) -> &'static str {
    if release {
        RELEASE_DIRECTIVES
    } else {
        DEBUG_DIRECTIVES
    }
}

/// The filter to install: `RUST_LOG` when it is set and not blank, otherwise
/// the build's default.
fn filter_directives(env: Option<&str>, release: bool) -> String {
    match env.map(str::trim) {
        Some(directives) if !directives.is_empty() => directives.to_string(),
        _ => default_directives(release).to_string(),
    }
}

/// Builds the rolling-file layer and its guard, or `None` when the logs
/// directory cannot take a log file.
///
/// The convenience constructor (`rolling::daily`) panics in that case, which
/// aborted the process before Writ could report why — and the likeliest
/// reason for it is the same unwritable `~/.writ` that makes startup fail.
fn file_layer<S>(logs_dir: &Path) -> Option<(impl Layer<S>, WorkerGuard)>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    let appender = build_appender(logs_dir)?;
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);
    Some((
        fmt::layer().with_writer(non_blocking).with_ansi(false),
        guard,
    ))
}

/// Opens `writ.log.<date>` in `logs_dir`, deleting the oldest files past
/// [`MAX_LOG_FILES`]. Returns `None` on any failure.
fn build_appender(logs_dir: &Path) -> Option<RollingFileAppender> {
    std::fs::create_dir_all(logs_dir).ok()?;
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("writ.log")
        .max_log_files(MAX_LOG_FILES)
        .build(logs_dir)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tracing::level_filters::LevelFilter;

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

    #[test]
    fn a_release_build_logs_at_info_and_nothing_finer() {
        let filter = EnvFilter::new(filter_directives(None, true));

        assert_eq!(filter.max_level_hint(), Some(LevelFilter::INFO));
    }

    #[test]
    fn a_debug_build_logs_writs_own_crates_at_debug() {
        let filter = EnvFilter::new(filter_directives(None, false));

        assert_eq!(filter.max_level_hint(), Some(LevelFilter::DEBUG));
    }

    #[test]
    fn rust_log_turns_debug_on_in_a_release_build() {
        assert_eq!(
            filter_directives(Some("writ_tauri_lib=debug"), true),
            "writ_tauri_lib=debug"
        );
        assert_eq!(filter_directives(Some("  "), true), RELEASE_DIRECTIVES);
    }

    #[test]
    fn the_logs_folder_keeps_the_newest_fourteen_log_files() {
        let logs = tempfile::tempdir().unwrap();
        // Pruning orders by creation time; the pause keeps the order strict.
        for day in 1..=20 {
            std::fs::write(
                logs.path().join(format!("writ.log.2000-01-{day:02}")),
                "old\n",
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        std::fs::write(
            logs.path().join("writ-crash-20000101-000000.log"),
            "report\n",
        )
        .unwrap();

        let _appender = build_appender(logs.path()).expect("appender");

        let log_files = std::fs::read_dir(logs.path())
            .unwrap()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| name.starts_with("writ.log"))
            .count();
        assert_eq!(log_files, MAX_LOG_FILES);
        assert!(!logs.path().join("writ.log.2000-01-01").exists());
        assert!(logs.path().join("writ.log.2000-01-20").exists());
        assert!(logs.path().join("writ-crash-20000101-000000.log").exists());
    }

    #[test]
    fn flushing_the_guard_writes_the_queued_lines() {
        let logs = tempfile::tempdir().unwrap();
        let (mut writer, guard) =
            tracing_appender::non_blocking(build_appender(logs.path()).unwrap());
        let mut guard = LogGuard(Some(guard));

        writer.write_all(b"the last line before quit\n").unwrap();
        guard.flush();

        let written: String = std::fs::read_dir(logs.path())
            .unwrap()
            .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
            .collect();
        assert!(written.contains("the last line before quit"));
    }
}
