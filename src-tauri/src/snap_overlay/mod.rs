//! Windows 11 snap-layout support for the custom caption maximize button.
//!
//! The window runs with `decorations: false`, so Windows has no caption of its
//! own to hit-test and the snap-layout flyout never appears. It appears only
//! when a window answers `HTMAXBUTTON` for the cursor position. A subclass on
//! the top-level window cannot answer it: the WebView2 child covers the client
//! area and claims the hit test before the frame ever sees it
//! (WebView2Feedback #446, tauri#4531). What does work is a hit-test-only child
//! window placed over the button, a sibling of the WebView2 child kept above it
//! in z-order; the geometry comes from the frontend, which owns the layout.
//!
//! This module holds the platform-independent half: the metrics the frontend
//! reports and the logical-to-physical geometry. [`win`] holds the Win32 half.

#[cfg(target_os = "windows")]
mod win;

use serde::Deserialize;

/// Upper bound for a reported metric, in logical pixels. Nothing on a caption
/// bar is anywhere near this; a larger value means the frontend measured a
/// detached or mis-laid-out node and the overlay would land off-window.
const MAX_METRIC_PX: f64 = 10_000.0;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CaptionMetricsError {
    #[error("caption button {field} must be a finite number, got {value}")]
    NotFinite { field: &'static str, value: f64 },
    #[error("caption button {field} must not be negative, got {value}")]
    Negative { field: &'static str, value: f64 },
    #[error("caption button {field} must be under {MAX_METRIC_PX} logical px, got {value}")]
    TooLarge { field: &'static str, value: f64 },
    #[error("caption button {field} must be greater than zero")]
    Empty { field: &'static str },
}

/// The maximize button's box in logical (CSS) pixels, as the frontend measures
/// it.
///
/// Horizontal position is carried as the distance from the window's right edge
/// rather than an absolute left, which makes it invariant under window resize:
/// the caption controls are right-anchored, so one report survives every later
/// resize and DPI change.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionButtonMetrics {
    pub offset_from_right: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

impl CaptionButtonMetrics {
    pub fn validate(self) -> Result<Self, CaptionMetricsError> {
        for (field, value) in [
            ("offsetFromRight", self.offset_from_right),
            ("top", self.top),
            ("width", self.width),
            ("height", self.height),
        ] {
            if !value.is_finite() {
                return Err(CaptionMetricsError::NotFinite { field, value });
            }
            if value < 0.0 {
                return Err(CaptionMetricsError::Negative { field, value });
            }
            if value > MAX_METRIC_PX {
                return Err(CaptionMetricsError::TooLarge { field, value });
            }
        }
        for (field, value) in [("width", self.width), ("height", self.height)] {
            if value == 0.0 {
                return Err(CaptionMetricsError::Empty { field });
            }
        }
        Ok(self)
    }
}

/// A physical-pixel rect in the parent window's client coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

fn scale(logical: f64, dpi: u32) -> i32 {
    (logical * f64::from(dpi) / 96.0).round() as i32
}

/// Places the overlay over the maximize button for a client area
/// `client_width` physical pixels wide at `dpi`.
///
/// Each edge is scaled from its own logical distance and the extent is taken as
/// the difference. Scaling the extents directly would let two independent
/// roundings stack, leaving a one-pixel seam beside the button at fractional
/// scales (125%, 150%) where the flyout is easiest to miss.
pub fn overlay_rect(client_width: i32, dpi: u32, metrics: CaptionButtonMetrics) -> OverlayRect {
    let right = client_width - scale(metrics.offset_from_right, dpi);
    let left = client_width - scale(metrics.offset_from_right + metrics.width, dpi);
    let top = scale(metrics.top, dpi);
    let bottom = scale(metrics.top + metrics.height, dpi);

    let x = left.max(0);
    OverlayRect {
        x,
        y: top,
        width: (right - x).max(0),
        height: (bottom - top).max(0),
    }
}

/// Creates the overlay if it does not exist yet, then positions it over the
/// button. Safe to call repeatedly; later calls reposition the same window.
#[cfg(target_os = "windows")]
pub fn install(window: &tauri::WebviewWindow, metrics: CaptionButtonMetrics) -> Result<(), String> {
    win::install(window, metrics)
}

/// No snap layouts outside Windows; the command stays callable everywhere so
/// the frontend has one code path.
#[cfg(not(target_os = "windows"))]
pub fn install(
    _window: &tauri::WebviewWindow,
    _metrics: CaptionButtonMetrics,
) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> CaptionButtonMetrics {
        CaptionButtonMetrics {
            offset_from_right: 46.0,
            top: 0.0,
            width: 46.0,
            height: 36.0,
        }
    }

    #[test]
    fn validate_accepts_a_measured_caption_button() {
        assert_eq!(metrics().validate(), Ok(metrics()));
    }

    #[test]
    fn validate_rejects_non_finite_values() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let m = CaptionButtonMetrics {
                width: bad,
                ..metrics()
            };
            // Compared field by field: NaN is never equal to itself, so the
            // error value cannot be matched with assert_eq.
            match m.validate() {
                Err(CaptionMetricsError::NotFinite { field, value }) => {
                    assert_eq!(field, "width");
                    assert!(value.is_nan() || value.is_infinite());
                }
                other => panic!("expected a non-finite rejection, got {other:?}"),
            }
        }
    }

    #[test]
    fn validate_rejects_negative_values() {
        let m = CaptionButtonMetrics {
            offset_from_right: -1.0,
            ..metrics()
        };
        assert_eq!(
            m.validate(),
            Err(CaptionMetricsError::Negative {
                field: "offsetFromRight",
                value: -1.0
            })
        );
    }

    #[test]
    fn validate_rejects_absurd_values() {
        let m = CaptionButtonMetrics {
            height: 10_001.0,
            ..metrics()
        };
        assert_eq!(
            m.validate(),
            Err(CaptionMetricsError::TooLarge {
                field: "height",
                value: 10_001.0
            })
        );
    }

    #[test]
    fn validate_rejects_a_zero_sized_button() {
        for m in [
            CaptionButtonMetrics {
                width: 0.0,
                ..metrics()
            },
            CaptionButtonMetrics {
                height: 0.0,
                ..metrics()
            },
        ] {
            assert!(matches!(
                m.validate(),
                Err(CaptionMetricsError::Empty { .. })
            ));
        }
    }

    #[test]
    fn validate_accepts_a_zero_offset_button_at_the_window_edge() {
        let m = CaptionButtonMetrics {
            offset_from_right: 0.0,
            ..metrics()
        };
        assert!(m.validate().is_ok());
    }

    #[test]
    fn overlay_rect_at_100_percent_sits_flush_with_the_right_edge() {
        let m = CaptionButtonMetrics {
            offset_from_right: 0.0,
            ..metrics()
        };
        assert_eq!(
            overlay_rect(1200, 96, m),
            OverlayRect {
                x: 1154,
                y: 0,
                width: 46,
                height: 36
            }
        );
    }

    #[test]
    fn overlay_rect_offsets_left_by_the_buttons_to_its_right() {
        assert_eq!(
            overlay_rect(1200, 96, metrics()),
            OverlayRect {
                x: 1108,
                y: 0,
                width: 46,
                height: 36
            }
        );
    }

    #[test]
    fn overlay_rect_scales_with_dpi() {
        let m = CaptionButtonMetrics {
            offset_from_right: 0.0,
            ..metrics()
        };
        let scaled = overlay_rect(2400, 192, m);
        assert_eq!(
            scaled,
            OverlayRect {
                x: 2308,
                y: 0,
                width: 92,
                height: 72
            }
        );
    }

    #[test]
    fn overlay_rect_keeps_the_button_whole_at_fractional_scaling() {
        // 150%: the neighbouring buttons scale to 69px each. Deriving the
        // extent from the two scaled edges keeps the run gapless, which
        // scaling the width on its own would not guarantee.
        let rect = overlay_rect(1800, 144, metrics());
        assert_eq!(rect.width, 69);
        assert_eq!(rect.x + rect.width, 1800 - 69);
    }

    #[test]
    fn overlay_rect_clamps_when_the_window_is_narrower_than_the_button() {
        let rect = overlay_rect(20, 96, metrics());
        assert_eq!(rect.x, 0);
        assert_eq!(rect.width, 0);
    }
}
