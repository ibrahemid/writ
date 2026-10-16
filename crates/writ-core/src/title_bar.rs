//! What a double-click on the window's title bar does.
//!
//! macOS keeps the choice under System Settings, "Double-click a window's title
//! bar to", as `AppleActionOnDoubleClick` in the global defaults domain. Before
//! that setting had more than two choices it was the boolean
//! `AppleMiniaturizeOnDoubleClick`, which still answers when the newer key is
//! missing. Windows and Linux keep no setting Writ reads, so a double-click
//! there zooms, which is maximize and restore.

/// The global defaults key holding the macOS setting.
pub const ACTION_ON_DOUBLE_CLICK_KEY: &str = "AppleActionOnDoubleClick";

/// The global defaults key holding the setting's earlier boolean form.
pub const MINIATURIZE_ON_DOUBLE_CLICK_KEY: &str = "AppleMiniaturizeOnDoubleClick";

/// The system setting as read. Either key may be missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DoubleClickPreference {
    /// `AppleActionOnDoubleClick`: `Maximize`, `Fill`, `Minimize` or `None`.
    pub action: Option<String>,
    /// `AppleMiniaturizeOnDoubleClick`.
    pub miniaturize: Option<bool>,
}

/// What the setting asks a title-bar double-click to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleBarDoubleClick {
    /// Zoom: the window takes the screen's visible frame, or goes back to the
    /// frame it had before.
    Zoom,
    /// Fill: the window takes the screen's visible frame.
    Fill,
    /// Minimize to the Dock.
    Minimize,
    /// Do nothing.
    Nothing,
}

impl DoubleClickPreference {
    /// The action the setting names. `AppleActionOnDoubleClick` wins when it
    /// holds a known value; otherwise the boolean decides, and a setting with
    /// neither zooms.
    #[must_use]
    pub fn resolve(&self) -> TitleBarDoubleClick {
        let named = self.action.as_deref().map(str::trim).and_then(|value| {
            [
                ("Maximize", TitleBarDoubleClick::Zoom),
                ("Fill", TitleBarDoubleClick::Fill),
                ("Minimize", TitleBarDoubleClick::Minimize),
                ("None", TitleBarDoubleClick::Nothing),
            ]
            .into_iter()
            .find_map(|(name, action)| value.eq_ignore_ascii_case(name).then_some(action))
        });
        named.unwrap_or(if self.miniaturize == Some(true) {
            TitleBarDoubleClick::Minimize
        } else {
            TitleBarDoubleClick::Zoom
        })
    }
}

/// The window as the double-click finds it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowState {
    /// The window fills the screen's visible frame.
    pub maximized: bool,
    /// The window is in full screen.
    pub fullscreen: bool,
}

/// The change a title-bar double-click makes to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleBarEffect {
    /// Take the screen's visible frame.
    Maximize,
    /// Go back to the frame the window had before it was maximized.
    Unmaximize,
    /// Minimize to the Dock or the taskbar.
    Minimize,
    /// Leave the window as it is.
    Unchanged,
}

/// The change `action` makes to a window in `state`.
///
/// Zoom toggles between the visible frame and the frame before it: Writ sets
/// no largest size of its own, so the standard frame macOS zooms to is the
/// visible frame. Fill takes the visible frame and leaves a window already
/// filling it as it is. A window in full screen has no title bar to
/// double-click and is left as it is, as macOS leaves it.
#[must_use]
pub fn title_bar_effect(action: TitleBarDoubleClick, state: WindowState) -> TitleBarEffect {
    if state.fullscreen {
        return TitleBarEffect::Unchanged;
    }
    match action {
        TitleBarDoubleClick::Zoom if state.maximized => TitleBarEffect::Unmaximize,
        TitleBarDoubleClick::Fill if state.maximized => TitleBarEffect::Unchanged,
        TitleBarDoubleClick::Zoom | TitleBarDoubleClick::Fill => TitleBarEffect::Maximize,
        TitleBarDoubleClick::Minimize => TitleBarEffect::Minimize,
        TitleBarDoubleClick::Nothing => TitleBarEffect::Unchanged,
    }
}
