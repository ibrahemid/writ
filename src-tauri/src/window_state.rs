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

/// A rectangle in logical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Resolution of a saved window position against the current display layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowPlacement {
    /// Place the window's top-left at these logical coordinates.
    At { x: i32, y: i32 },
    /// The saved position is unusable on the current layout (off-screen or on
    /// a disconnected monitor); center on the OS default instead.
    Center,
}

/// Minimum per-axis overlap with a monitor for a saved window to count as
/// visible. Sized to a title-bar strip so the window stays grabbable; below
/// this the monitor is treated as effectively disconnected.
const MIN_VISIBLE_OVERLAP_PX: i32 = 48;

/// Length of the intersection between two 1-D segments, clamped at zero.
fn axis_overlap(a_pos: i32, a_len: i32, b_pos: i32, b_len: i32) -> i32 {
    let start = a_pos.max(b_pos);
    let end = (a_pos + a_len).min(b_pos + b_len);
    (end - start).max(0)
}

/// Slide `pos` so a segment of length `len` sits within `[mon_pos, mon_pos + mon_len)`.
/// A segment longer than the monitor is pinned to the monitor origin.
fn clamp_axis(pos: i32, len: i32, mon_pos: i32, mon_len: i32) -> i32 {
    if len >= mon_len {
        mon_pos
    } else {
        pos.clamp(mon_pos, mon_pos + mon_len - len)
    }
}

/// Convert a physical monitor rectangle to logical pixels using its scale factor.
pub fn logical_rect(px: i32, py: i32, pw: u32, ph: u32, scale: f64) -> Rect {
    let s = if scale > 0.0 { scale } else { 1.0 };
    Rect {
        x: (f64::from(px) / s).round() as i32,
        y: (f64::from(py) / s).round() as i32,
        width: (f64::from(pw) / s).round().max(0.0) as u32,
        height: (f64::from(ph) / s).round().max(0.0) as u32,
    }
}

/// Decide where to restore a window given its saved geometry and the current
/// monitors. Picks the monitor with the greatest overlap; if a meaningfully
/// visible monitor exists the position is clamped fully onto it, otherwise the
/// window is centered so a changed display layout can never reopen it off-screen.
pub fn place_window(saved: Rect, monitors: &[Rect]) -> WindowPlacement {
    let w = saved.width as i32;
    let h = saved.height as i32;

    let best = monitors
        .iter()
        .map(|m| {
            let ox = axis_overlap(saved.x, w, m.x, m.width as i32);
            let oy = axis_overlap(saved.y, h, m.y, m.height as i32);
            (m, ox, oy)
        })
        .filter(|(_, ox, oy)| *ox >= MIN_VISIBLE_OVERLAP_PX && *oy >= MIN_VISIBLE_OVERLAP_PX)
        .max_by_key(|(_, ox, oy)| i64::from(*ox) * i64::from(*oy));

    match best {
        Some((m, _, _)) => WindowPlacement::At {
            x: clamp_axis(saved.x, w, m.x, m.width as i32),
            y: clamp_axis(saved.y, h, m.y, m.height as i32),
        },
        None => WindowPlacement::Center,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimized_window_unminimizes() {
        assert_eq!(decide_toggle(true, true, false), ToggleAction::Unminimize,);
    }

    #[test]
    fn minimized_overrides_focus_state() {
        assert_eq!(decide_toggle(true, true, true), ToggleAction::Unminimize,);
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
    fn place_window_centers_when_no_monitors() {
        let saved = Rect {
            x: 100,
            y: 100,
            width: 800,
            height: 600,
        };
        assert_eq!(place_window(saved, &[]), WindowPlacement::Center);
    }

    #[test]
    fn place_window_keeps_fully_visible_position() {
        let mon = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let saved = Rect {
            x: 200,
            y: 150,
            width: 800,
            height: 600,
        };
        assert_eq!(
            place_window(saved, &[mon]),
            WindowPlacement::At { x: 200, y: 150 }
        );
    }

    #[test]
    fn place_window_clamps_partially_offscreen_window_onto_monitor() {
        let mon = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let saved = Rect {
            x: 1600,
            y: 900,
            width: 800,
            height: 600,
        };
        assert_eq!(
            place_window(saved, &[mon]),
            WindowPlacement::At { x: 1120, y: 480 },
        );
    }

    #[test]
    fn place_window_centers_when_saved_on_disconnected_monitor() {
        let mon = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let saved = Rect {
            x: 3000,
            y: 200,
            width: 800,
            height: 600,
        };
        assert_eq!(place_window(saved, &[mon]), WindowPlacement::Center);
    }

    #[test]
    fn place_window_pins_oversized_window_to_monitor_origin() {
        let mon = Rect {
            x: 0,
            y: 0,
            width: 1280,
            height: 800,
        };
        let saved = Rect {
            x: -50,
            y: -50,
            width: 1600,
            height: 1000,
        };
        assert_eq!(
            place_window(saved, &[mon]),
            WindowPlacement::At { x: 0, y: 0 }
        );
    }

    #[test]
    fn place_window_picks_monitor_with_greatest_overlap() {
        let left = Rect {
            x: 0,
            y: 0,
            width: 1440,
            height: 900,
        };
        let right = Rect {
            x: 1440,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let saved = Rect {
            x: 1500,
            y: 100,
            width: 800,
            height: 600,
        };
        assert_eq!(
            place_window(saved, &[left, right]),
            WindowPlacement::At { x: 1500, y: 100 },
        );
    }

    #[test]
    fn logical_rect_converts_physical_using_scale() {
        let r = logical_rect(2880, 0, 3840, 2160, 2.0);
        assert_eq!(
            r,
            Rect {
                x: 1440,
                y: 0,
                width: 1920,
                height: 1080
            }
        );
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
