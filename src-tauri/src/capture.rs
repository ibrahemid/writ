//! The quick capture window (ADR-055): a second webview the capture chord
//! shows over whatever app is in front, and hides once its entry is written.
//!
//! The window is mechanism only. What the entry adds to Today's File is
//! `writ_core::capture`, and the append is `commands::capture`.

use std::sync::Mutex;

use tauri::{
    AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tracing::{info, warn};

/// The capture window's label. Every single-window path in the shell asks for
/// `"main"` by name, so none of them reaches this one.
pub const CAPTURE_LABEL: &str = "capture";

/// The note's size in logical pixels.
const CAPTURE_WIDTH: f64 = 560.0;
const CAPTURE_HEIGHT: f64 = 176.0;

/// What had the keyboard before the note took it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Previous {
    /// Writ's own window.
    Writ,
    /// Another app: its process on macOS, its window on Windows.
    Other(i64),
}

/// Where the keyboard goes when the note hides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnFocus {
    /// Back to the app that was in front.
    Other(i64),
    /// Writ's main window was in front, and still shows.
    MainWindow,
    /// Wherever the system puts it.
    Nothing,
}

/// [`ReturnFocus`] for what was in front before the note.
#[must_use]
pub fn decide_return(previous: Option<Previous>, main_visible: bool) -> ReturnFocus {
    match previous {
        Some(Previous::Other(token)) => ReturnFocus::Other(token),
        Some(Previous::Writ) if main_visible => ReturnFocus::MainWindow,
        Some(Previous::Writ) | None => ReturnFocus::Nothing,
    }
}

/// What showing and hiding the note asks of the desktop. A trait so the order
/// of the asks can be tested without one.
pub trait CaptureSurface {
    /// The note is on screen and has the keyboard.
    fn is_in_front(&self) -> bool;
    /// What has the keyboard now, before the note takes it.
    fn frontmost(&self) -> Option<Previous>;
    /// Places the note on the screen under the pointer and gives it the
    /// keyboard.
    fn show(&self);
    /// Takes the note off screen. What it holds stays for the next press.
    fn hide(&self);
    /// Writ's main window is on screen.
    fn main_visible(&self) -> bool;
    /// Gives Writ's main window the keyboard.
    fn focus_main(&self);
    /// Brings the app `token` names back to the front.
    fn activate(&self, token: i64);
}

/// What was in front when the note was last shown, until it hides.
static PREVIOUS: Mutex<Option<Previous>> = Mutex::new(None);

/// The capture chord: shows the note, or hides it when it is already in
/// front, the way the summon chord toggles the window.
pub fn press(surface: &impl CaptureSurface, previous: &Mutex<Option<Previous>>) {
    if surface.is_in_front() {
        close(surface, previous, true);
        return;
    }
    let front = surface.frontmost();
    *previous.lock().unwrap_or_else(|e| e.into_inner()) = front;
    surface.show();
}

/// Hides the note. With `return_focus`, the keyboard goes back to what had it
/// before, asked for first so the system never hands it to another window of
/// Writ's in between. Without it, the person already moved on.
pub fn close(
    surface: &impl CaptureSurface,
    previous: &Mutex<Option<Previous>>,
    return_focus: bool,
) {
    let before = previous.lock().unwrap_or_else(|e| e.into_inner()).take();
    if return_focus {
        match decide_return(before, surface.main_visible()) {
            ReturnFocus::Other(token) => surface.activate(token),
            ReturnFocus::MainWindow => surface.focus_main(),
            ReturnFocus::Nothing => {}
        }
    }
    surface.hide();
}

/// Where the note opens on a monitor's work area: centred across, a fifth of
/// the way down. Physical pixels throughout.
#[must_use]
pub fn placement(
    area_x: i32,
    area_y: i32,
    area_width: u32,
    area_height: u32,
    width: u32,
) -> (i32, i32) {
    let spare = i64::from(area_width) - i64::from(width);
    let x = i64::from(area_x) + spare.max(0) / 2;
    let y = i64::from(area_y) + i64::from(area_height) / 5;
    (
        i32::try_from(x).unwrap_or(area_x),
        i32::try_from(y).unwrap_or(area_y),
    )
}

/// Builds the window, hidden. It loads its own page, so the keys typed after
/// the first press land in a page that is already there.
fn build(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(CAPTURE_LABEL) {
        return Ok(window);
    }
    let window =
        WebviewWindowBuilder::new(app, CAPTURE_LABEL, WebviewUrl::App("capture.html".into()))
            .title("Quick capture")
            .inner_size(CAPTURE_WIDTH, CAPTURE_HEIGHT)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .decorations(false)
            .transparent(true)
            .shadow(true)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .visible(false)
            .focused(false)
            // A drop on the note is text for the note, not files for the main
            // window to open.
            .disable_drag_drop_handler()
            .build()?;
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            close(&OsSurface(&handle), &PREVIOUS, true);
        }
    });
    info!("capture window built");
    Ok(window)
}

