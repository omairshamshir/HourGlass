//! Hourglass domain logic: what the timer does, and what the numbers mean.
//!
//! Nothing here touches SQLite, AppKit, or the GPU. The app crate wires this to
//! the machine; this crate can be reasoned about and tested on its own.

pub mod manual;
pub mod model;
pub mod report;
pub mod timer;

pub use manual::{DraftError, first_conflict, parse_date, parse_time, resolve_span};
pub use model::{
    EntryId, Project, ProjectId, Settings, StopReason, ThemeChoice, TimeEntry, format_clock,
    format_decimal_hours, format_duration,
};
pub use report::{
    DateRange, RangeKind, Report, ReportEntry, ProjectTotal, build_report, csv_filename, local_day,
    local_month, local_week, range_for, report_to_csv, seconds_within,
};
pub use timer::{Effect, Paused, Session, SystemEvent, Timer, idle_threshold};
