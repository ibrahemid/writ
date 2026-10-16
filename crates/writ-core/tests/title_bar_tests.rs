//! What a double-click on the window's title bar does, from the macOS setting
//! and the window's state.

use writ_core::title_bar::{
    title_bar_effect, DoubleClickPreference, TitleBarDoubleClick, TitleBarEffect, WindowState,
};

fn named(action: &str) -> DoubleClickPreference {
    DoubleClickPreference {
        action: Some(action.to_string()),
        miniaturize: None,
    }
}

fn floating() -> WindowState {
    WindowState {
        maximized: false,
        fullscreen: false,
    }
}

#[test]
fn each_system_settings_choice_names_its_action() {
    assert_eq!(named("Maximize").resolve(), TitleBarDoubleClick::Zoom);
    assert_eq!(named("Fill").resolve(), TitleBarDoubleClick::Fill);
    assert_eq!(named("Minimize").resolve(), TitleBarDoubleClick::Minimize);
    assert_eq!(named("None").resolve(), TitleBarDoubleClick::Nothing);
}

#[test]
fn the_value_is_read_without_regard_to_case_or_surrounding_space() {
    assert_eq!(named(" minimize ").resolve(), TitleBarDoubleClick::Minimize);
    assert_eq!(named("FILL").resolve(), TitleBarDoubleClick::Fill);
}

#[test]
fn a_named_action_wins_over_the_older_boolean() {
    let preference = DoubleClickPreference {
        action: Some("Maximize".to_string()),
        miniaturize: Some(true),
    };
    assert_eq!(preference.resolve(), TitleBarDoubleClick::Zoom);

    let preference = DoubleClickPreference {
        action: Some("None".to_string()),
        miniaturize: Some(true),
    };
    assert_eq!(preference.resolve(), TitleBarDoubleClick::Nothing);
}

#[test]
fn without_the_named_key_the_older_boolean_decides() {
    let minimize = DoubleClickPreference {
        action: None,
        miniaturize: Some(true),
    };
    assert_eq!(minimize.resolve(), TitleBarDoubleClick::Minimize);

    let zoom = DoubleClickPreference {
        action: None,
        miniaturize: Some(false),
    };
    assert_eq!(zoom.resolve(), TitleBarDoubleClick::Zoom);
}

#[test]
fn an_unknown_value_falls_back_to_the_older_boolean() {
    let preference = DoubleClickPreference {
        action: Some("Shade".to_string()),
        miniaturize: Some(true),
    };
    assert_eq!(preference.resolve(), TitleBarDoubleClick::Minimize);
    assert_eq!(named("").resolve(), TitleBarDoubleClick::Zoom);
}

#[test]
fn with_neither_key_a_double_click_zooms() {
    assert_eq!(
        DoubleClickPreference::default().resolve(),
        TitleBarDoubleClick::Zoom
    );
}

#[test]
fn zoom_toggles_between_the_visible_frame_and_the_frame_before() {
    assert_eq!(
        title_bar_effect(TitleBarDoubleClick::Zoom, floating()),
        TitleBarEffect::Maximize
    );
    let maximized = WindowState {
        maximized: true,
        fullscreen: false,
    };
    assert_eq!(
        title_bar_effect(TitleBarDoubleClick::Zoom, maximized),
        TitleBarEffect::Unmaximize
    );
}

#[test]
fn fill_takes_the_visible_frame_and_gives_it_back() {
    assert_eq!(
        title_bar_effect(TitleBarDoubleClick::Fill, floating()),
        TitleBarEffect::Maximize
    );
    let filled = WindowState {
        maximized: true,
        fullscreen: false,
    };
    assert_eq!(
        title_bar_effect(TitleBarDoubleClick::Fill, filled),
        TitleBarEffect::Unmaximize
    );
}

#[test]
fn minimize_minimizes_whatever_the_frame() {
    assert_eq!(
        title_bar_effect(TitleBarDoubleClick::Minimize, floating()),
        TitleBarEffect::Minimize
    );
    let maximized = WindowState {
        maximized: true,
        fullscreen: false,
    };
    assert_eq!(
        title_bar_effect(TitleBarDoubleClick::Minimize, maximized),
        TitleBarEffect::Minimize
    );
}

#[test]
fn do_nothing_leaves_the_window_alone() {
    assert_eq!(
        title_bar_effect(TitleBarDoubleClick::Nothing, floating()),
        TitleBarEffect::Unchanged
    );
}

#[test]
fn a_window_in_full_screen_is_left_as_it_is() {
    let fullscreen = WindowState {
        maximized: true,
        fullscreen: true,
    };
    for action in [
        TitleBarDoubleClick::Zoom,
        TitleBarDoubleClick::Fill,
        TitleBarDoubleClick::Minimize,
        TitleBarDoubleClick::Nothing,
    ] {
        assert_eq!(
            title_bar_effect(action, fullscreen),
            TitleBarEffect::Unchanged
        );
    }
}