/// Builds the window ahead of the first press, when the capture chord is held.
/// Off the thread that asked: a window built inside an event handler can
/// deadlock WebView2.
pub fn prepare(app: &AppHandle) {
    if !crate::hotkey::capture_chord_is_held() {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = build(&handle) {
            warn!(%error, "the capture window could not be built");
        }
    });
}

/// What the capture chord runs.
pub fn on_chord(app: &AppHandle) {
    if app.get_webview_window(CAPTURE_LABEL).is_some() {
        press(&OsSurface(app), &PREVIOUS);
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = build(&handle) {
            warn!(%error, "the capture window could not be built");
            return;
        }
        let shown = handle.clone();
        if let Err(error) = handle.run_on_main_thread(move || press(&OsSurface(&shown), &PREVIOUS))
        {
            warn!(%error, "the capture window could not be shown");
        }
    });
}

/// Hides the note, from its own page (Escape, or the click elsewhere that took
/// the keyboard away).
pub fn close_from_page(app: &AppHandle, return_focus: bool) {
    close(&OsSurface(app), &PREVIOUS, return_focus);
}

/// Whether the note has the keyboard. The menu bar's items act on the main
/// window, so their chords are not forwarded while it does.
pub fn is_in_front(app: &AppHandle) -> bool {
    OsSurface(app).is_in_front()
}

/// [`CaptureSurface`] over the running app.
struct OsSurface<'a>(&'a AppHandle);

impl OsSurface<'_> {
    fn window(&self) -> Option<WebviewWindow> {
        self.0.get_webview_window(CAPTURE_LABEL)
    }

    fn place(&self, window: &WebviewWindow) {
        let Ok(pointer) = self.0.cursor_position() else {
            return;
        };
        let monitor = match self.0.monitor_from_point(pointer.x, pointer.y) {
            Ok(Some(monitor)) => monitor,
            _ => match self.0.primary_monitor() {
                Ok(Some(monitor)) => monitor,
                _ => return,
            },
        };
        let width = window.outer_size().map(|size| size.width).unwrap_or(0);
        let area = monitor.work_area();
        let (x, y) = placement(
            area.position.x,
            area.position.y,
            area.size.width,
            area.size.height,
            width,
        );
        if let Err(error) = window.set_position(PhysicalPosition::new(x, y)) {
            warn!(%error, "the capture window could not be placed");
        }
    }
}

