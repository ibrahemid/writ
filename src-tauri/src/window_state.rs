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

/// The monitor a saved rect belongs to: greatest overlap area among the
/// monitors it overlaps meaningfully on both axes. `None` when the saved rect
/// is effectively off-screen on the current layout (or there are no monitors).
fn best_monitor(saved: Rect, monitors: &[Rect]) -> Option<&Rect> {
    let w = saved.width as i32;
    let h = saved.height as i32;

    monitors
        .iter()
        .map(|m| {
            let ox = axis_overlap(saved.x, w, m.x, m.width as i32);
            let oy = axis_overlap(saved.y, h, m.y, m.height as i32);
            (m, ox, oy)
        })
        .filter(|(_, ox, oy)| *ox >= MIN_VISIBLE_OVERLAP_PX && *oy >= MIN_VISIBLE_OVERLAP_PX)
        .max_by_key(|(_, ox, oy)| i64::from(*ox) * i64::from(*oy))
        .map(|(m, _, _)| m)
}

/// Decide where to restore a window given its saved geometry and the current
/// monitors. Picks the monitor with the greatest overlap; if a meaningfully
/// visible monitor exists the position is clamped fully onto it, otherwise the
/// window is centered so a changed display layout can never reopen it off-screen.
pub fn place_window(saved: Rect, monitors: &[Rect]) -> WindowPlacement {
    match best_monitor(saved, monitors) {
        Some(m) => WindowPlacement::At {
            x: clamp_axis(saved.x, saved.width as i32, m.x, m.width as i32),
            y: clamp_axis(saved.y, saved.height as i32, m.y, m.height as i32),
        },
        None => WindowPlacement::Center,
    }
}

/// Floor for a restored window size, mirroring `minWidth`/`minHeight` in
/// tauri.conf.json (pinned by `minimum_size_constants_match_tauri_conf`).
pub const MIN_LOGICAL_WIDTH: u32 = 720;
pub const MIN_LOGICAL_HEIGHT: u32 = 480;

/// Fit a saved window size to the monitor it will be restored on: never larger
/// than that monitor, never below the app's minimum. Uses the same monitor
/// [`place_window`] would pick; when the saved rect overlaps none of them the
/// window gets centered instead, so `fallback` must be the monitor the caller's
/// centering lands on (the window's current monitor). `monitors.first()` stands
/// in when the caller has no current monitor, and with nothing at all reported
/// the saved size passes through.
pub fn fit_size(
    saved: Rect,
    monitors: &[Rect],
    fallback: Option<Rect>,
    min_width: u32,
    min_height: u32,
) -> (u32, u32) {
    let target = best_monitor(saved, monitors)
        .copied()
        .or(fallback)
        .or_else(|| monitors.first().copied());
    let Some(m) = target else {
        return (saved.width, saved.height);
    };
    (
        saved.width.min(m.width).max(min_width),
        saved.height.min(m.height).max(min_height),
    )
}

/// What restoring a saved window rect resolves to on the current layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestorePlan {
    /// Size to apply, or `None` when nothing usable was saved.
    pub size: Option<(u32, u32)>,
    /// Where to put the window, or `None` when no position was saved (leave the
    /// OS default placement alone).
    pub placement: Option<WindowPlacement>,
}

