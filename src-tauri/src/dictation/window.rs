//! What the main window ends: hiding the application, or hiding, minimizing
//! or closing the window, stops the live session, and a page load in it
//! cancels the session the old page started. A window that is only covered,
//! on another Space or unfocused keeps it.
//!
//! The window's own events carry no hide or minimize: on macOS both arrive
//! only as a loss of focus, elsewhere also as a resize, so those events read
//! the window's state before stopping. A macOS minimize reads as on screen
//! until its animation ends, and no window event follows it, so AppKit's
//! miniaturize notification stops the session instead ([`observe_minimize`]),
//! as its application-hide notification does ([`observe_app_hide`]). Whatever
//! raises neither is left to a watchdog ([`SightWatch`]) that reads the same
//! state once a second while a session is live. A minimize under Wayland is
//! not detected: GDK never reports it, so the window reads as on screen.

use std::future::Future;
use std::time::Duration;

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

/// How often the watchdog reads the main window's state while a session is
/// live.
pub const WATCH_PERIOD: Duration = Duration::from_secs(1);

/// Whether a window with this state is off the screen: hidden, minimized or
/// in a hidden application. A state that could not be read counts as on it.
pub fn is_out_of_sight(
    visible: Option<bool>,
    minimized: Option<bool>,
    app_hidden: Option<bool>,
) -> bool {
    visible == Some(false) || minimized == Some(true) || app_hidden == Some(true)
}

/// Whether a page load in webview `label` ends the session its old page
/// started.
pub fn page_load_ends_session(label: &str, event: PageLoadEvent) -> bool {
    label == MAIN_WINDOW && event == PageLoadEvent::Started
}

/// The main window's state as far as it can be read: whether it is visible,
/// whether it is minimized, and whether the application is hidden.
pub type WindowState = (Option<bool>, Option<bool>, Option<bool>);

/// Whether `state` leaves the main window off the screen; `None`, no main
/// window, does.
fn leaves_screen(state: Option<WindowState>) -> bool {
    state.is_none_or(|(visible, minimized, app_hidden)| {
        is_out_of_sight(visible, minimized, app_hidden)
    })
}

/// The main window's state, `None` when there is no main window. Whether the
/// application is hidden is read only on the main thread.
pub fn read_window_state<R: Runtime>(app: &AppHandle<R>) -> Option<WindowState> {
    let window = app.get_webview_window(MAIN_WINDOW)?;
    Some((
        window.is_visible().ok(),
        window.is_minimized().ok(),
        is_app_hidden(),
    ))
}

/// Whether the application is hidden; `None` off the main thread, where
/// AppKit may not be asked.
#[cfg(target_os = "macos")]
fn is_app_hidden() -> Option<bool> {
    let mtm = objc2::MainThreadMarker::new()?;
    Some(objc2_app_kit::NSApplication::sharedApplication(mtm).isHidden())
}

/// Whether the application is hidden; only macOS hides an application.
#[cfg(not(target_os = "macos"))]
fn is_app_hidden() -> Option<bool> {
    None
}

/// The main window's state read on the main thread, without blocking the
/// caller. `None` when the main thread did not answer.
pub async fn read_on_main_thread<R: Runtime>(app: AppHandle<R>) -> Option<Option<WindowState>> {
    let (answer, answered) = tokio::sync::oneshot::channel();
    let reader = app.clone();
    app.run_on_main_thread(move || {
        let _ = answer.send(read_window_state(&reader));
    })
    .ok()?;
    answered.await.ok()
}

/// Stops the live session when the main window has been hidden, minimized or
/// closed. Returns the stopped session's id.
pub fn stop_when_out_of_sight<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    event: &WindowEvent,
) -> Option<u32> {
    stop_after(app, label, sight_after(event), || read_window_state(app))
}

/// Stops the live session because the main window was minimized or the
/// application hidden. Returns the stopped session's id.
pub fn stop_when_gone<R: Runtime>(app: &AppHandle<R>) -> Option<u32> {
    stop_after(app, MAIN_WINDOW, Sight::Gone, || None)
}

/// Stops the live session when `sight` in window `label` leaves the main
/// window off the screen. `read` gives the main window's state, `None` when
/// there is no main window, and is called only when `sight` leaves it open.
pub fn stop_after<R: Runtime>(
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
    if sight != Sight::Gone && !leaves_screen(read()) {
        return None;
    }
    let id = state.stop_live()?;
    tracing::info!(
        session = id,
        "dictation: the window left the screen, stopping"
    );
    Some(id)
}

/// The watchdog of one session: stops it when a periodic read finds the main
/// window off the screen. It ends when the session ends and is aborted when
/// dropped, so it lives exactly as long as the session's task holds it. The
/// handle resolves when the watchdog ends.
pub struct SightWatch(pub tauri::async_runtime::JoinHandle<()>);

