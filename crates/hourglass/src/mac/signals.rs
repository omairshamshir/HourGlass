//! Sleep, wake, and screen-lock notifications from macOS.
//!
//! Sleep and wake come from `NSWorkspace`. Lock and unlock are only published
//! on the distributed notification centre, so both centres are observed.

use crate::mac::{Signal, push_signal};

/// Holds the observer tokens. Dropping this stops the app from being told
/// about sleep, so the app keeps it alive for its whole run.
pub struct SystemSignals {
    #[cfg(target_os = "macos")]
    _tokens: Vec<objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>>,
}

#[cfg(target_os = "macos")]
impl SystemSignals {
    /// Subscribe to every signal that means "the user stepped away" or "the
    /// user is back".
    pub fn install() -> Self {
        use block2::RcBlock;
        use objc2_app_kit::{
            NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceScreensDidSleepNotification,
            NSWorkspaceScreensDidWakeNotification, NSWorkspaceWillSleepNotification,
        };
        use objc2_foundation::{
            NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSString,
        };
        use std::ptr::NonNull;

        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        let distributed = NSDistributedNotificationCenter::defaultCenter();
        let mut tokens = Vec::new();

        let mut observe = |center: &NSNotificationCenter, name: &NSString, signal: Signal| {
            let block = RcBlock::new(move |_: NonNull<NSNotification>| push_signal(signal));
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
            };
            tokens.push(token);
        };

        // The notification names are extern statics, hence the unsafe reads.
        let will_sleep = unsafe { NSWorkspaceWillSleepNotification };
        let did_wake = unsafe { NSWorkspaceDidWakeNotification };
        let screens_slept = unsafe { NSWorkspaceScreensDidSleepNotification };
        let screens_woke = unsafe { NSWorkspaceScreensDidWakeNotification };

        observe(&workspace, will_sleep, Signal::WillSleep);
        observe(&workspace, did_wake, Signal::DidWake);
        // A display going to sleep is the same story as the machine sleeping:
        // nobody is looking at the screen.
        observe(&workspace, screens_slept, Signal::ScreenLocked);
        observe(&workspace, screens_woke, Signal::ScreenUnlocked);

        // Lock and unlock are private-but-stable distributed notifications;
        // there is no public API for them.
        observe(
            &distributed,
            &NSString::from_str("com.apple.screenIsLocked"),
            Signal::ScreenLocked,
        );
        observe(
            &distributed,
            &NSString::from_str("com.apple.screenIsUnlocked"),
            Signal::ScreenUnlocked,
        );

        // Light/dark switches, so a machine that changes at sunset takes the
        // window with it without the app polling for it.
        observe(
            &distributed,
            &NSString::from_str("AppleInterfaceThemeChangedNotification"),
            Signal::AppearanceChanged,
        );

        SystemSignals { _tokens: tokens }
    }
}

#[cfg(not(target_os = "macos"))]
impl SystemSignals {
    pub fn install() -> Self {
        SystemSignals {}
    }
}
