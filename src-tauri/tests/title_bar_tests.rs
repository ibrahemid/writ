//! A double-click on the window's top chrome: the system setting is read, the
//! window's state decides the change, and the window is told to make it.

use std::cell::RefCell;

use writ_core::title_bar::{DoubleClickPreference, TitleBarEffect};
use writ_tauri_lib::commands::window::{
    title_bar_double_click_inner, TitleBarError, TitleBarWindow,
};

const LIB_RS: &str = include_str!("../src/lib.rs");

/// A window that records what it was told and holds a state to report.
#[derive(Default)]
struct FakeWindow {
    maximized: bool,
    fullscreen: bool,
    refuse: bool,
    calls: RefCell<Vec<&'static str>>,
}

impl FakeWindow {
    fn record(&self, call: &'static str) -> Result<(), TitleBarError> {
        self.calls.borrow_mut().push(call);
        if self.refuse {
            return Err(TitleBarError::Change("refused".to_string()));
        }
        Ok(())
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.borrow().clone()
    }
}

impl TitleBarWindow for FakeWindow {
    fn is_maximized(&self) -> Result<bool, TitleBarError> {
        Ok(self.maximized)
    }

    fn is_fullscreen(&self) -> Result<bool, TitleBarError> {
        Ok(self.fullscreen)
    }

    fn maximize(&self) -> Result<(), TitleBarError> {
        self.record("maximize")
    }

    fn unmaximize(&self) -> Result<(), TitleBarError> {
        self.record("unmaximize")
    }

    fn minimize(&self) -> Result<(), TitleBarError> {
        self.record("minimize")
    }
}

fn setting(action: &str) -> DoubleClickPreference {
    DoubleClickPreference {
        action: Some(action.to_string()),
        miniaturize: None,
    }
}

#[test]
fn zoom_maximizes_a_floating_window() {
    let window = FakeWindow::default();
    let effect = title_bar_double_click_inner(&window, &setting("Maximize")).unwrap();
    assert_eq!(effect, TitleBarEffect::Maximize);
    assert_eq!(window.calls(), ["maximize"]);
}

#[test]
fn zoom_restores_a_maximized_window() {
    let window = FakeWindow {
        maximized: true,
        ..FakeWindow::default()
    };
    let effect = title_bar_double_click_inner(&window, &setting("Maximize")).unwrap();
    assert_eq!(effect, TitleBarEffect::Unmaximize);
    assert_eq!(window.calls(), ["unmaximize"]);
}

#[test]
fn fill_takes_the_visible_frame_and_gives_it_back() {
    let floating = FakeWindow::default();
    title_bar_double_click_inner(&floating, &setting("Fill")).unwrap();
    assert_eq!(floating.calls(), ["maximize"]);

    let filled = FakeWindow {
        maximized: true,
        ..FakeWindow::default()
    };
    let effect = title_bar_double_click_inner(&filled, &setting("Fill")).unwrap();
    assert_eq!(effect, TitleBarEffect::Unmaximize);
    assert_eq!(filled.calls(), ["unmaximize"]);
}

#[test]
fn minimize_minimizes() {
    let window = FakeWindow::default();
    let effect = title_bar_double_click_inner(&window, &setting("Minimize")).unwrap();
    assert_eq!(effect, TitleBarEffect::Minimize);
    assert_eq!(window.calls(), ["minimize"]);
}

#[test]
fn the_older_minimize_setting_still_minimizes() {
    let window = FakeWindow::default();
    let preference = DoubleClickPreference {
        action: None,
        miniaturize: Some(true),
    };
    title_bar_double_click_inner(&window, &preference).unwrap();
    assert_eq!(window.calls(), ["minimize"]);
}

#[test]
fn do_nothing_tells_the_window_nothing() {
    let window = FakeWindow::default();
    let effect = title_bar_double_click_inner(&window, &setting("None")).unwrap();
    assert_eq!(effect, TitleBarEffect::Unchanged);
    assert!(window.calls().is_empty());
}

#[test]
fn a_full_screen_window_is_told_nothing() {
    let window = FakeWindow {
        fullscreen: true,
        ..FakeWindow::default()
    };
    let effect = title_bar_double_click_inner(&window, &setting("Minimize")).unwrap();
    assert_eq!(effect, TitleBarEffect::Unchanged);
    assert!(window.calls().is_empty());
}

#[test]
fn without_a_setting_the_window_maximizes_and_restores() {
    let window = FakeWindow::default();
    title_bar_double_click_inner(&window, &DoubleClickPreference::default()).unwrap();
    assert_eq!(window.calls(), ["maximize"]);
}

#[test]
fn a_window_that_refuses_the_change_is_reported() {
    let window = FakeWindow {
        refuse: true,
        ..FakeWindow::default()
    };
    let error = title_bar_double_click_inner(&window, &setting("Maximize")).unwrap_err();
    assert_eq!(error, TitleBarError::Change("refused".to_string()));
    assert!(error.to_string().starts_with("ERR_TITLE_BAR"));
}

#[test]
fn the_command_is_registered() {
    assert!(LIB_RS.contains("commands::window::title_bar_double_click,"));
}

/// The setting as `defaults` reports it from the global domain, which is where
/// System Settings writes it.
#[cfg(target_os = "macos")]
fn defaults_read_global(key: &str) -> Option<String> {
    let output = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", key])
        .output()
        .expect("run defaults");
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
#[test]
fn the_setting_is_read_from_the_global_domain() {
    use writ_core::title_bar::{ACTION_ON_DOUBLE_CLICK_KEY, MINIATURIZE_ON_DOUBLE_CLICK_KEY};

    let read = writ_tauri_lib::title_bar::read_double_click_preference();
    assert_eq!(
        read.action,
        defaults_read_global(ACTION_ON_DOUBLE_CLICK_KEY)
    );
    let miniaturize = defaults_read_global(MINIATURIZE_ON_DOUBLE_CLICK_KEY)
        .map(|value| value == "1" || value.eq_ignore_ascii_case("yes"));
    assert_eq!(read.miniaturize, miniaturize);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn there_is_no_setting_to_read_off_macos() {
    assert_eq!(
        writ_tauri_lib::title_bar::read_double_click_preference(),
        DoubleClickPreference::default()
    );
}