/// Compose the size fit and the position resolution for a saved window rect.
///
/// The size is fitted only when both axes were saved; the placement is resolved
/// only when both coordinates were saved, and against the fitted size so the
/// clamp matches the window that will actually appear.
#[allow(clippy::too_many_arguments)]
// The saved rect, the layout, and the floors are three separate concerns and
// splitting them into a struct here would only move the argument list.
pub fn plan_restore(
    width: u32,
    height: u32,
    x: Option<i32>,
    y: Option<i32>,
    monitors: &[Rect],
    fallback: Option<Rect>,
    min_width: u32,
    min_height: u32,
) -> RestorePlan {
    let size = if width > 0 && height > 0 {
        let saved = Rect {
            x: x.unwrap_or(0),
            y: y.unwrap_or(0),
            width,
            height,
        };
        Some(fit_size(saved, monitors, fallback, min_width, min_height))
    } else {
        None
    };

    let placement = match (x, y) {
        (Some(x), Some(y)) => {
            let (width, height) = size.unwrap_or((width, height));
            Some(place_window(
                Rect {
                    x,
                    y,
                    width,
                    height,
                },
                monitors,
            ))
        }
        _ => None,
    };

    RestorePlan { size, placement }
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

    // The constants are the floor restore applies; tauri.conf.json is the floor
    // the OS enforces on the live window. Drift between them means a restore
    // that silently fights the window manager.
    #[test]
    fn minimum_size_constants_match_tauri_conf() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("parse tauri.conf.json");
        let main = &conf["app"]["windows"][0];
        assert_eq!(
            main["minWidth"].as_u64(),
            Some(u64::from(MIN_LOGICAL_WIDTH))
        );
        assert_eq!(
            main["minHeight"].as_u64(),
            Some(u64::from(MIN_LOGICAL_HEIGHT))
        );
    }

    #[test]
    fn fit_size_clamps_maximized_rect_to_the_work_area() {
        let work_area = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1032,
        };
        // What a quit-while-maximized run persists on Windows: the frame rect,
        // origin outside the work area and larger than it on both axes.
        let saved = Rect {
            x: -8,
            y: -8,
            width: 1936,
            height: 1048,
        };
        assert_eq!(
            fit_size(
                saved,
                &[work_area],
                None,
                MIN_LOGICAL_WIDTH,
                MIN_LOGICAL_HEIGHT
            ),
            (1920, 1032)
        );
    }

    #[test]
    fn fit_size_leaves_a_rect_that_already_fits() {
        let work_area = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1032,
        };
        let saved = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1032,
        };
        assert_eq!(
            fit_size(
                saved,
                &[work_area],
                None,
                MIN_LOGICAL_WIDTH,
                MIN_LOGICAL_HEIGHT
            ),
            (1920, 1032)
        );
    }

    #[test]
    fn fit_size_floors_a_sub_minimum_rect_at_the_minimums() {
        let work_area = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1032,
        };
        let saved = Rect {
            x: 100,
            y: 100,
            width: 320,
            height: 200,
        };
        assert_eq!(
            fit_size(
                saved,
                &[work_area],
                None,
                MIN_LOGICAL_WIDTH,
                MIN_LOGICAL_HEIGHT
            ),
            (720, 480)
        );
    }

    #[test]
    fn fit_size_passes_through_with_no_monitors_and_no_fallback() {
        let saved = Rect {
            x: 0,
            y: 0,
            width: 4000,
            height: 3000,
        };
        assert_eq!(
            fit_size(saved, &[], None, MIN_LOGICAL_WIDTH, MIN_LOGICAL_HEIGHT),
            (4000, 3000)
        );
    }

    // Nothing overlaps, so the window gets centered — on the monitor it is
    // currently on, which is not necessarily the first one reported. Clamping
    // against monitors[0] here can still hand back a size larger than the
    // display the window lands on.
    #[test]
    fn fit_size_clamps_against_the_fallback_monitor_when_nothing_overlaps() {
        let first = Rect {
            x: 0,
            y: 0,
            width: 3840,
            height: 2100,
        };
        let current = Rect {
            x: 3840,
            y: 0,
            width: 1440,
            height: 860,
        };
        let saved = Rect {
            x: -4000,
            y: 200,
            width: 2560,
            height: 1400,
        };
        assert_eq!(
            fit_size(
                saved,
                &[first, current],
                Some(current),
                MIN_LOGICAL_WIDTH,
                MIN_LOGICAL_HEIGHT
            ),
            (1440, 860)
        );
    }

    // Without a current monitor the first one is the only guess available.
    #[test]
    fn fit_size_clamps_against_the_first_monitor_when_there_is_no_fallback() {
        let first = Rect {
            x: 0,
            y: 0,
            width: 1440,
            height: 860,
        };
        let saved = Rect {
            x: 4000,
            y: 200,
            width: 2560,
            height: 1400,
        };
        assert_eq!(
            fit_size(saved, &[first], None, MIN_LOGICAL_WIDTH, MIN_LOGICAL_HEIGHT),
            (1440, 860)
        );
    }

    #[test]
    fn fit_size_uses_the_monitor_the_window_lands_on() {
        let left = Rect {
            x: 0,
            y: 0,
            width: 3840,
            height: 2100,
        };
        let right = Rect {
            x: 3840,
            y: 0,
            width: 1280,
            height: 800,
        };
        let saved = Rect {
            x: 3900,
            y: 100,
            width: 2000,
            height: 1200,
        };
        assert_eq!(
            fit_size(
                saved,
                &[left, right],
                None,
                MIN_LOGICAL_WIDTH,
                MIN_LOGICAL_HEIGHT
            ),
            (1280, 800)
        );
    }

    #[test]
    fn fit_size_prefers_the_overlapped_monitor_over_the_fallback() {
        let left = Rect {
            x: 0,
            y: 0,
            width: 3840,
            height: 2100,
        };
        let right = Rect {
            x: 3840,
            y: 0,
            width: 1280,
            height: 800,
        };
        let saved = Rect {
            x: 3900,
            y: 100,
            width: 2000,
            height: 1200,
        };
        assert_eq!(
            fit_size(
                saved,
                &[left, right],
                Some(left),
                MIN_LOGICAL_WIDTH,
                MIN_LOGICAL_HEIGHT
            ),
            (1280, 800)
        );
    }

    // The rect a quit-while-maximized Windows run persists: oversized, origin
    // outside the work area. Restore has to shrink it to the work area and pull
    // the origin back inside, or the window reopens off-screen and too big.
    #[test]
    fn plan_restore_fits_and_clamps_a_maximized_windows_rect() {
        let work_area = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1032,
        };
        let plan = plan_restore(
            1936,
            1048,
            Some(-8),
            Some(-8),
            &[work_area],
            None,
            MIN_LOGICAL_WIDTH,
            MIN_LOGICAL_HEIGHT,
        );
        assert_eq!(plan.size, Some((1920, 1032)));
        assert_eq!(plan.placement, Some(WindowPlacement::At { x: 0, y: 0 }));
    }

    #[test]
    fn plan_restore_leaves_placement_alone_without_a_saved_position() {
        let work_area = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1032,
        };
        let plan = plan_restore(
            1100,
            720,
            None,
            None,
            &[work_area],
            None,
            MIN_LOGICAL_WIDTH,
            MIN_LOGICAL_HEIGHT,
        );
        assert_eq!(plan.size, Some((1100, 720)));
        assert_eq!(plan.placement, None);
    }

    #[test]
    fn plan_restore_places_a_saved_position_with_no_saved_size() {
        let work_area = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1032,
        };
        // A zero-size rect overlaps nothing, so the position is unusable and the
        // window centers — same as leaving both untouched to the OS default.
        let plan = plan_restore(
            0,
            0,
            Some(200),
            Some(150),
            &[work_area],
            None,
            MIN_LOGICAL_WIDTH,
            MIN_LOGICAL_HEIGHT,
        );
        assert_eq!(plan.size, None);
        assert_eq!(plan.placement, Some(WindowPlacement::Center));
    }

    // Fit and place have to agree on which monitor the window lands on: the
    // saved rect overlaps neither display, so it centers on the current one and
    // the size must be clamped against that same display.
    #[test]
    fn plan_restore_sizes_against_the_fallback_it_will_center_on() {
        let first = Rect {
            x: 0,
            y: 0,
            width: 3840,
            height: 2100,
        };
        let current = Rect {
            x: 3840,
            y: 0,
            width: 1440,
            height: 860,
        };
        let plan = plan_restore(
            2560,
            1400,
            Some(-4000),
            Some(200),
            &[first, current],
            Some(current),
            MIN_LOGICAL_WIDTH,
            MIN_LOGICAL_HEIGHT,
        );
        assert_eq!(plan.size, Some((1440, 860)));
        assert_eq!(plan.placement, Some(WindowPlacement::Center));
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
