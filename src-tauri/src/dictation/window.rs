//! What the main window ends: hiding, minimizing or closing it stops the live
//! session, and a page load in it cancels the session the old page started.
//!
//! The window's own events carry no hide or minimize: on macOS both arrive
//! only as a loss of focus, elsewhere also as a resize, so those events read
//! the window's state before stopping.

use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, Runtime, WindowEvent};

use super::DictationState;

/// The label of the window dictation inserts into.
pub const MAIN_WINDOW: &str = "main";

/// What a main-window event says about whether the window can still be seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sight {
    /// The window is closing or closed.
    Gone,
    /// The window may have been hidden or minimized; its state says.
    Check,
    /// The event does not change whether the window is seen.
    Unchanged,
}

/// What `event` says about the window's visibility.
pub fn sight_after(event: &WindowEvent) -> Sight {
    match event {
        WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed => Sight::Gone,
        WindowEvent::Focused(false) | WindowEvent::Resized(_) => Sight::Check,
        _ => Sight::Unchanged,
    }
}

/// Whether a window with this state is off the screen. A state that could not
/// be read counts as on it.
pub fn is_out_of_sight(visible: Option<bool>, minimized: Option<bool>) -> bool {
    visible == Some(false) || minimized == Some(true)
}

/// Whether a page load in webview `label` ends the session its old page
/// started.
pub fn page_load_ends_session(label: &str, event: PageLoadEvent) -> bool {
    label == MAIN_WINDOW && event == PageLoadEvent::Started
}

/// Stops the live session when the main window has been hidden, minimized or
/// closed. Returns the stopped session's id.
pub fn stop_when_out_of_sight<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    event: &WindowEvent,
) -> Option<u32> {
    if label != MAIN_WINDOW {
        return None;
    }
    let sight = sight_after(event);
    if sight == Sight::Unchanged {
        return None;
    }
    let state = app.state::<DictationState>();
    if !state.is_busy() {
        return None;
    }
    let gone = sight == Sight::Gone
        || app.get_webview_window(MAIN_WINDOW).is_none_or(|window| {
            is_out_of_sight(window.is_visible().ok(), window.is_minimized().ok())
        });
    if !gone {
        return None;
    }
    let id = state.stop_live()?;
    tracing::info!(
        session = id,
        "dictation: the window left the screen, stopping"
    );
    Some(id)
}

/// Cancels the live session when the main window starts loading a page: the
/// page that started it is gone and nothing would insert its text.
pub fn cancel_on_page_load<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    event: PageLoadEvent,
) -> Option<u32> {
    if !page_load_ends_session(label, event) {
        return None;
    }
    let id = app.state::<DictationState>().cancel_live()?;
    tracing::info!(
        session = id,
        "dictation: the window's page reloaded, cancelling"
    );
    Some(id)
}

#[cfg(test)]
mod tests {
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
    use tauri::PhysicalSize;

    use super::*;

    fn app_with_window(label: &str) -> tauri::App<MockRuntime> {
        let app = mock_builder()
            .manage(DictationState::default())
            .build(mock_context(noop_assets()))
            .expect("mock app");
        tauri::WebviewWindowBuilder::new(&app, label, Default::default())
            .build()
            .expect("window");
        app
    }

    #[test]
    fn closing_counts_as_gone_and_focus_or_size_needs_a_check() {
        assert_eq!(sight_after(&WindowEvent::Destroyed), Sight::Gone);
        assert_eq!(sight_after(&WindowEvent::Focused(false)), Sight::Check);
        assert_eq!(
            sight_after(&WindowEvent::Resized(PhysicalSize::new(0, 0))),
            Sight::Check
        );
        assert_eq!(sight_after(&WindowEvent::Focused(true)), Sight::Unchanged);
        assert_eq!(
            sight_after(&WindowEvent::Moved(tauri::PhysicalPosition::new(4, 4))),
            Sight::Unchanged
        );
    }

    #[test]
    fn a_hidden_or_minimized_window_is_out_of_sight() {
        assert!(is_out_of_sight(Some(false), Some(false)));
        assert!(is_out_of_sight(Some(true), Some(true)));
        assert!(!is_out_of_sight(Some(true), Some(false)));
        assert!(!is_out_of_sight(None, None), "an unread state is on screen");
    }

    #[test]
    fn only_a_page_starting_in_the_main_window_ends_the_session() {
        assert!(page_load_ends_session("main", PageLoadEvent::Started));
        assert!(!page_load_ends_session("main", PageLoadEvent::Finished));
        assert!(!page_load_ends_session("capture", PageLoadEvent::Started));
    }

    #[test]
    fn closing_the_main_window_stops_the_live_session() {
        let app = app_with_window(MAIN_WINDOW);
        let state = app.state::<DictationState>();
        let reservation = state.reserve().expect("the slot is free");
        let id = reservation.id();
        let controls = reservation.controls();
        reservation.keep();

        assert_eq!(
            stop_when_out_of_sight(app.handle(), "capture", &WindowEvent::Destroyed),
            None
        );
        assert_eq!(
            stop_when_out_of_sight(app.handle(), MAIN_WINDOW, &WindowEvent::Focused(false)),
            None,
            "a window still on screen keeps the session"
        );
        assert!(!controls.stop.is_cancelled());

        assert_eq!(
            stop_when_out_of_sight(app.handle(), MAIN_WINDOW, &WindowEvent::Destroyed),
            Some(id)
        );
        assert!(controls.stop.is_cancelled());
        assert!(!controls.cancel.is_cancelled(), "a stop, not a cancel");
    }

    #[test]
    fn a_page_load_in_the_main_window_cancels_a_start_in_progress() {
        let app = app_with_window(MAIN_WINDOW);
        let state = app.state::<DictationState>();
        let reservation = state.reserve().expect("the slot is free");
        let controls = reservation.controls();

        assert_eq!(
            cancel_on_page_load(app.handle(), "capture", PageLoadEvent::Started),
            None
        );
        assert!(!controls.cancel.is_cancelled());
        assert_eq!(
            cancel_on_page_load(app.handle(), MAIN_WINDOW, PageLoadEvent::Started),
            Some(reservation.id())
        );
        assert!(controls.cancel.is_cancelled());
    }
}