impl CaptureSurface for OsSurface<'_> {
    fn is_in_front(&self) -> bool {
        self.window().is_some_and(|window| {
            window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false)
        })
    }

    fn frontmost(&self) -> Option<Previous> {
        platform::frontmost()
    }

    fn show(&self) {
        let Some(window) = self.window() else {
            return;
        };
        self.place(&window);
        if let Err(error) = window.show().and_then(|()| window.set_focus()) {
            warn!(%error, "the capture window could not be shown");
        }
    }

    fn hide(&self) {
        if let Some(window) = self.window() {
            if let Err(error) = window.hide() {
                warn!(%error, "the capture window could not be hidden");
            }
        }
    }

    fn main_visible(&self) -> bool {
        self.0
            .get_webview_window("main")
            .is_some_and(|window| window.is_visible().unwrap_or(false))
    }

    fn focus_main(&self) {
        if let Some(window) = self.0.get_webview_window("main") {
            let _ = window.set_focus();
        }
    }

    fn activate(&self, token: i64) {
        platform::activate(token);
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Previous;
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

    pub(super) fn frontmost() -> Option<Previous> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let pid = app.processIdentifier();
        if pid < 0 {
            return None;
        }
        if u32::try_from(pid).ok() == Some(std::process::id()) {
            Some(Previous::Writ)
        } else {
            Some(Previous::Other(i64::from(pid)))
        }
    }

    pub(super) fn activate(token: i64) {
        let Ok(pid) = i32::try_from(token) else {
            return;
        };
        if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
            if !app.activateWithOptions(NSApplicationActivationOptions::empty()) {
                tracing::warn!("the app in front before the capture note could not be activated");
            }
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::Previous;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };

    pub(super) fn frontmost() -> Option<Previous> {
        // SAFETY: neither call takes a pointer Writ owns; the process id is
        // written into a local.
        let (window, pid) = unsafe {
            let window = GetForegroundWindow();
            let mut pid = 0u32;
            GetWindowThreadProcessId(window, Some(std::ptr::addr_of_mut!(pid)));
            (window, pid)
        };
        if window.0.is_null() {
            return None;
        }
        if pid == std::process::id() {
            Some(Previous::Writ)
        } else {
            Some(Previous::Other(window.0 as i64))
        }
    }

    pub(super) fn activate(token: i64) {
        // SAFETY: a handle that has gone stale is refused by the call, which
        // then returns false.
        let brought = unsafe { SetForegroundWindow(HWND(token as *mut core::ffi::c_void)) };
        if !brought.as_bool() {
            tracing::warn!("the window in front before the capture note could not be activated");
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use super::Previous;

    pub(super) fn frontmost() -> Option<Previous> {
        None
    }

    pub(super) fn activate(_token: i64) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records each ask in order.
    struct FakeDesktop {
        front: Option<Previous>,
        in_front: bool,
        main_visible: bool,
        asks: RefCell<Vec<String>>,
    }

    impl FakeDesktop {
        fn new(front: Option<Previous>, main_visible: bool) -> Self {
            Self {
                front,
                in_front: false,
                main_visible,
                asks: RefCell::new(Vec::new()),
            }
        }

        fn asks(&self) -> Vec<String> {
            self.asks.borrow().clone()
        }
    }

    impl CaptureSurface for FakeDesktop {
        fn is_in_front(&self) -> bool {
            self.in_front
        }
        fn frontmost(&self) -> Option<Previous> {
            self.front
        }
        fn show(&self) {
            self.asks.borrow_mut().push("show".into());
        }
        fn hide(&self) {
            self.asks.borrow_mut().push("hide".into());
        }
        fn main_visible(&self) -> bool {
            self.main_visible
        }
        fn focus_main(&self) {
            self.asks.borrow_mut().push("focus main".into());
        }
        fn activate(&self, token: i64) {
            self.asks.borrow_mut().push(format!("activate {token}"));
        }
    }

    #[test]
    fn escape_after_an_entry_gives_the_keyboard_back_to_the_app_that_had_it() {
        let slot = Mutex::new(None);
        let desktop = FakeDesktop::new(Some(Previous::Other(4242)), true);
        press(&desktop, &slot);
        close(&desktop, &slot, true);
        assert_eq!(desktop.asks(), ["show", "activate 4242", "hide"]);
    }

    #[test]
    fn a_note_opened_from_writ_hands_the_keyboard_to_the_main_window() {
        let slot = Mutex::new(None);
        let desktop = FakeDesktop::new(Some(Previous::Writ), true);
        press(&desktop, &slot);
        close(&desktop, &slot, true);
        assert_eq!(desktop.asks(), ["show", "focus main", "hide"]);
    }

    #[test]
    fn a_hidden_main_window_is_not_brought_back() {
        let slot = Mutex::new(None);
        let desktop = FakeDesktop::new(Some(Previous::Writ), false);
        press(&desktop, &slot);
        close(&desktop, &slot, true);
        assert_eq!(desktop.asks(), ["show", "hide"]);
    }

    #[test]
    fn clicking_away_hides_the_note_and_leaves_the_keyboard_where_it_went() {
        let slot = Mutex::new(None);
        let desktop = FakeDesktop::new(Some(Previous::Other(7)), true);
        press(&desktop, &slot);
        close(&desktop, &slot, false);
        assert_eq!(desktop.asks(), ["show", "hide"]);
        assert_eq!(*slot.lock().unwrap(), None);
    }

    #[test]
    fn a_second_close_brings_nothing_back_twice() {
        let slot = Mutex::new(None);
        let desktop = FakeDesktop::new(Some(Previous::Other(7)), false);
        press(&desktop, &slot);
        close(&desktop, &slot, true);
        close(&desktop, &slot, true);
        assert_eq!(desktop.asks(), ["show", "activate 7", "hide", "hide"]);
    }

    #[test]
    fn the_chord_hides_a_note_already_in_front() {
        let slot = Mutex::new(Some(Previous::Other(9)));
        let mut desktop = FakeDesktop::new(Some(Previous::Writ), false);
        desktop.in_front = true;
        press(&desktop, &slot);
        assert_eq!(desktop.asks(), ["activate 9", "hide"]);
    }

    #[test]
    fn the_note_opens_centred_a_fifth_of_the_way_down() {
        assert_eq!(placement(0, 25, 1440, 875, 560), (440, 200));
        assert_eq!(placement(-1920, 0, 1920, 1080, 560), (-1240, 216));
        assert_eq!(placement(0, 0, 400, 800, 560), (0, 160));
    }
}
