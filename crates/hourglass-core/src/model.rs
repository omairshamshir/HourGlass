//! Value types shared by the timer, the store, and the UI.

use chrono::{DateTime, Utc};
use std::fmt;

/// Stable identifier for a project row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectId(pub i64);

/// Stable identifier for a time entry row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId(pub i64);

/// Why a time entry stopped. Anything other than `Manual` or `Switch` means the
/// machine decided the user had stepped away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The user pressed stop.
    Manual,
    /// The user started a different project while this one was running.
    Switch,
    /// The machine went to sleep.
    Sleep,
    /// No keyboard or mouse input for longer than the idle threshold.
    Idle,
    /// The screen locked or the screen saver started.
    Lock,
}

impl StopReason {
    /// Text stored in SQLite and shown in the entry list.
    pub fn as_str(self) -> &'static str {
        match self {
            StopReason::Manual => "manual",
            StopReason::Switch => "switch",
            StopReason::Sleep => "sleep",
            StopReason::Idle => "idle",
            StopReason::Lock => "lock",
        }
    }

    /// Parse a reason previously written by [`StopReason::as_str`].
    /// Unknown values read back as `Manual` so an old database still opens.
    pub fn from_str_lossy(value: &str) -> Self {
        match value {
            "switch" => StopReason::Switch,
            "sleep" => StopReason::Sleep,
            "idle" => StopReason::Idle,
            "lock" => StopReason::Lock,
            _ => StopReason::Manual,
        }
    }

    /// True when the app stopped the timer on the user's behalf.
    pub fn is_automatic(self) -> bool {
        matches!(self, StopReason::Sleep | StopReason::Idle | StopReason::Lock)
    }
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            StopReason::Manual => "Stopped",
            StopReason::Switch => "Switched",
            StopReason::Sleep => "Slept",
            StopReason::Idle => "Idle",
            StopReason::Lock => "Locked",
        };
        f.write_str(label)
    }
}

/// Which appearance the user asked for.
///
/// `System` is the default and follows macOS, so a machine that switches at
/// sunset takes the app with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    Light,
    Dark,
    #[default]
    System,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] =
        [ThemeChoice::Light, ThemeChoice::Dark, ThemeChoice::System];

    /// Text stored in SQLite.
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
            ThemeChoice::System => "system",
        }
    }

    /// Parse a choice previously written by [`ThemeChoice::as_str`]. Anything
    /// unrecognised falls back to following the system.
    pub fn from_str_lossy(value: &str) -> Self {
        match value {
            "light" => ThemeChoice::Light,
            "dark" => ThemeChoice::Dark,
            _ => ThemeChoice::System,
        }
    }

    /// How the choice reads in the window.
    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
            ThemeChoice::System => "Auto",
        }
    }
}

/// A project the user tracks time against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    /// Index into the app's swatch palette, kept small so the DB stays portable.
    pub color: u8,
    /// Archived projects keep their history but leave the start menu.
    pub archived: bool,
    pub created_at: DateTime<Utc>,
}

/// One recorded stretch of work. `ended_at` is `None` while the timer runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeEntry {
    pub id: EntryId,
    pub project_id: ProjectId,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub stop_reason: Option<StopReason>,
}

impl TimeEntry {
    /// Seconds recorded so far, measured against `now` while still running.
    pub fn seconds_at(&self, now: DateTime<Utc>) -> i64 {
        let end = self.ended_at.unwrap_or(now);
        (end - self.started_at).num_seconds().max(0)
    }

    /// True when this entry is the open one.
    pub fn is_running(&self) -> bool {
        self.ended_at.is_none()
    }
}

/// User-tunable behaviour, persisted in the `settings` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Minutes without input before the timer pauses itself.
    pub idle_minutes: u32,
    /// A pause shorter than this resumes on its own instead of asking.
    pub auto_resume_under_secs: u32,
    /// Light, dark, or follow the system.
    pub theme: ThemeChoice,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            idle_minutes: 10,
            auto_resume_under_secs: 120,
            theme: ThemeChoice::System,
        }
    }
}

/// Render seconds as `H:MM:SS`, the menu bar and timer readout format.
pub fn format_clock(total_seconds: i64) -> String {
    let seconds = total_seconds.max(0);
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{hours}:{minutes:02}:{secs:02}")
}

/// Render seconds as `4h 05m`, the report and list format.
pub fn format_duration(total_seconds: i64) -> String {
    let seconds = total_seconds.max(0);
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours == 0 {
        format!("{minutes}m")
    } else {
        format!("{hours}h {minutes:02}m")
    }
}

/// Render seconds as decimal hours, the unit invoices use.
pub fn format_decimal_hours(total_seconds: i64) -> String {
    format!("{:.2}", total_seconds.max(0) as f64 / 3600.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_pads_minutes_and_seconds() {
        assert_eq!(format_clock(0), "0:00:00");
        assert_eq!(format_clock(61), "0:01:01");
        assert_eq!(format_clock(3_671), "1:01:11");
        assert_eq!(format_clock(-5), "0:00:00");
    }

    #[test]
    fn duration_drops_the_hour_when_there_is_none() {
        assert_eq!(format_duration(0), "0m");
        assert_eq!(format_duration(1_800), "30m");
        assert_eq!(format_duration(14_700), "4h 05m");
    }

    #[test]
    fn decimal_hours_round_to_two_places() {
        assert_eq!(format_decimal_hours(1_800), "0.50");
        assert_eq!(format_decimal_hours(14_700), "4.08");
    }

    #[test]
    fn stop_reasons_round_trip_through_text() {
        for reason in [
            StopReason::Manual,
            StopReason::Switch,
            StopReason::Sleep,
            StopReason::Idle,
            StopReason::Lock,
        ] {
            assert_eq!(StopReason::from_str_lossy(reason.as_str()), reason);
        }
        assert_eq!(StopReason::from_str_lossy("nonsense"), StopReason::Manual);
    }

    #[test]
    fn theme_choices_round_trip_and_default_to_following_the_system() {
        for choice in ThemeChoice::ALL {
            assert_eq!(ThemeChoice::from_str_lossy(choice.as_str()), choice);
        }
        assert_eq!(ThemeChoice::from_str_lossy("chartreuse"), ThemeChoice::System);
        assert_eq!(ThemeChoice::default(), ThemeChoice::System);
    }
}
