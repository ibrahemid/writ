//! Win32 half of the snap-layout overlay: a hit-test-only child window over the
//! maximize button, plus the top-level subclass that keeps it in place.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Manager, WebviewWindow};
use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH, NULL_BRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW,
    RegisterClassExW, SetWindowLongPtrW, SetWindowPos, CREATESTRUCTW, GWLP_USERDATA, HTMAXBUTTON,
    HWND_TOP, SWP_NOACTIVATE, WINDOW_EX_STYLE, WM_DPICHANGED, WM_NCCREATE, WM_NCDESTROY,
    WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE,
    WM_NCMOUSEMOVE, WM_SIZE, WNDCLASSEXW, WNDCLASS_STYLES, WS_CHILD, WS_CLIPSIBLINGS,
    WS_OVERLAPPED, WS_VISIBLE,
};

use super::{overlay_rect, CaptionButtonMetrics};
use crate::events::{emit_event_to_main, CaptionHitPhase, WritFrontendEvent};

const PARENT_SUBCLASS_ID: usize = 1;

/// Live overlay window, or 0 when there is none. Written only from the
/// event-loop thread; read from the IPC thread to decide create-or-reposition.
static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);

/// Last metrics the frontend reported. Read on every reposition, so the
/// overlay follows resizes and DPI changes without a new report.
static METRICS: Mutex<Option<CaptionButtonMetrics>> = Mutex::new(None);

struct OverlayState {
    app: AppHandle,
    hovered: bool,
    pressed: bool,
}

pub fn install(window: &WebviewWindow, metrics: CaptionButtonMetrics) -> Result<(), String> {
    {
        let mut slot = METRICS.lock().unwrap_or_else(|e| e.into_inner());
        *slot = Some(metrics);
    }

    // HWND is not Send: carry the raw pointer value across the thread hop and
    // rewrap it where it is used.
    let parent = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
    let app = window.app_handle().clone();
    window
        .run_on_main_thread(move || {
            let parent = HWND(parent as *mut c_void);
            // SAFETY: the event-loop thread owns `parent`, and every call below
            // is a window operation on it or on the overlay it created.
            unsafe {
                if OVERLAY_HWND.load(Ordering::SeqCst) == 0 {
                    match create_overlay(parent, app) {
                        Ok(overlay) => {
                            OVERLAY_HWND.store(overlay.0 as isize, Ordering::SeqCst);
                            let subclassed = SetWindowSubclass(
                                parent,
                                Some(parent_subclass_proc),
                                PARENT_SUBCLASS_ID,
                                0,
                            );
                            if !subclassed.as_bool() {
                                tracing::warn!(
                                    "snap-layout overlay installed without a window subclass; it will not follow resizes"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "snap-layout overlay could not be created");
                            return;
                        }
                    }
                }
                reposition(parent);
            }
        })
        .map_err(|e| e.to_string())
}

fn register_class() -> Result<(), String> {
    static CLASS: OnceLock<Result<(), String>> = OnceLock::new();
    CLASS
        .get_or_init(|| {
            let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: WNDCLASS_STYLES::default(),
                lpfnWndProc: Some(overlay_proc),
                hInstance: HINSTANCE(module.0),
                // The overlay exists to answer a hit test, never to draw: a null
                // brush leaves whatever the webview painted underneath visible.
                hbrBackground: HBRUSH(unsafe { GetStockObject(NULL_BRUSH) }.0),
                lpszClassName: w!("WritSnapOverlay"),
                ..Default::default()
            };
            if unsafe { RegisterClassExW(&class) } == 0 {
                return Err(windows::core::Error::from_win32().to_string());
            }
            Ok(())
        })
        .clone()
}

/// Set while `CreateWindowExW` is in flight, so a failed creation can tell
/// whether the window ever reached `WM_NCCREATE` and took ownership of the
/// state box. Only ever touched on the event-loop thread.
static NCCREATE_RAN: AtomicBool = AtomicBool::new(false);

unsafe fn create_overlay(parent: HWND, app: AppHandle) -> Result<HWND, String> {
    register_class()?;
    let module = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;
    let state = Box::into_raw(Box::new(OverlayState {
        app,
        hovered: false,
        pressed: false,
    }));

    NCCREATE_RAN.store(false, Ordering::SeqCst);
    let created = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("WritSnapOverlay"),
            None,
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            Some(parent),
            None,
            Some(HINSTANCE(module.0)),
            Some(state as *const c_void),
        )
    };

    match created {
        Ok(overlay) => Ok(overlay),
        Err(e) => {
            // A window that reached WM_NCCREATE owns the box and frees it from
            // WM_NCDESTROY, which Windows still sends for a failed creation.
            // Reclaim it only when creation failed before that point.
            if !NCCREATE_RAN.load(Ordering::SeqCst) {
                drop(unsafe { Box::from_raw(state) });
            }
            Err(e.to_string())
        }
    }
}

