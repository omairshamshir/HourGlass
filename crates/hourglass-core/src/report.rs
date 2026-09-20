//! Turning entries into totals: day ranges, per-project sums, and CSV.
//!
//! Ranges are local-time questions ("what did I do today?") answered against
//! UTC storage, so every boundary is computed in the local zone and converted.

use crate::model::{
    EntryId, Project, ProjectId, StopReason, TimeEntry, format_decimal_hours, format_duration,
};
use chrono::{DateTime, Datelike, Days, Local, Months, NaiveDate, TimeZone, Utc};
use std::cmp::Reverse;

/// Which stretch of time a report covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeKind {
    Today,
    /// Monday through Sunday containing the reference day.
    Week,
    /// The calendar month containing the reference day.
    Month,
}

impl RangeKind {
    pub fn label(self) -> &'static str {
        match self {
            RangeKind::Today => "Today",
            RangeKind::Week => "This week",
            RangeKind::Month => "This month",
        }
    }

    pub const ALL: [RangeKind; 3] = [RangeKind::Today, RangeKind::Week, RangeKind::Month];
}

/// A half-open instant range `[start, end)` in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl DateRange {
    pub fn contains(&self, instant: DateTime<Utc>) -> bool {
        instant >= self.start && instant < self.end
    }
}

/// Midnight-to-midnight in the local zone for the day containing `instant`.
pub fn local_day(instant: DateTime<Utc>) -> DateRange {
    let day = instant.with_timezone(&Local).date_naive();
    day_span(day, day.succ_opt().unwrap_or(day))
}

/// The local week (Monday start) containing `instant`.
pub fn local_week(instant: DateTime<Utc>) -> DateRange {
    let today = instant.with_timezone(&Local).date_naive();
    let back = today.weekday().num_days_from_monday() as u64;
    let monday = today.checked_sub_days(Days::new(back)).unwrap_or(today);
    let next_monday = monday.checked_add_days(Days::new(7)).unwrap_or(monday);
    day_span(monday, next_monday)
}

/// The local calendar month containing `instant`.
pub fn local_month(instant: DateTime<Utc>) -> DateRange {
    let today = instant.with_timezone(&Local).date_naive();
    let first = today.with_day(1).unwrap_or(today);
    let next = first.checked_add_months(Months::new(1)).unwrap_or(first);
    day_span(first, next)
}

/// The range a [`RangeKind`] refers to, relative to `now`.
pub fn range_for(kind: RangeKind, now: DateTime<Utc>) -> DateRange {
    match kind {
        RangeKind::Today => local_day(now),
        RangeKind::Week => local_week(now),
        RangeKind::Month => local_month(now),
    }
}

/// Convert two local dates into a UTC instant range.
///
/// Local midnight can be skipped by a daylight-saving jump; when it is, the
/// first valid instant of that local day is used instead of failing.
fn day_span(start: NaiveDate, end: NaiveDate) -> DateRange {
    DateRange {
        start: local_midnight(start),
        end: local_midnight(end),
    }
}

fn local_midnight(date: NaiveDate) -> DateTime<Utc> {
    let midnight = date.and_hms_opt(0, 0, 0).expect("midnight is a valid time");
    match Local.from_local_datetime(&midnight).earliest() {
        Some(local) => local.with_timezone(&Utc),
        // Daylight saving removed 00:00 on this date; step forward to a real hour.
        None => (1..=3)
            .find_map(|hour| {
                let candidate = date.and_hms_opt(hour, 0, 0)?;
                Local
                    .from_local_datetime(&candidate)
                    .earliest()
                    .map(|local| local.with_timezone(&Utc))
            })
            .unwrap_or_else(|| Utc.from_utc_datetime(&midnight)),
    }
}

/// Seconds of one entry that fall inside `range`, clipping at both edges so a
/// session spanning midnight is split across days instead of double counted.
pub fn seconds_within(entry: &TimeEntry, range: DateRange, now: DateTime<Utc>) -> i64 {
    let start = entry.started_at.max(range.start);
    let end = entry.ended_at.unwrap_or(now).min(range.end);
    (end - start).num_seconds().max(0)
}

/// One project's share of a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTotal {
    pub project_id: ProjectId,
    pub name: String,
    pub color: u8,
    pub seconds: i64,
}

/// A row shown in the report's entry list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportEntry {
    pub id: EntryId,
    pub project_id: ProjectId,
    pub project_name: String,
    pub color: u8,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub seconds: i64,
    /// Why the entry ended, so the list can say when the app stopped it.
    pub stop_reason: Option<StopReason>,
}

