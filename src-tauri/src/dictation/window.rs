//! What the main window ends: hiding, minimizing or closing it stops the live
//! session, and a page load in it cancels the session the old page started.
//! A window that is only covered, on another Space or unfocused keeps it.
//!
//! The window's own events carry no hide or minimize: on macOS both arrive
//! only as a loss of focus, elsewhere also as a resize, so those events read
//! the window's state before stopping. A macOS minimize reads as on screen
//! until its animation ends, and no window event follows it, so AppKit's
//! miniaturize notification stops the session instead ([`observe_minimize`]).

use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, Runtime, WindowEvent};

use super::DictationState;

/// The label of the window dictation inserts into.
pub const MAIN_WINDOW: &str = "main";

/// What a main-window event says about whether the window can still be seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sight {
    /// The window is closing, closed or minimized.
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

/// The main window's state as far as it can be read: whether it is visible,
/// and whether it is minimized.
type WindowState = (Option<bool>, Option<bool>);

/// Stops the live session when the main window has been hidden, minimized or
/// closed. Returns the stopped session's id.
pub fn stop_when_out_of_sight<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    event: &WindowEvent,
) -> Option<u32> {
    stop_after(app, label, sight_after(event), || {
        app.get_webview_window(MAIN_WINDOW)
            .map(|window| (window.is_visible().ok(), window.is_minimized().ok()))
    })
}

/// Stops the live session because the main window was minimized. Returns the
/// stopped session's id.
pub fn stop_when_minimized<R: Runtime>(app: &AppHandle<R>) -> Option<u32> {
    stop_after(app, MAIN_WINDOW, Sight::Gone, || None)
}

/// Stops the live session when `sight` in window `label` leaves the main
/// window off the screen. `read` gives the main window's state, `None` when
/// there is no main window, and is called only when `sight` leaves it open.
fn stop_after<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    sight: Sight,
    read: impl FnOnce() -> Option<WindowState>,
) -> Option<u32> {
    if label != MAIN_WINDOW || sight == Sight::Unchanged {
        return None;
    }
    let state = app.state::<DictationState>();
    if !state.is_busy() {
        return None;
    }
    let gone = sight == Sight::Gone
        || read().is_none_or(|(visible, minimized)| is_out_of_sight(visible, minimized));
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

/// Whether `window` is the main window's `NSWindow`.
#[cfg(target_os = "macos")]
pub fn is_main_ns_window<R: Runtime>(app: &AppHandle<R>, window: *const std::ffi::c_void) -> bool {
    app.get_webview_window(MAIN_WINDOW)
        .and_then(|main| main.ns_window().ok())
        .is_some_and(|main| std::ptr::eq(main.cast_const(), window))
}

/// Stops the live session each time AppKit reports a window minimized for
/// which `is_main` holds, for the app's life. Installed once, from setup.
#[cfg(target_os = "macos")]
pub fn observe_minimize<R: Runtime>(
    app: &AppHandle<R>,
    is_main: impl Fn(&AppHandle<R>, *const std::ffi::c_void) -> bool + 'static,
) {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_app_kit::NSWindowDidMiniaturizeNotification;
    use objc2_foundation::{NSNotification, NSNotificationCenter};

    let app = app.clone();
    let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
        // SAFETY: AppKit hands the block a notification that is live for the
        // length of the call.
        let Some(window) = unsafe { notification.as_ref() }.object() else {
            return;
        };
        if is_main(&app, Retained::as_ptr(&window).cast()) {
            stop_when_minimized(&app);
        }
    });
    // SAFETY: the name is AppKit's own constant, no object or queue narrows
    // it, and the block holds only an app handle, which is Send and Sync.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSWindowDidMiniaturizeNotification),
            None,
            None,
            &block,
        )
    };
    // Never removed: the observer lives as long as the app.
    std::mem::forget(observer);
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

    /// A main window with a live session, and the controls that session
    /// watches.
    fn live_session(app: &tauri::App<MockRuntime>) -> (u32, super::super::session::Controls) {
        let state = app.state::<DictationState>();
        let reservation = state.reserve().expect("the slot is free");
        let id = reservation.id();
        let controls = reservation.controls();
        reservation.keep();
        (id, controls)
    }

    #[test]
    fn a_hidden_or_minimized_main_window_stops_the_session_on_focus_or_resize() {
        let resized = WindowEvent::Resized(PhysicalSize::new(800, 600));
        for (event, state) in [
            (WindowEvent::Focused(false), (Some(false), Some(false))),
            (WindowEvent::Focused(false), (Some(true), Some(true))),
            (resized.clone(), (Some(true), Some(true))),
            (resized.clone(), (Some(false), Some(false))),
        ] {
            let app = app_with_window(MAIN_WINDOW);
            let (id, controls) = live_session(&app);
            assert_eq!(
                stop_after(app.handle(), MAIN_WINDOW, sight_after(&event), || Some(
                    state
                )),
                Some(id),
                "{event:?} with {state:?}"
            );
            assert!(controls.stop.is_cancelled());
            assert!(!controls.cancel.is_cancelled(), "a stop, not a cancel");
        }
    }

    #[test]
    fn a_main_window_on_screen_or_another_window_keeps_the_session() {
        let app = app_with_window(MAIN_WINDOW);
        let (_, controls) = live_session(&app);
        let on_screen = || Some((Some(true), Some(false)));
        for event in [
            WindowEvent::Focused(false),
            WindowEvent::Resized(PhysicalSize::new(800, 600)),
        ] {
            assert_eq!(
                stop_after(app.handle(), MAIN_WINDOW, sight_after(&event), on_screen),
                None
            );
        }
        assert_eq!(
            stop_after(
                app.handle(),
                MAIN_WINDOW,
                sight_after(&WindowEvent::Focused(true)),
                || Some((Some(true), Some(true)))
            ),
            None,
            "an event that changes nothing reads no state"
        );
        assert_eq!(
            stop_after(app.handle(), "capture", Sight::Gone, || None),
            None
        );
        assert!(!controls.stop.is_cancelled());
    }

    #[test]
    fn a_minimized_main_window_stops_the_session() {
        let app = app_with_window(MAIN_WINDOW);
        assert_eq!(stop_when_minimized(app.handle()), None, "nothing is live");
        let (id, controls) = live_session(&app);
        assert_eq!(stop_when_minimized(app.handle()), Some(id));
        assert!(controls.stop.is_cancelled());
        assert!(!controls.cancel.is_cancelled());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn appkit_reporting_the_main_window_minimized_stops_the_session() {
        use objc2::rc::Retained;
        use objc2::runtime::NSObject;
        use objc2_app_kit::NSWindowDidMiniaturizeNotification;
        use objc2_foundation::NSNotificationCenter;

        let app = app_with_window(MAIN_WINDOW);
        let (_, controls) = live_session(&app);
        let main = NSObject::new();
        let other = NSObject::new();
        let main_ptr = Retained::as_ptr(&main) as usize;
        observe_minimize(app.handle(), move |_, window| window as usize == main_ptr);

        let post = |window: &NSObject| {
            // SAFETY: the name is AppKit's constant and any object may carry it.
            unsafe {
                NSNotificationCenter::defaultCenter()
                    .postNotificationName_object(NSWindowDidMiniaturizeNotification, Some(window));
            }
        };
        post(&other);
        assert!(
            !controls.stop.is_cancelled(),
            "another window was minimized"
        );
        post(&main);
        assert!(controls.stop.is_cancelled());
        assert!(!controls.cancel.is_cancelled());
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
