//! The day band: today drawn as a strip of time.
//!
//! This is the one picture the app is built around. Bars in a report tell you
//! how much; the band tells you *when*, and just as importantly where the gaps
//! are. Honest hours are as much about the empty stretches as the full ones.

use crate::theme;
use chrono::{DateTime, Local, Timelike, Utc};
use gpui::{Div, ParentElement, Styled, div, px, relative};
use hourglass_core::report::Report;

/// The band's height in pixels.
const BAND_HEIGHT: f32 = 54.;

/// The stretch of the day the band draws, in local hours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start_hour: u32,
    pub end_hour: u32,
}

impl Window {
    fn span_hours(self) -> f32 {
        (self.end_hour - self.start_hour) as f32
    }

    /// Where an instant sits across the band, 0.0 at the left edge to 1.0 at
    /// the right. Values outside that range mean the instant is off-band.
    fn fraction_of(self, instant: DateTime<Utc>) -> f32 {
        let local = instant.with_timezone(&Local);
        let hours = local.hour() as f32
            + local.minute() as f32 / 60.0
            + local.second() as f32 / 3_600.0;
        (hours - self.start_hour as f32) / self.span_hours()
    }
}

/// Pick the hours worth showing.
///
/// A fixed midnight-to-midnight band spends most of its width on hours nobody
/// works, so the window covers the working day and stretches only as far as
/// the day's real entries require.
pub fn window_for(report: &Report, now: DateTime<Utc>) -> Window {
    let mut earliest = 8u32;
    let mut latest = 19u32;

    for entry in &report.entries {
        let start = entry.started_at.with_timezone(&Local).hour();
        let end = entry
            .ended_at
            .unwrap_or(now)
            .with_timezone(&Local)
            .hour()
            .saturating_add(1)
            .min(24);
        earliest = earliest.min(start);
        latest = latest.max(end);
    }

    // Keep the current hour in view even before any work is recorded.
    let current = now.with_timezone(&Local).hour();
    earliest = earliest.min(current);
    latest = latest.max((current + 1).min(24));

    Window {
        start_hour: earliest,
        end_hour: latest.max(earliest + 1),
    }
}

/// Hour marks, thinned out so the labels never collide on a narrow window.
fn tick_hours(window: Window) -> Vec<u32> {
    let step = match window.span_hours() as u32 {
        0..=8 => 1,
        9..=14 => 2,
        _ => 3,
    };
    (window.start_hour..=window.end_hour)
        .filter(|hour| hour % step == 0)
        .collect()
}

/// Format an hour the way a clock face would: `9a`, `12p`, `5p`.
fn hour_label(hour: u32) -> String {
    match hour {
        0 | 24 => "12a".to_string(),
        12 => "12p".to_string(),
        1..=11 => format!("{hour}a"),
        _ => format!("{}p", hour - 12),
    }
}