/// Totals for a range, sorted with the biggest project first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub kind: RangeKind,
    pub range: DateRange,
    pub totals: Vec<ProjectTotal>,
    pub entries: Vec<ReportEntry>,
    pub total_seconds: i64,
}

impl Report {
    /// The largest project total, used to scale the report's bars.
    pub fn peak_seconds(&self) -> i64 {
        self.totals.first().map(|t| t.seconds).unwrap_or(0)
    }
}

/// Build a report from every entry that overlaps `range`.
///
/// Projects are looked up by id; an entry whose project was deleted is skipped
/// rather than shown as a blank row.
pub fn build_report(
    kind: RangeKind,
    range: DateRange,
    projects: &[Project],
    entries: &[TimeEntry],
    now: DateTime<Utc>,
) -> Report {
    let mut totals: Vec<ProjectTotal> = Vec::new();
    let mut rows: Vec<ReportEntry> = Vec::new();

    for entry in entries {
        let seconds = seconds_within(entry, range, now);
        if seconds == 0 {
            continue;
        }
        let Some(project) = projects.iter().find(|p| p.id == entry.project_id) else {
            continue;
        };

        match totals.iter_mut().find(|t| t.project_id == project.id) {
            Some(total) => total.seconds += seconds,
            None => totals.push(ProjectTotal {
                project_id: project.id,
                name: project.name.clone(),
                color: project.color,
                seconds,
            }),
        }

        rows.push(ReportEntry {
            id: entry.id,
            project_id: project.id,
            project_name: project.name.clone(),
            color: project.color,
            started_at: entry.started_at,
            ended_at: entry.ended_at,
            seconds,
            stop_reason: entry.stop_reason,
        });
    }

    totals.sort_by(|a, b| b.seconds.cmp(&a.seconds).then_with(|| a.name.cmp(&b.name)));
    rows.sort_by_key(|row| Reverse(row.started_at));
    let total_seconds = totals.iter().map(|t| t.seconds).sum();

    Report {
        kind,
        range,
        totals,
        entries: rows,
        total_seconds,
    }
}

/// A spreadsheet-ready view of a report: one row per entry, local timestamps.
pub fn report_to_csv(report: &Report) -> String {
    let mut out = String::from("project,date,start,end,duration,hours\n");
    // Oldest first reads better in a spreadsheet than the on-screen order.
    let mut rows = report.entries.clone();
    rows.sort_by_key(|row| row.started_at);

    for row in &rows {
        let start = row.started_at.with_timezone(&Local);
        let end = row.ended_at.map(|e| e.with_timezone(&Local));
        out.push_str(&format!(
            "{},{},{},{},{},{}\n",
            csv_field(&row.project_name),
            start.format("%Y-%m-%d"),
            start.format("%H:%M"),
            end.map(|e| e.format("%H:%M").to_string())
                .unwrap_or_else(|| "running".to_string()),
            format_duration(row.seconds),
            format_decimal_hours(row.seconds),
        ));
    }
    out
}

