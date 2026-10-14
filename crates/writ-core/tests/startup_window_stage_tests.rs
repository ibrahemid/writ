//! A window that cannot be created is reported like any other startup step
//! (ADR-045): a missing WebView2 runtime or webkit2gtk shows a dialog that
//! says which one to install.

use writ_core::startup::{
    format_failure_dialog, format_failure_report, StartupFailure, StartupStage,
};

fn window_failure() -> StartupFailure {
    StartupFailure::new(
        StartupStage::Window,
        "failed to create webview: WebView2 runtime not found",
        None,
        "20261014-101500",
    )
}

#[test]
fn the_dialog_names_the_step_the_error_and_both_runtimes() {
    let dialog = format_failure_dialog(&window_failure(), None);

    assert!(dialog.starts_with("Step: opening the window and its web view\n"));
    assert!(dialog.contains("Error: failed to create webview: WebView2 runtime not found\n"));
    assert!(dialog.contains("Microsoft Edge WebView2 Runtime"));
    assert!(dialog.contains("webkit2gtk-4.1"));
    assert!(!dialog.contains("Path:"));
}

#[test]
fn the_report_carries_the_same_remedy() {
    let report = format_failure_report(&window_failure(), None);

    assert!(report.contains(StartupStage::Window.remedy()));
    assert!(report.contains("Time: 20261014-101500\n"));
}
