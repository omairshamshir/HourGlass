//! Idle detection through the window server's input clock.
//!
//! `CGEventSourceSecondsSinceLastEventType` reports how long the machine has
//! gone without keyboard or mouse input. Reading it needs no Accessibility
//! permission, which matters: a time tracker that demands input monitoring on
//! first launch is a time tracker people delete.

use chrono::{DateTime, Duration, Utc};
use hourglass_core::timer::SystemEvent;

/// Watches the input clock and reports the two moments that matter: going
/// quiet, and coming back.
pub struct IdleWatcher {
    idle: bool,
}

impl IdleWatcher {
    pub fn new() -> Self {
        IdleWatcher { idle: false }
    }

    /// Check the input clock. Returns an event only on a change of state, so
    /// this can be called several times a second for free.
    pub fn poll(&mut self, threshold: Duration, now: DateTime<Utc>) -> Option<SystemEvent> {
        let quiet = Duration::seconds(seconds_since_last_input() as i64);

        if !self.idle && quiet >= threshold {
            self.idle = true;
            // Report the moment input actually stopped, not the moment we
            // noticed: the difference is the whole idle threshold.
            return Some(SystemEvent::WentIdle { since: now - quiet });
        }

        if self.idle && quiet < threshold {
            self.idle = false;
            return Some(SystemEvent::BecameActive { at: now });
        }

        None
    }

    /// True while the machine is considered idle.
    pub fn is_idle(&self) -> bool {
        self.idle
    }
}

impl Default for IdleWatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Seconds since the last human input of any kind.
#[cfg(target_os = "macos")]
fn seconds_since_last_input() -> f64 {
    use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};

    // kCGAnyInputEventType: any event at all, rather than one specific kind.
    const ANY_INPUT: CGEventType = CGEventType(u32::MAX);

    CGEventSource::seconds_since_last_event_type(CGEventSourceStateID::HIDSystemState, ANY_INPUT)
        .max(0.0)
}

#[cfg(not(target_os = "macos"))]
fn seconds_since_last_input() -> f64 {
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_watcher_is_not_idle() {
        assert!(!IdleWatcher::new().is_idle());
    }

    #[test]
    fn a_threshold_beyond_the_current_reading_produces_no_events() {
        // How long the human has been away is not something a test can know,
        // and a fixed ten minutes fails whenever the machine is left locked.
        // Set the threshold past whatever the reading actually is instead.
        let mut watcher = IdleWatcher::new();
        let beyond = Duration::seconds(seconds_since_last_input() as i64 + 60);
        assert_eq!(watcher.poll(beyond, Utc::now()), None);
    }

    #[test]
    fn a_zero_threshold_reports_idle_once_then_stays_quiet() {
        let mut watcher = IdleWatcher::new();
        let now = Utc::now();
        assert!(matches!(
            watcher.poll(Duration::zero(), now),
            Some(SystemEvent::WentIdle { .. })
        ));
        assert!(watcher.is_idle());
        assert_eq!(watcher.poll(Duration::zero(), now), None);
    }
}