/// Quote a field only when it contains something a parser would misread.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// A suggested filename for an exported report, e.g. `hourglass-week-2026-09-07.csv`.
pub fn csv_filename(report: &Report) -> String {
    let day = report.range.start.with_timezone(&Local).format("%Y-%m-%d");
    let kind = match report.kind {
        RangeKind::Today => "day",
        RangeKind::Week => "week",
        RangeKind::Month => "month",
    };
    format!("hourglass-{kind}-{day}.csv")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntryId, StopReason};
    use chrono::Duration;

    fn project(id: i64, name: &str) -> Project {
        Project {
            id: ProjectId(id),
            name: name.to_string(),
            color: 0,
            archived: false,
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        }
    }

    fn entry(id: i64, project: i64, start: DateTime<Utc>, minutes: i64) -> TimeEntry {
        TimeEntry {
            id: EntryId(id),
            project_id: ProjectId(project),
            started_at: start,
            ended_at: Some(start + Duration::minutes(minutes)),
            stop_reason: Some(StopReason::Manual),
        }
    }

    /// A local-noon instant, safely inside the day whatever the machine's zone.
    fn local_noon(year: i32, month: u32, day: u32) -> DateTime<Utc> {
        let naive = NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        Local
            .from_local_datetime(&naive)
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn a_local_day_is_exactly_one_day_long() {
        let range = local_day(local_noon(2026, 9, 7));
        assert_eq!((range.end - range.start).num_hours(), 24);
        assert!(range.contains(local_noon(2026, 9, 7)));
        assert!(!range.contains(local_noon(2026, 9, 8)));
    }

    #[test]
    fn a_week_starts_on_monday_and_holds_seven_days() {
        // 2026-09-07 is a Monday; 2026-09-09 a Wednesday.
        let range = local_week(local_noon(2026, 9, 9));
        assert_eq!(range.start, local_day(local_noon(2026, 9, 7)).start);
        assert_eq!((range.end - range.start).num_days(), 7);
    }

    #[test]
    fn a_month_covers_the_first_to_the_last_day() {
        let range = local_month(local_noon(2026, 9, 20));
        assert_eq!(range.start, local_day(local_noon(2026, 9, 1)).start);
        assert_eq!(range.end, local_day(local_noon(2026, 10, 1)).start);
    }

    #[test]
    fn an_entry_crossing_midnight_is_split_between_the_two_days() {
        let today = local_day(local_noon(2026, 9, 7));
        let tomorrow = local_day(local_noon(2026, 9, 8));
        // Starts 90 minutes before midnight, runs for three hours.
        let crossing = entry(1, 1, tomorrow.start - Duration::minutes(90), 180);

        assert_eq!(seconds_within(&crossing, today, tomorrow.start), 90 * 60);
        assert_eq!(seconds_within(&crossing, tomorrow, tomorrow.start), 90 * 60);
    }

    #[test]
    fn a_running_entry_counts_up_to_now() {
        let now = local_noon(2026, 9, 7);
        let running = TimeEntry {
            id: EntryId(1),
            project_id: ProjectId(1),
            started_at: now - Duration::minutes(20),
            ended_at: None,
            stop_reason: None,
        };
        assert_eq!(seconds_within(&running, local_day(now), now), 1_200);
    }

    #[test]
    fn totals_sum_per_project_and_lead_with_the_biggest() {
        let now = local_noon(2026, 9, 7);
        let day = local_day(now);
        let projects = vec![project(1, "Atlas"), project(2, "Beacon")];
        let entries = vec![
            entry(1, 1, day.start + Duration::hours(9), 30),
            entry(2, 2, day.start + Duration::hours(10), 90),
            entry(3, 1, day.start + Duration::hours(12), 45),
        ];

        let report = build_report(RangeKind::Today, day, &projects, &entries, now);
        assert_eq!(report.totals.len(), 2);
        assert_eq!(report.totals[0].name, "Beacon");
        assert_eq!(report.totals[0].seconds, 90 * 60);
        assert_eq!(report.totals[1].seconds, 75 * 60);
        assert_eq!(report.total_seconds, 165 * 60);
        assert_eq!(report.peak_seconds(), 90 * 60);
    }

    #[test]
    fn entries_outside_the_range_are_left_out() {
        let now = local_noon(2026, 9, 7);
        let day = local_day(now);
        let projects = vec![project(1, "Atlas")];
        let entries = vec![entry(1, 1, day.start - Duration::hours(5), 60)];

        let report = build_report(RangeKind::Today, day, &projects, &entries, now);
        assert!(report.totals.is_empty());
        assert_eq!(report.total_seconds, 0);
    }

    #[test]
    fn entries_are_listed_newest_first() {
        let now = local_noon(2026, 9, 7);
        let day = local_day(now);
        let projects = vec![project(1, "Atlas")];
        let entries = vec![
            entry(1, 1, day.start + Duration::hours(9), 30),
            entry(2, 1, day.start + Duration::hours(11), 30),
        ];

        let report = build_report(RangeKind::Today, day, &projects, &entries, now);
        assert_eq!(report.entries[0].id, EntryId(2));
    }

    #[test]
    fn csv_has_a_header_and_one_row_per_entry_oldest_first() {
        let now = local_noon(2026, 9, 7);
        let day = local_day(now);
        let projects = vec![project(1, "Atlas")];
        let entries = vec![
            entry(1, 1, day.start + Duration::hours(11), 30),
            entry(2, 1, day.start + Duration::hours(9), 60),
        ];

        let csv = report_to_csv(&build_report(RangeKind::Today, day, &projects, &entries, now));
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "project,date,start,end,duration,hours");
        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("09:00"), "oldest entry comes first");
        assert!(lines[1].ends_with("1h 00m,1.00"));
        assert!(lines[2].ends_with("30m,0.50"));
    }

    #[test]
    fn a_project_name_with_a_comma_is_quoted() {
        assert_eq!(csv_field("Atlas, Inc"), "\"Atlas, Inc\"");
        assert_eq!(csv_field("Atlas \"A\""), "\"Atlas \"\"A\"\"\"");
        assert_eq!(csv_field("Atlas"), "Atlas");
    }

    #[test]
    fn the_export_filename_names_the_range() {
        let now = local_noon(2026, 9, 9);
        let report = build_report(RangeKind::Week, local_week(now), &[], &[], now);
        assert_eq!(csv_filename(&report), "hourglass-week-2026-09-07.csv");
    }
}
