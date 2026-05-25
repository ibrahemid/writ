//! Pure window-toggle state machine.
//!
//! The global hotkey and the menu-driven `toggle_window` command share the
//! same intent: bring Writ into view when away, dismiss it when active.
//! macOS minimization adds a fourth state that must be handled explicitly,
//! since a minimized window is technically "visible" but invisible to the
//! user.
//!
//! Decision logic lives here as a pure function so it can be unit-tested
//! without a real `tauri::Window`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleAction {
    /// Window is minimized to the dock; restore + focus.
    Unminimize,
    /// Window is hidden; show + focus.
    Show,
    /// Window is visible but unfocused; focus it.
    Focus,
    /// Window is visible and focused; hide it (always-ready scratchpad dismiss).
    Hide,
}

pub fn decide_toggle(is_minimized: bool, is_visible: bool, is_focused: bool) -> ToggleAction {
    if is_minimized {
        ToggleAction::Unminimize
    } else if !is_visible {
        ToggleAction::Show
    } else if !is_focused {
        ToggleAction::Focus
    } else {
        ToggleAction::Hide
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimized_window_unminimizes() {
        assert_eq!(
            decide_toggle(true, true, false),
            ToggleAction::Unminimize,
        );
    }

    #[test]
    fn minimized_overrides_focus_state() {
        assert_eq!(
            decide_toggle(true, true, true),
            ToggleAction::Unminimize,
        );
    }

    #[test]
    fn hidden_window_shows() {
        assert_eq!(decide_toggle(false, false, false), ToggleAction::Show);
    }

    #[test]
    fn visible_but_unfocused_focuses() {
        assert_eq!(decide_toggle(false, true, false), ToggleAction::Focus);
    }

    #[test]
    fn visible_focused_hides() {
        assert_eq!(decide_toggle(false, true, true), ToggleAction::Hide);
    }

    #[test]
    #[allow(unused_assignments)]
    fn lifecycle_minimize_then_toggle_then_toggle() {
        // Starts hidden.
        let mut is_minimized = false;
        let mut is_visible = false;
        let mut is_focused = false;

        // First toggle: hidden → shown.
        let action = decide_toggle(is_minimized, is_visible, is_focused);
        assert_eq!(action, ToggleAction::Show);
        is_visible = true;
        is_focused = true;

        // User clicks the yellow minimize: window minimizes, focus drops.
        is_minimized = true;
        is_focused = false;

        // Toggle hotkey: minimized → unminimize.
        let action = decide_toggle(is_minimized, is_visible, is_focused);
        assert_eq!(action, ToggleAction::Unminimize);
        is_minimized = false;
        is_focused = true;

        // Toggle hotkey again: visible + focused → hide.
        let action = decide_toggle(is_minimized, is_visible, is_focused);
        assert_eq!(action, ToggleAction::Hide);
    }
}
