//! Hours typed in by hand.
//!
//! Forgetting to press start is the most common way a time tracker loses an
//! afternoon, so entries can be written after the fact. Everything typed is
//! checked before it reaches the database: the app's whole claim is that the
//! recorded hours are honest, and a hand-written entry is the easiest place to
//! quietly break that.
//!
//! Parsing is generous about form and strict about meaning. `9:30`, `0930`,
//! and `9.30am` all name the same minute; an entry that ends before it starts,
//! or that runs into the future, or that covers time already claimed by
//! another entry, is refused.

use crate::model::TimeEntry;
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Utc};
use std::fmt;

/// Why a typed entry was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftError {
    NoProject,
    UnknownProject(String),
    BadDate,
    BadStart,
    BadEnd,
    EndBeforeStart,
    InFuture,
    /// The span collides with an entry already recorded against this project.
    Overlaps(String),
    /// Local midnight-to-midnight arithmetic failed, which daylight saving can
    /// cause for an hour that does not exist on the given date.
    ImpossibleLocalTime,
}

impl fmt::Display for DraftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DraftError::NoProject => f.write_str("Name a project"),
            DraftError::UnknownProject(name) => write!(f, "No project called \"{name}\""),
            DraftError::BadDate => f.write_str("Date should look like 2026-09-07, or \"today\""),
            DraftError::BadStart => f.write_str("Start should look like 9:30 or 9:30am"),
            DraftError::BadEnd => f.write_str("End should look like 17:00 or 5pm"),
            DraftError::EndBeforeStart => f.write_str("The end has to come after the start"),
            DraftError::InFuture => f.write_str("Hours cannot be logged in the future"),
            DraftError::Overlaps(name) => write!(f, "That time is already recorded against {name}"),
            DraftError::ImpossibleLocalTime => f.write_str("That clock time does not exist on that date"),
        }
    }
}

/// A time of day, `9:30` / `0930` / `9.30am` / `5pm` all being valid.
///
/// Without a meridiem the hour is read as it stands, so `17` and `5pm` agree.
pub fn parse_time(input: &str) -> Option<(u32, u32)> {
    let cleaned = input.trim().to_lowercase().replace(' ', "");
    if cleaned.is_empty() {
        return None;
    }

    let (body, afternoon) = match cleaned.strip_suffix("am") {
        Some(rest) => (rest, Some(false)),
        None => match cleaned.strip_suffix("pm") {
            Some(rest) => (rest, Some(true)),
            None => (cleaned.as_str(), None),
        },
    };

    let separated = body.split_once(':').or_else(|| body.split_once('.'));
    let (hour, minute) = match separated {
        Some((hours, minutes)) => (parse_number(hours)?, parse_number(minutes)?),
        None => {
            if !body.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            match body.len() {
                1 | 2 => (parse_number(body)?, 0),
                3 => (parse_number(&body[..1])?, parse_number(&body[1..])?),
                4 => (parse_number(&body[..2])?, parse_number(&body[2..])?),
                _ => return None,
            }
        }
    };

    let hour = match afternoon {
        // 12am is midnight and 12pm is noon; every other hour just shifts.
        Some(true) if hour == 12 => 12,
        Some(true) if hour < 12 => hour + 12,
        Some(false) if hour == 12 => 0,
        Some(false) if hour < 12 => hour,
        Some(_) => return None,
        None => hour,
    };

    (hour <= 23 && minute <= 59).then_some((hour, minute))
}

