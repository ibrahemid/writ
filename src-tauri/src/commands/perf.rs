use serde::Deserialize;
use tracing::info;

/// Which show the first paint measured.
///
/// Typed so the webview cannot hand the log a string of its own: anything
/// else fails to deserialize and nothing is logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FirstPaintMode {
    /// The first show after launch.
    Cold,
    /// A show of a window that was hidden.
    Warm,
}

impl FirstPaintMode {
    fn label(self) -> &'static str {
        match self {
            FirstPaintMode::Cold => "cold",
            FirstPaintMode::Warm => "warm",
        }
    }
}

#[tauri::command]
pub fn report_first_paint(elapsed_ms: f64, mode: FirstPaintMode, rust_elapsed_us: Option<u64>) {
    if !is_plausible_elapsed(elapsed_ms) {
        return;
    }
    info!(
        mode = mode.label(),
        elapsed_ms = elapsed_ms,
        rust_elapsed_us = rust_elapsed_us.unwrap_or(0),
        "toggle-latency first paint"
    );
}

fn is_plausible_elapsed(elapsed_ms: f64) -> bool {
    elapsed_ms.is_finite() && elapsed_ms >= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_modes_the_frontend_sends_deserialize() {
        assert_eq!(
            serde_json::from_str::<FirstPaintMode>("\"cold\"").unwrap(),
            FirstPaintMode::Cold
        );
        assert_eq!(
            serde_json::from_str::<FirstPaintMode>("\"warm\"").unwrap(),
            FirstPaintMode::Warm
        );
    }

    #[test]
    fn any_other_mode_string_is_refused() {
        for forged in [
            "\"cold\\n2026-10-14T00:00:00Z ERROR writ_tauri_lib: forged\"",
            "\"Cold\"",
            "\"\"",
        ] {
            assert!(
                serde_json::from_str::<FirstPaintMode>(forged).is_err(),
                "{forged}"
            );
        }
    }

    #[test]
    fn an_elapsed_time_that_is_not_a_duration_is_not_logged() {
        assert!(is_plausible_elapsed(12.5));
        assert!(is_plausible_elapsed(0.0));
        assert!(!is_plausible_elapsed(f64::NAN));
        assert!(!is_plausible_elapsed(f64::INFINITY));
        assert!(!is_plausible_elapsed(-1.0));
    }
}
