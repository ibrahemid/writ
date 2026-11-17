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
type WindowState = (Option<bool>, Option<bool>, Option<bool>);

/// Whether `state` leaves the main window off the screen; `None`, no main
/// window, does.
fn leaves_screen(state: Option<WindowState>) -> bool {
    state.is_none_or(|(visible, minimized, app_hidden)| {
        is_out_of_sight(visible, minimized, app_hidden)
    })
}

/// The main window's state, `None` when there is no main window. Whether the
/// application is hidden is read only on the main thread.
fn read_window_state<R: Runtime>(app: &AppHandle<R>) -> Option<WindowState> {
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
async fn read_on_main_thread<R: Runtime>(app: AppHandle<R>) -> Option<Option<WindowState>> {
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
/// dropped, so it lives exactly as long as the session's task holds it.
pub struct SightWatch(tauri::async_runtime::JoinHandle<()>);

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
    fn with_reader<R, F, Read>(
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

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
    fn the_main_window_state_is_read_from_the_window() {
        let app = app_with_window(MAIN_WINDOW);
        let read = [
            read_window_state(app.handle()),
            tauri::async_runtime::block_on(read_on_main_thread(app.handle().clone()))
                .expect("the main thread answers"),
        ];
        for state in read {
            let (visible, minimized, app_hidden) = state.expect("a main window");
            assert_eq!((visible, minimized), (Some(true), Some(false)));
            assert_ne!(app_hidden, Some(true));
        }
        let without_main = app_with_window("capture");
        assert_eq!(read_window_state(without_main.handle()), None);
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
            (
                WindowEvent::Focused(false),
                (Some(false), Some(false), None),
            ),
            (WindowEvent::Focused(false), (Some(true), Some(true), None)),
            (
                WindowEvent::Focused(false),
                (Some(true), Some(false), Some(true)),
            ),
            (resized.clone(), (Some(true), Some(true), None)),
            (resized.clone(), (Some(false), Some(false), None)),
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
        let on_screen = || Some((Some(true), Some(false), Some(false)));
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
                || Some((Some(true), Some(true), Some(true)))
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
    fn a_minimized_main_window_or_a_hidden_application_stops_the_session() {
        let app = app_with_window(MAIN_WINDOW);
        assert_eq!(stop_when_gone(app.handle()), None, "nothing is live");
        let (id, controls) = live_session(&app);
        assert_eq!(stop_when_gone(app.handle()), Some(id));
        assert!(controls.stop.is_cancelled());
        assert!(!controls.cancel.is_cancelled());
    }

    const TICK: Duration = Duration::from_millis(5);

    /// Waits up to five seconds for `done`.
    async fn wait_for(what: &str, done: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !done() {
                tokio::time::sleep(TICK).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
    }

    /// A read that finds the main window on screen in an application hidden
    /// once `hidden` is set, counting the reads in `reads`.
    fn reader(
        hidden: &Arc<AtomicBool>,
        reads: &Arc<AtomicUsize>,
    ) -> impl Fn() -> std::future::Ready<Option<Option<WindowState>>> + Send + 'static {
        let (hidden, reads) = (Arc::clone(hidden), Arc::clone(reads));
        move || {
            reads.fetch_add(1, Ordering::SeqCst);
            let app_hidden = hidden.load(Ordering::SeqCst);
            std::future::ready(Some(Some((Some(true), Some(false), Some(app_hidden)))))
        }
    }

    #[test]
    fn the_watchdog_stops_its_session_once_the_window_leaves_the_screen() {
        let app = app_with_window(MAIN_WINDOW);
        let (id, controls) = live_session(&app);
        let (hidden, reads) = (Arc::new(AtomicBool::new(false)), Arc::default());
        let mut watch = SightWatch::with_reader(app.handle(), id, TICK, reader(&hidden, &reads));
        tauri::async_runtime::block_on(async {
            wait_for("three reads", || reads.load(Ordering::SeqCst) >= 3).await;
            assert!(!controls.stop.is_cancelled(), "on screen keeps the session");
            hidden.store(true, Ordering::SeqCst);
            tokio::time::timeout(Duration::from_secs(5), &mut watch.0)
                .await
                .expect("the watchdog ends")
                .expect("the watchdog ran to its end");
        });
        assert!(controls.stop.is_cancelled());
        assert!(!controls.cancel.is_cancelled(), "a stop, not a cancel");
    }

    #[test]
    fn the_watchdog_ends_when_its_session_ends_or_is_asked_to_end() {
        let ends: [fn(&DictationState, u32); 3] = [
            DictationState::release,
            DictationState::stop,
            DictationState::cancel,
        ];
        for end in ends {
            let app = app_with_window(MAIN_WINDOW);
            let (id, _) = live_session(&app);
            let (hidden, reads) = (Arc::new(AtomicBool::new(false)), Arc::default());
            let mut watch =
                SightWatch::with_reader(app.handle(), id, TICK, reader(&hidden, &reads));
            end(&app.state::<DictationState>(), id);
            tauri::async_runtime::block_on(async {
                tokio::time::timeout(Duration::from_secs(5), &mut watch.0)
                    .await
                    .expect("the watchdog ends")
                    .expect("the watchdog ran to its end");
            });
            assert_eq!(
                reads.load(Ordering::SeqCst),
                0,
                "an ended session is not read"
            );
        }
    }

    #[test]
    fn dropping_the_watchdog_ends_it() {
        let app = app_with_window(MAIN_WINDOW);
        let (id, controls) = live_session(&app);
        let (hidden, reads) = (Arc::new(AtomicBool::new(false)), Arc::default());
        let watch = SightWatch::with_reader(app.handle(), id, TICK, reader(&hidden, &reads));
        tauri::async_runtime::block_on(async {
            wait_for("a read", || reads.load(Ordering::SeqCst) >= 1).await;
            drop(watch);
            tokio::time::sleep(TICK * 10).await;
            let after_drop = reads.load(Ordering::SeqCst);
            hidden.store(true, Ordering::SeqCst);
            tokio::time::sleep(TICK * 10).await;
            assert_eq!(reads.load(Ordering::SeqCst), after_drop);
        });
        assert!(!controls.stop.is_cancelled());
    }

    #[test]
    fn the_watchdog_reads_on_its_period_and_ends_with_the_session() {
        let app = app_with_window(MAIN_WINDOW);
        let (id, controls) = live_session(&app);
        let mut watch = SightWatch::start(app.handle(), id);
        tauri::async_runtime::block_on(async {
            tokio::time::sleep(WATCH_PERIOD + WATCH_PERIOD / 2).await;
            assert!(
                !controls.stop.is_cancelled(),
                "the mock window is on screen"
            );
            app.state::<DictationState>().release(id);
            tokio::time::timeout(WATCH_PERIOD * 3, &mut watch.0)
                .await
                .expect("the watchdog ends")
                .expect("the watchdog ran to its end");
        });
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

    #[cfg(target_os = "macos")]
    #[test]
    fn appkit_reporting_the_application_hidden_stops_the_session() {
        use objc2::runtime::NSObject;
        use objc2_app_kit::NSApplicationDidHideNotification;
        use objc2_foundation::NSNotificationCenter;

        let app = app_with_window(MAIN_WINDOW);
        let (_, controls) = live_session(&app);
        observe_app_hide(app.handle());
        assert!(!controls.stop.is_cancelled());
        // SAFETY: the name is AppKit's constant and any object may carry it.
        unsafe {
            NSNotificationCenter::defaultCenter().postNotificationName_object(
                NSApplicationDidHideNotification,
                Some(&NSObject::new()),
            );
        }
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