fn parse_number(text: &str) -> Option<u32> {
    let text = text.trim();
    (!text.is_empty() && text.chars().all(|c| c.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// A date, defaulting to `today` when the field is left alone.
///
/// Accepts `2026-09-07`, `09-07` or `9/7` for the current year, and the words
/// `today` and `yesterday`.
pub fn parse_date(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    let cleaned = input.trim().to_lowercase();
    match cleaned.as_str() {
        "" | "today" => return Some(today),
        "yesterday" => return today.pred_opt(),
        _ => {}
    }

    let parts: Vec<&str> = cleaned.split(['-', '/', '.']).collect();
    match parts.len() {
        3 => NaiveDate::from_ymd_opt(
            parse_number(parts[0])? as i32,
            parse_number(parts[1])?,
            parse_number(parts[2])?,
        ),
        2 => NaiveDate::from_ymd_opt(
            today.year(),
            parse_number(parts[0])?,
            parse_number(parts[1])?,
        ),
        _ => None,
    }
}

/// Turn typed fields into a UTC span, or explain what is wrong with them.
///
/// The date and both times are read in the local zone, because that is the
/// zone the user was working in when they forgot to press start.
pub fn resolve_span(
    date: &str,
    start: &str,
    end: &str,
    now: DateTime<Utc>,
) -> Result<(DateTime<Utc>, DateTime<Utc>), DraftError> {
    let today = now.with_timezone(&Local).date_naive();
    let day = parse_date(date, today).ok_or(DraftError::BadDate)?;
    let (start_hour, start_minute) = parse_time(start).ok_or(DraftError::BadStart)?;
    let (end_hour, end_minute) = parse_time(end).ok_or(DraftError::BadEnd)?;

    let started_at = local_instant(day, start_hour, start_minute)?;
    let ended_at = local_instant(day, end_hour, end_minute)?;

    if ended_at <= started_at {
        return Err(DraftError::EndBeforeStart);
    }
    if ended_at > now {
        return Err(DraftError::InFuture);
    }
    Ok((started_at, ended_at))
}

fn local_instant(day: NaiveDate, hour: u32, minute: u32) -> Result<DateTime<Utc>, DraftError> {
    let naive = day
        .and_hms_opt(hour, minute, 0)
        .ok_or(DraftError::ImpossibleLocalTime)?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|local| local.with_timezone(&Utc))
        .ok_or(DraftError::ImpossibleLocalTime)
}

/// Whether two half-open spans share any time at all.
pub fn spans_overlap(
    a_start: DateTime<Utc>,
    a_end: DateTime<Utc>,
    b_start: DateTime<Utc>,
    b_end: DateTime<Utc>,
) -> bool {
    a_start < b_end && b_start < a_end
}

/// The first recorded entry that collides with `start..end`.
///
/// A still-running entry is treated as reaching up to `now`, so hours cannot be
/// written over the session currently being counted.
pub fn first_conflict(
    entries: &[TimeEntry],
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Option<&TimeEntry> {
    entries.iter().find(|entry| {
        spans_overlap(start, end, entry.started_at, entry.ended_at.unwrap_or(now))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EntryId, ProjectId};
    use chrono::Duration;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 7).unwrap()
    }

    fn local_at(hour: u32, minute: u32) -> DateTime<Utc> {
        local_instant(today(), hour, minute).unwrap()
    }

    // -- times ------------------------------------------------------------

    #[test]
    fn a_time_can_be_written_with_or_without_a_separator() {
        assert_eq!(parse_time("9:30"), Some((9, 30)));
        assert_eq!(parse_time("09:30"), Some((9, 30)));
        assert_eq!(parse_time("9.30"), Some((9, 30)));
        assert_eq!(parse_time("0930"), Some((9, 30)));
        assert_eq!(parse_time("930"), Some((9, 30)));
        assert_eq!(parse_time("9"), Some((9, 0)));
    }

    #[test]
    fn a_meridiem_shifts_the_afternoon_hours() {
        assert_eq!(parse_time("5pm"), Some((17, 0)));
        assert_eq!(parse_time("5:45pm"), Some((17, 45)));
        assert_eq!(parse_time("9am"), Some((9, 0)));
        assert_eq!(parse_time("9 am"), Some((9, 0)));
    }

    #[test]
    fn noon_and_midnight_are_the_two_awkward_ones() {
        assert_eq!(parse_time("12pm"), Some((12, 0)));
        assert_eq!(parse_time("12am"), Some((0, 0)));
        assert_eq!(parse_time("12:30am"), Some((0, 30)));
    }

    #[test]
    fn a_twenty_four_hour_time_needs_no_meridiem() {
        assert_eq!(parse_time("17:00"), Some((17, 0)));
        assert_eq!(parse_time("23:59"), Some((23, 59)));
        assert_eq!(parse_time("00:00"), Some((0, 0)));
    }

    #[test]
    fn nonsense_times_are_refused_rather_than_guessed_at() {
        assert_eq!(parse_time(""), None);
        assert_eq!(parse_time("half past nine"), None);
        assert_eq!(parse_time("25:00"), None);
        assert_eq!(parse_time("9:75"), None);
        assert_eq!(parse_time("13pm"), None);
        assert_eq!(parse_time("99999"), None);
    }

    // -- dates ------------------------------------------------------------

    #[test]
    fn an_empty_date_means_today() {
        assert_eq!(parse_date("", today()), Some(today()));
        assert_eq!(parse_date("  ", today()), Some(today()));
        assert_eq!(parse_date("today", today()), Some(today()));
    }

    #[test]
    fn yesterday_steps_back_one_day() {
        assert_eq!(
            parse_date("yesterday", today()),
            NaiveDate::from_ymd_opt(2026, 9, 6)
        );
    }

    #[test]
    fn a_date_can_leave_off_the_year() {
        assert_eq!(
            parse_date("08-21", today()),
            NaiveDate::from_ymd_opt(2026, 8, 21)
        );
        assert_eq!(
            parse_date("8/21", today()),
            NaiveDate::from_ymd_opt(2026, 8, 21)
        );
    }

    #[test]
    fn a_full_date_is_read_as_written() {
        assert_eq!(
            parse_date("2025-12-31", today()),
            NaiveDate::from_ymd_opt(2025, 12, 31)
        );
    }

    #[test]
    fn impossible_dates_are_refused() {
        assert_eq!(parse_date("2026-02-30", today()), None);
        assert_eq!(parse_date("2026-13-01", today()), None);
        assert_eq!(parse_date("sometime", today()), None);
    }

    // -- spans ------------------------------------------------------------

    #[test]
    fn a_well_formed_span_resolves_to_the_local_hours_typed() {
        let now = local_at(18, 0);
        let (start, end) = resolve_span("today", "9:30", "11:00", now).unwrap();
        assert_eq!(start, local_at(9, 30));
        assert_eq!(end, local_at(11, 0));
        assert_eq!((end - start).num_minutes(), 90);
    }

    #[test]
    fn an_end_before_the_start_is_refused() {
        let now = local_at(18, 0);
        assert_eq!(
            resolve_span("today", "11:00", "9:30", now),
            Err(DraftError::EndBeforeStart)
        );
    }

    #[test]
    fn a_zero_length_span_is_refused() {
        let now = local_at(18, 0);
        assert_eq!(
            resolve_span("today", "9:30", "9:30", now),
            Err(DraftError::EndBeforeStart)
        );
    }

    #[test]
    fn hours_cannot_be_logged_into_the_future() {
        let now = local_at(12, 0);
        assert_eq!(
            resolve_span("today", "13:00", "14:00", now),
            Err(DraftError::InFuture)
        );
    }

    #[test]
    fn each_malformed_field_names_itself() {
        let now = local_at(18, 0);
        assert_eq!(
            resolve_span("nonsense", "9:00", "10:00", now),
            Err(DraftError::BadDate)
        );
        assert_eq!(
            resolve_span("today", "breakfast", "10:00", now),
            Err(DraftError::BadStart)
        );
        assert_eq!(
            resolve_span("today", "9:00", "lunch", now),
            Err(DraftError::BadEnd)
        );
    }

    // -- conflicts --------------------------------------------------------

    fn entry(id: i64, start: DateTime<Utc>, end: Option<DateTime<Utc>>) -> TimeEntry {
        TimeEntry {
            id: EntryId(id),
            project_id: ProjectId(1),
            started_at: start,
            ended_at: end,
            stop_reason: None,
        }
    }

    #[test]
    fn spans_that_merely_touch_do_not_overlap() {
        let a = (local_at(9, 0), local_at(10, 0));
        let b = (local_at(10, 0), local_at(11, 0));
        assert!(!spans_overlap(a.0, a.1, b.0, b.1));
    }

    #[test]
    fn a_span_inside_another_overlaps() {
        let recorded = vec![entry(1, local_at(9, 0), Some(local_at(12, 0)))];
        let found = first_conflict(
            &recorded,
            local_at(10, 0),
            local_at(11, 0),
            local_at(18, 0),
        );
        assert_eq!(found.map(|e| e.id), Some(EntryId(1)));
    }

    #[test]
    fn a_span_in_a_gap_between_entries_is_free() {
        let recorded = vec![
            entry(1, local_at(9, 0), Some(local_at(10, 0))),
            entry(2, local_at(13, 0), Some(local_at(14, 0))),
        ];
        let found = first_conflict(
            &recorded,
            local_at(11, 0),
            local_at(12, 0),
            local_at(18, 0),
        );
        assert!(found.is_none());
    }

    #[test]
    fn the_running_entry_reaches_up_to_now_and_blocks_writing_over_it() {
        let now = local_at(15, 0);
        let recorded = vec![entry(1, local_at(14, 0), None)];
        let found = first_conflict(&recorded, local_at(14, 30), local_at(14, 45), now);
        assert_eq!(found.map(|e| e.id), Some(EntryId(1)));
    }

    #[test]
    fn a_span_before_a_running_entry_began_is_still_free() {
        let now = local_at(15, 0);
        let recorded = vec![entry(1, local_at(14, 0), None)];
        let found = first_conflict(&recorded, local_at(12, 0), local_at(13, 0), now);
        assert!(found.is_none());
    }

    #[test]
    fn yesterdays_hours_can_be_written_today() {
        let now = local_at(9, 0);
        let (start, end) = resolve_span("yesterday", "14:00", "17:30", now).unwrap();
        assert!(end < now);
        assert_eq!((end - start), Duration::minutes(210));
    }
}
