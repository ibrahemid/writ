use std::path::Path;
use std::sync::{Mutex, PoisonError};

use tracing::error;
use writ_core::crash::{
    crash_report_file_name, format_crash_report, AddressAnchor, CrashFiles, PanicSummary,
};

/// Installs the hook that records a panic without its message (ADR-045).
///
/// Every panic gets one line in the rolling log. The first panic at each
/// location with each message digest also gets a crash file in `logs_dir`
/// ([`CrashFiles`]), with a backtrace whose frame addresses map back to the
/// build's symbol files through the anchor. With renderers contained, a
/// document that trips one would otherwise write a file on every render.
///
/// The file goes to `logs_dir` or nowhere. The shared temporary directory is
/// readable by other accounts on a multi-user machine.
pub fn install_panic_handler(logs_dir: &Path) {
    let crash_dir = logs_dir.to_path_buf();
    let crash_files = Mutex::new(CrashFiles::default());
    std::panic::set_hook(Box::new(move |info| {
        let summary = PanicSummary::new(
            info.payload(),
            info.location().map(ToString::to_string),
            std::thread::current().name().map(str::to_owned),
        );
        error!("{}", summary.log_line());

        let admitted = crash_files
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .admit(&summary);
        let Some(sequence) = admitted else {
            return;
        };
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let backtrace = std::backtrace::Backtrace::force_capture();
        let report = format_crash_report(
            &summary,
            &timestamp,
            address_anchor(),
            &format!("{backtrace:#}"),
        );
        let file = crash_dir.join(crash_report_file_name(&timestamp, sequence));
        let _ = std::fs::write(file, report);
    }));
}

fn address_anchor() -> AddressAnchor {
    AddressAnchor {
        symbol: "writ_tauri_lib::logging::panic_handler::install_panic_handler",
        address: install_panic_handler as fn(&Path) as usize,
    }
}
