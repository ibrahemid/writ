//! The macOS termination hook, so `NSApp terminate:` flushes like Cmd+Q does.
//!
//! Quit does not arrive one way. The menu item calls [`tauri::AppHandle::exit`]
//! and so raises `RunEvent::ExitRequested`, which the shutdown path already
//! handles. The Dock's Quit, an Apple Event quit and a logout all send AppKit's
//! `terminate:` instead, which tears the process down without ever reaching the
//! event loop: no exit request, no flush, and whatever the user typed inside
//! the autosave debounce window is gone.
//!
//! AppKit's own answer to that is `applicationShouldTerminate:` returning
//! `NSTerminateLater`, which holds the termination open until
//! `replyToApplicationShouldTerminate:`. Nothing between here and AppKit
//! offers it: tao 0.35.3 registers `applicationWillTerminate:` on its delegate
//! and no termination hook that can be answered late, and neither tauri 2.11.5
//! nor tauri-runtime-wry exposes one. So the selector is added to the class of
//! the delegate tao installed, which is sound precisely because tao has not
//! defined it — nothing is being replaced.
//!
//! The handler shares [`crate::quit::QuitState`] with the exit-request path, so
//! a Cmd+Q pressed during a Dock quit waits on the flush already running rather
//! than starting a second one.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
use tauri::Manager;

use crate::quit::QuitDecision;
use crate::state::AppState;

/// The app handle the termination hook works through. An AppKit selector
/// carries nothing of ours, so the handle has to be reachable from a static.
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// How long a termination that arrived on top of a running flush will wait for
/// that flush before replying anyway. Twice the flush timeout, because the
/// flush it is waiting on may itself be sitting out the whole of that.
const HANDOVER_LIMIT: Duration = Duration::from_secs(4);

/// How often the handover waits re-read the phase.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Objective-C type encoding for
/// `NSApplicationTerminateReply (*)(id self, SEL _cmd, NSApplication *sender)`:
/// an `NSUInteger` return over the two implicit arguments and the sender.
const SIGNATURE: &std::ffi::CStr = c"Q@:@";

/// Installs the termination hook on the running application delegate.
///
/// Called once, from setup, after tao has installed its delegate. A failure is
/// logged and nothing else: the app still quits correctly through every other
/// gesture, it only loses the last debounce window on this one.
pub fn install(app: &tauri::AppHandle) {
    if APP.set(app.clone()).is_err() {
        return;
    }

    let Some(mtm) = MainThreadMarker::new() else {
        tracing::warn!("termination hook needs the main thread");
        return;
    };

    let ns_app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = ns_app.delegate() else {
        tracing::warn!("no application delegate to install the termination hook on");
        return;
    };

    // SAFETY: `delegate` holds a live Objective-C object for the length of
    // this borrow, and every Objective-C object is an `AnyObject`; the cast
    // only forgets which protocol it conforms to.
    let delegate_object: &AnyObject = unsafe { &*Retained::as_ptr(&delegate).cast::<AnyObject>() };
    let class: &AnyClass = delegate_object.class();
    let selector = sel!(applicationShouldTerminate:);

    if class.instance_method(selector).is_some() {
        // Replacing an implementation means owning what it did, and there is
        // nothing here to own: no version of tao in the tree defines this.
        // Quitting through the Dock keeps the behaviour it has today.
        tracing::warn!(
            class = %class.name().to_string_lossy(),
            "application delegate already answers applicationShouldTerminate:, hook not installed"
        );
        return;
    }

    // SAFETY: the selector is absent from this class, checked directly above,
    // so nothing is being overwritten. `should_terminate` has exactly the
    // signature `SIGNATURE` describes and the one AppKit calls this selector
    // with, and it is a plain function pointer with no captured state.
    let added = unsafe {
        objc2::ffi::class_addMethod(
            (class as *const AnyClass).cast_mut(),
            selector,
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(
                    &AnyObject,
                    Sel,
                    *mut AnyObject,
                ) -> NSApplicationTerminateReply,
                objc2::runtime::Imp,
            >(should_terminate),
            SIGNATURE.as_ptr(),
        )
    };

    if !added.as_bool() {
        tracing::warn!("failed to install the termination hook");
        return;
    }

    // `setDelegate:` is where AppKit records which delegate methods exist, and
    // tao set the delegate long before the selector did. Setting the same
    // object again is what makes AppKit look now.
    ns_app.setDelegate(Some(&delegate));
    tracing::info!("termination hook installed");
}

/// AppKit's question, answered late.
///
/// Returns `TerminateLater` whenever there is anything left to write, and the
/// termination resumes from [`reply_when_complete`] once it is written.
unsafe extern "C-unwind" fn should_terminate(
    _this: &AnyObject,
    _cmd: Sel,
    _sender: *mut AnyObject,
) -> NSApplicationTerminateReply {
    let Some(app) = APP.get() else {
        return NSApplicationTerminateReply::TerminateNow;
    };
    let state = app.state::<AppState>();

    match state.quit.begin(None) {
        // Written already, by an exit request that got here first.
        QuitDecision::Proceed => NSApplicationTerminateReply::TerminateNow,
        // A flush is running. Let it finish and answer for it.
        QuitDecision::Wait => {
            reply_when_complete(app.clone());
            NSApplicationTerminateReply::TerminateLater
        }
        QuitDecision::StartFlush => {
            start_flush(app.clone());
            NSApplicationTerminateReply::TerminateLater
        }
    }
}

/// Runs the same handshake the exit-request path runs, off the main thread so
/// the webview can answer it, and resumes the termination afterwards.
fn start_flush(app: tauri::AppHandle) {
    let state = app.state::<AppState>();
    state
        .notes_index_cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);

    if !state
        .frontend_ready
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        crate::finish_shutdown(&app);
        state.quit.finish();
        reply_on_main();
        return;
    }

    state
        .event_bus
        .emit(writ_core::events::bus::WritEvent::FlushBeforeQuit);

    let quit_state = state.quit.clone();
    std::thread::spawn(move || {
        if !quit_state.wait_for_flush() {
            tracing::warn!("frontend did not confirm its flush before the timeout");
        }
        crate::finish_shutdown(&app);
        // Only now may another quit gesture take the process down.
        quit_state.finish();
        reply_on_main();
    });
}

/// Waits for a flush started elsewhere, then resumes the termination.
fn reply_when_complete(app: tauri::AppHandle) {
    let quit_state = app.state::<AppState>().quit.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        while !quit_state.is_complete() && started.elapsed() < HANDOVER_LIMIT {
            std::thread::sleep(POLL_INTERVAL);
        }
        reply_on_main();
    });
}

/// Answers AppKit on the main thread, which is the only thread that may.
fn reply_on_main() {
    DispatchQueue::main().exec_async(|| {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        NSApplication::sharedApplication(mtm).replyToApplicationShouldTerminate(true);
    });
}