/// Draw the band for `report`, which must be a single day.
pub fn day_band(report: &Report, now: DateTime<Utc>) -> Div {
    let window = window_for(report, now);

    let segments = report.entries.iter().filter_map(|entry| {
        let left = window.fraction_of(entry.started_at).clamp(0.0, 1.0);
        let right = window
            .fraction_of(entry.ended_at.unwrap_or(now))
            .clamp(0.0, 1.0);

        // A one-minute entry would otherwise be invisible; give every segment
        // a floor so short sessions still register as marks on the day.
        let width = (right - left).max(0.004);
        if width <= 0.0 {
            return None;
        }

        Some(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(left))
                .w(relative(width.min(1.0 - left)))
                .bg(theme::swatch(entry.color)),
        )
    });

    let now_marker = div()
        .absolute()
        .top(px(-4.))
        .bottom(px(-4.))
        .left(relative(window.fraction_of(now).clamp(0.0, 1.0)))
        .w(px(1.5))
        .bg(theme::ember());

    let ticks = tick_hours(window).into_iter().map(move |hour| {
        div()
            .absolute()
            .top_0()
            .left(relative(
                ((hour - window.start_hour) as f32 / window.span_hours()).clamp(0.0, 1.0),
            ))
            .text_size(px(9.5))
            .text_color(theme::muted())
            .child(hour_label(hour))
    });

    div()
        .flex()
        .flex_col()
        .gap(px(7.))
        .child(
            div()
                .relative()
                .h(px(BAND_HEIGHT))
                .w_full()
                // An unworked hour is paper the colour of a ruled line, not a
                // dark trough: the band should read as a chart printed on the
                // page rather than as a widget dropped onto it.
                .bg(theme::rule_soft())
                .children(segments)
                .child(now_marker),
        )
        .child(div().relative().h(px(12.)).w_full().children(ticks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, NaiveDate, TimeZone};
    use hourglass_core::model::{EntryId, Project, ProjectId};
    use hourglass_core::report::{RangeKind, build_report, local_day};

    fn local_at(hour: u32, minute: u32) -> DateTime<Utc> {
        let naive = NaiveDate::from_ymd_opt(2026, 9, 7)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap();
        Local
            .from_local_datetime(&naive)
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    }

    fn report_with(entries: Vec<(u32, u32, i64)>, now: DateTime<Utc>) -> Report {
        let project = Project {
            id: ProjectId(1),
            name: "Atlas".into(),
            color: 0,
            archived: false,
            created_at: now,
        };
        let entries: Vec<_> = entries
            .into_iter()
            .enumerate()
            .map(|(index, (hour, minute, minutes))| {
                let start = local_at(hour, minute);
                hourglass_core::model::TimeEntry {
                    id: EntryId(index as i64 + 1),
                    project_id: ProjectId(1),
                    started_at: start,
                    ended_at: Some(start + Duration::minutes(minutes)),
                    stop_reason: None,
                }
            })
            .collect();

        build_report(
            RangeKind::Today,
            local_day(now),
            std::slice::from_ref(&project),
            &entries,
            now,
        )
    }

    #[test]
    fn an_empty_day_still_shows_the_working_hours() {
        let now = local_at(12, 0);
        let window = window_for(&report_with(vec![], now), now);
        assert_eq!(window.start_hour, 8);
        assert_eq!(window.end_hour, 19);
    }

    #[test]
    fn an_early_start_widens_the_window_to_include_it() {
        let now = local_at(12, 0);
        let window = window_for(&report_with(vec![(5, 30, 60)], now), now);
        assert_eq!(window.start_hour, 5);
    }

    #[test]
    fn a_late_finish_widens_the_window_to_include_it() {
        let now = local_at(12, 0);
        let window = window_for(&report_with(vec![(22, 0, 30)], now), now);
        assert_eq!(window.end_hour, 23);
    }

    #[test]
    fn the_current_hour_is_always_visible() {
        let now = local_at(23, 30);
        let window = window_for(&report_with(vec![], now), now);
        assert_eq!(window.end_hour, 24);
    }

    #[test]
    fn positions_run_from_zero_at_the_left_edge_to_one_at_the_right() {
        let window = Window {
            start_hour: 8,
            end_hour: 20,
        };
        assert!((window.fraction_of(local_at(8, 0)) - 0.0).abs() < 1e-5);
        assert!((window.fraction_of(local_at(14, 0)) - 0.5).abs() < 1e-5);
        assert!((window.fraction_of(local_at(20, 0)) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn hour_labels_read_like_a_clock() {
        assert_eq!(hour_label(0), "12a");
        assert_eq!(hour_label(9), "9a");
        assert_eq!(hour_label(12), "12p");
        assert_eq!(hour_label(17), "5p");
        assert_eq!(hour_label(24), "12a");
    }

    #[test]
    fn a_wide_window_thins_out_its_hour_marks() {
        let narrow = tick_hours(Window {
            start_hour: 9,
            end_hour: 15,
        });
        assert_eq!(narrow.len(), 7);

        let wide = tick_hours(Window {
            start_hour: 0,
            end_hour: 24,
        });
        assert!(wide.len() <= 9, "got {} marks", wide.len());
    }
}
