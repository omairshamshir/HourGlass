//! The thin macOS layer: system signals, idle detection, and the menu bar item.
//!
//! Everything here runs on the main thread alongside gpui. AppKit callbacks
//! cannot reach into gpui's context while gpui is mid-frame, so they drop a
//! timestamped signal into a queue that the app drains on its own tick. One
//! direction, no locks held across a frame, no reentrancy.

pub mod appearance;
pub mod idle;
pub mod signals;
pub mod tray;

use chrono::{DateTime, Utc};
use std::sync::{Mutex, OnceLock};

/// Something macOS told us, recorded with the moment we heard it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    WillSleep,
    DidWake,
    ScreenLocked,
    ScreenUnlocked,
    /// The user switched macOS between light and dark.
    AppearanceChanged,
}

/// Signals waiting to be drained, each with the moment it arrived.
type SignalQueue = Mutex<Vec<(Signal, DateTime<Utc>)>>;

fn queue() -> &'static SignalQueue {
    static QUEUE: OnceLock<SignalQueue> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Record a signal. Called from AppKit notification callbacks.
pub fn push_signal(signal: Signal) {
    if let Ok(mut pending) = queue().lock() {
        pending.push((signal, Utc::now()));
    }
}

/// Take everything recorded since the last drain, oldest first.
pub fn drain_signals() -> Vec<(Signal, DateTime<Utc>)> {
    queue()
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default()
}

/// Drop the Dock icon and the app menu: Hourglass lives in the menu bar.
///
/// gpui sets the regular activation policy while starting up, so this has to
/// run after the application is alive rather than before.
#[cfg(target_os = "macos")]
pub fn become_menu_bar_app() {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

/// Bring Hourglass to the front. An accessory app has to ask.
#[cfg(target_os = "macos")]
pub fn activate_app() {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    NSApplication::sharedApplication(mtm).activate();
}
