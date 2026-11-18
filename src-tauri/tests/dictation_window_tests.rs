//! What the main window ends: hiding, minimizing or closing it, or hiding the
//! application, stops the live session; a page load in it cancels a start in
//! progress; and the watchdog that reads the window's state lives exactly as
//! long as its session.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use jotra_tauri_lib::dictation::session::Controls;
use jotra_tauri_lib::dictation::window::{
    cancel_on_page_load, read_on_main_thread, read_window_state, sight_after, stop_after,
    stop_when_gone, stop_when_out_of_sight, Sight, SightWatch, WindowState, MAIN_WINDOW,
    WATCH_PERIOD,
};
use jotra_tauri_lib::dictation::DictationState;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::webview::PageLoadEvent;
use tauri::{Manager, PhysicalSize, WindowEvent};

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

/// A main window with a live session, and the controls that session
/// watches.
fn live_session(app: &tauri::App<MockRuntime>) -> (u32, Controls) {
    let state = app.state::<DictationState>();
    let reservation = state.reserve().expect("the slot is free");
    let id = reservation.id();
    let controls = reservation.controls();
    reservation.keep();
    (id, controls)
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
fn closing_the_main_window_stops_the_live_session() {
    let app = app_with_window(MAIN_WINDOW);
    let (id, controls) = live_session(&app);

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

/// A read that finds the main window on screen in an application hidden once
/// `hidden` is set, counting the reads in `reads`.
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
        end(&app.state::<DictationState>(), id);
        let mut watch = SightWatch::with_reader(app.handle(), id, TICK, reader(&hidden, &reads));
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
    use jotra_tauri_lib::dictation::window::observe_minimize;
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
    use jotra_tauri_lib::dictation::window::observe_app_hide;
    use objc2::runtime::NSObject;
    use objc2_app_kit::NSApplicationDidHideNotification;
    use objc2_foundation::NSNotificationCenter;

    let app = app_with_window(MAIN_WINDOW);
    let (_, controls) = live_session(&app);
    observe_app_hide(app.handle());
    assert!(!controls.stop.is_cancelled());
    // SAFETY: the name is AppKit's constant and any object may carry it.
    unsafe {
        NSNotificationCenter::defaultCenter()
            .postNotificationName_object(NSApplicationDidHideNotification, Some(&NSObject::new()));
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
