//! The title-bar double-click setting as the system holds it. What it asks of
//! the window is decided in [`writ_core::title_bar`].

use writ_core::title_bar::DoubleClickPreference;

/// Reads `AppleActionOnDoubleClick` and `AppleMiniaturizeOnDoubleClick`
/// through the app's defaults search list, which ends in the global domain
/// System Settings writes them to. Read on every double-click, so a change in
/// System Settings applies without a relaunch.
#[cfg(target_os = "macos")]
#[must_use]
pub fn read_double_click_preference() -> DoubleClickPreference {
    use objc2_foundation::{NSString, NSUserDefaults};
    use writ_core::title_bar::{ACTION_ON_DOUBLE_CLICK_KEY, MINIATURIZE_ON_DOUBLE_CLICK_KEY};

    let defaults = NSUserDefaults::standardUserDefaults();
    let action_key = NSString::from_str(ACTION_ON_DOUBLE_CLICK_KEY);
    let miniaturize_key = NSString::from_str(MINIATURIZE_ON_DOUBLE_CLICK_KEY);
    DoubleClickPreference {
        action: defaults
            .stringForKey(&action_key)
            .map(|value| value.to_string()),
        miniaturize: defaults
            .objectForKey(&miniaturize_key)
            .map(|_| defaults.boolForKey(&miniaturize_key)),
    }
}

/// Windows and Linux keep no setting Writ reads.
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn read_double_click_preference() -> DoubleClickPreference {
    DoubleClickPreference::default()
}