impl SightWatch {
    /// Watches session `session_id`, reading the state on the main thread
    /// every [`WATCH_PERIOD`].
    pub fn start<R: Runtime>(app: &AppHandle<R>, session_id: u32) -> Self {
        let reader = app.clone();
        Self::with_reader(app, session_id, WATCH_PERIOD, move || {
            read_on_main_thread(reader.clone())
        })
    }

    /// Watches session `session_id`, calling `read` every `period`; a read
    /// that gives `None` is skipped.
    pub fn with_reader<R, F, Read>(
        app: &AppHandle<R>,
        session_id: u32,
        period: Duration,
        read: Read,
    ) -> Self
    where
        R: Runtime,
        F: Future<Output = Option<Option<WindowState>>> + Send + 'static,
        Read: Fn() -> F + Send + 'static,
    {
        let app = app.clone();
        Self(tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(period).await;
                if !is_watched(&app, session_id) {
                    return;
                }
                let Some(state) = read().await else {
                    continue;
                };
                if leaves_screen(state) && is_watched(&app, session_id) {
                    app.state::<DictationState>().stop(session_id);
                    tracing::info!(
                        session = session_id,
                        "dictation: the window left the screen, stopping"
                    );
                    return;
                }
            }
        }))
    }
}

/// Whether session `session_id` still runs with no stop or cancel asked.
fn is_watched<R: Runtime>(app: &AppHandle<R>, session_id: u32) -> bool {
    app.state::<DictationState>()
        .controls(session_id)
        .is_some_and(|c| !c.stop.is_cancelled() && !c.cancel.is_cancelled())
}

impl Drop for SightWatch {
    fn drop(&mut self) {
        self.0.abort();
    }
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
    // SAFETY: an AppKit constant, read only.
    let name = unsafe { objc2_app_kit::NSWindowDidMiniaturizeNotification };
    stop_on_notification(app, name, move |app, object| {
        object.is_some_and(|window| is_main(app, window))
    });
}

/// Stops the live session each time AppKit reports the application hidden,
/// for the app's life. Installed once, from setup.
#[cfg(target_os = "macos")]
pub fn observe_app_hide<R: Runtime>(app: &AppHandle<R>) {
    // SAFETY: an AppKit constant, read only.
    let name = unsafe { objc2_app_kit::NSApplicationDidHideNotification };
    stop_on_notification(app, name, |_, _| true);
}

/// Stops the live session each time notification `name` arrives with an
/// object for which `matches` holds. The observer is never removed.
#[cfg(target_os = "macos")]
fn stop_on_notification<R: Runtime>(
    app: &AppHandle<R>,
    name: &'static objc2_foundation::NSNotificationName,
    matches: impl Fn(&AppHandle<R>, Option<*const std::ffi::c_void>) -> bool + 'static,
) {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_foundation::{NSNotification, NSNotificationCenter};

    let app = app.clone();
    let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
        // SAFETY: AppKit hands the block a notification that is live for the
        // length of the call.
        let object = unsafe { notification.as_ref() }.object();
        if matches(&app, object.as_ref().map(|o| Retained::as_ptr(o).cast())) {
            stop_when_gone(&app);
        }
    });
    // SAFETY: the name is AppKit's own constant, no object or queue narrows
    // it, and the block holds only an app handle, which is Send and Sync.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(name),
            None,
            None,
            &block,
        )
    };
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
    use tauri::PhysicalSize;

    use super::*;

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
    fn a_hidden_or_minimized_window_or_a_hidden_application_is_out_of_sight() {
        assert!(is_out_of_sight(Some(false), Some(false), Some(false)));
        assert!(is_out_of_sight(Some(true), Some(true), Some(false)));
        assert!(is_out_of_sight(Some(true), Some(false), Some(true)));
        assert!(is_out_of_sight(None, None, Some(true)));
        assert!(!is_out_of_sight(Some(true), Some(false), Some(false)));
        assert!(!is_out_of_sight(Some(true), Some(false), None));
        assert!(
            !is_out_of_sight(None, None, None),
            "an unread state is on screen"
        );
        assert!(leaves_screen(None), "no main window");
        assert!(leaves_screen(Some((None, None, Some(true)))));
        assert!(!leaves_screen(Some((Some(true), Some(false), None))));
    }

    #[test]
    fn the_application_hidden_state_is_not_read_off_the_main_thread() {
        let read = std::thread::spawn(is_app_hidden).join().expect("thread");
        assert_eq!(read, None);
    }

    #[test]
    fn only_a_page_starting_in_the_main_window_ends_the_session() {
        assert!(page_load_ends_session("main", PageLoadEvent::Started));
        assert!(!page_load_ends_session("main", PageLoadEvent::Finished));
        assert!(!page_load_ends_session("capture", PageLoadEvent::Started));
    }
}