unsafe fn reposition(parent: HWND) {
    let overlay = OVERLAY_HWND.load(Ordering::SeqCst);
    if overlay == 0 {
        return;
    }
    // Copied out before any Win32 call: SetWindowPos dispatches messages
    // synchronously, and re-entering this function while the lock is held
    // would deadlock on a non-reentrant mutex.
    let Some(metrics) = *METRICS.lock().unwrap_or_else(|e| e.into_inner()) else {
        return;
    };

    let mut client = RECT::default();
    if unsafe { GetClientRect(parent, &mut client) }.is_err() {
        return;
    }
    let rect = overlay_rect(
        client.right - client.left,
        unsafe { GetDpiForWindow(parent) },
        metrics,
    );

    // HWND_TOP on every call, including when the geometry is unchanged: the
    // WebView2 controller re-asserts its own z-order whenever the host bounds
    // change, and a skipped raise leaves the overlay buried behind it.
    let _ = unsafe {
        SetWindowPos(
            HWND(overlay as *mut c_void),
            Some(HWND_TOP),
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            SWP_NOACTIVATE,
        )
    };
}

unsafe extern "system" fn parent_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _ref_data: usize,
) -> LRESULT {
    match msg {
        WM_SIZE | WM_DPICHANGED => unsafe { reposition(hwnd) },
        // Teardown belongs to WM_NCDESTROY, not WM_CLOSE: Writ hides on close,
        // so the window outlives every WM_CLOSE it receives (Alt+F4 included)
        // and tearing down there would kill snap for the rest of the session.
        WM_NCDESTROY => {
            let overlay = OVERLAY_HWND.swap(0, Ordering::SeqCst);
            if overlay != 0 {
                let _ = unsafe { DestroyWindow(HWND(overlay as *mut c_void)) };
            }
            let _ = unsafe {
                RemoveWindowSubclass(hwnd, Some(parent_subclass_proc), PARENT_SUBCLASS_ID)
            };
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        NCCREATE_RAN.store(true, Ordering::SeqCst);
        let create = lparam.0 as *const CREATESTRUCTW;
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
    }

    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut OverlayState;
    if state_ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }

    // Every arm below scopes its access to the state and takes an AppHandle
    // clone out of that scope: an emit re-enters the UI thread, so no borrow of
    // the state may still be live when one runs.
    match msg {
        // The overlay's whole rect is the button, so the answer is
        // unconditional. This is the one message that earns the window: it is
        // what makes Windows offer the snap-layout flyout.
        WM_NCHITTEST => LRESULT(HTMAXBUTTON as isize),

        WM_NCMOUSEMOVE => {
            let (app, entering) = {
                let state = unsafe { &mut *state_ptr };
                let entering = !state.hovered;
                state.hovered = true;
                (state.app.clone(), entering)
            };
            if entering {
                unsafe { track_leave(hwnd) };
                emit_phase(&app, CaptionHitPhase::Enter);
            }
            LRESULT(0)
        }

        WM_NCMOUSELEAVE => {
            let app = {
                let state = unsafe { &mut *state_ptr };
                state.hovered = false;
                state.pressed = false;
                state.app.clone()
            };
            emit_phase(&app, CaptionHitPhase::Leave);
            LRESULT(0)
        }

        // Returning 0 rather than falling through: DefWindowProcW answers a
        // non-client press on HTMAXBUTTON by entering the caption's own
        // button-tracking loop, which swallows the release and can start a
        // window drag.
        //
        // The second press of a fast double-click arrives as WM_NCLBUTTONDBLCLK
        // rather than WM_NCLBUTTONDOWN; treating it as a press keeps the second
        // toggle, the way a system caption button behaves.
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => {
            let app = {
                let state = unsafe { &mut *state_ptr };
                state.pressed = true;
                state.app.clone()
            };
            emit_phase(&app, CaptionHitPhase::Press);
            LRESULT(0)
        }

        WM_NCLBUTTONUP => {
            let (app, was_pressed) = {
                let state = unsafe { &mut *state_ptr };
                let was_pressed = state.pressed;
                state.pressed = false;
                (state.app.clone(), was_pressed)
            };
            if was_pressed {
                emit_phase(&app, CaptionHitPhase::Click);
            }
            LRESULT(0)
        }

        WM_NCDESTROY => {
            let previous =
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *mut OverlayState;
            if !previous.is_null() {
                drop(unsafe { Box::from_raw(previous) });
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }

        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Arms the one WM_NCMOUSELEAVE that ends the current hover. Without it the
/// button would stay lit after the cursor left, since the overlay sees no
/// further messages once the pointer is elsewhere.
unsafe fn track_leave(hwnd: HWND) {
    let mut track = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE | TME_NONCLIENT,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    let _ = unsafe { TrackMouseEvent(&mut track) };
}

fn emit_phase(app: &AppHandle, phase: CaptionHitPhase) {
    if let Err(e) = emit_event_to_main(app, WritFrontendEvent::CaptionMaximizeHit { phase }) {
        tracing::debug!(error = %e, phase = ?phase, "caption hit emit failed");
    }
}
