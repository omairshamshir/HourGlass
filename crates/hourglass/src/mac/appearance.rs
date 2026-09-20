//! Which appearance macOS is currently in.
//!
//! Read from `AppleInterfaceStyle` in the standard defaults, which is the
//! documented way to ask and needs no permission. The key is absent in light
//! mode and holds "Dark" otherwise — there is no "Light" value to compare
//! against, so absence is the light case.

/// True when macOS is in dark mode.
#[cfg(target_os = "macos")]
pub fn system_is_dark() -> bool {
    use objc2_foundation::{NSString, NSUserDefaults};

    let defaults = NSUserDefaults::standardUserDefaults();
    let key = NSString::from_str("AppleInterfaceStyle");
    defaults
        .stringForKey(&key)
        .map(|style| style.to_string().eq_ignore_ascii_case("dark"))
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
pub fn system_is_dark() -> bool {
    false
}
