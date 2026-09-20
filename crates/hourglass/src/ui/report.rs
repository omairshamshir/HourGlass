//! The lower half of the window: how the range's hours split by project, and
//! the entries behind those totals, set as a ledger.

use crate::theme;
use crate::ui::kit::{dot, empty_note, ghost_button, label, range_tab, tag};
use crate::ui::root::{MARGIN, Root};
use chrono::{DateTime, Local, Utc};
use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px, relative,
};
use hourglass_core::model::{StopReason, format_duration};
use hourglass_core::report::{RangeKind, Report, ReportEntry};

impl Root {
    pub(super) fn report_pane(
        &self,
        now: DateTime<Utc>,
        report: &Report,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = now;
        div()
            .id("report")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_y_scroll()
            .px(px(MARGIN))
            .pt(px(16.))
            .pb(px(24.))
            .gap(px(20.))
            .child(self.range_switcher(report, cx))
            .child(self.totals(report))
            .child(self.entry_list(report, cx))
            .into_any_element()
    }

    /// Today, week, month, with the range's total on the right.
    fn range_switcher(&self, report: &Report, cx: &mut Context<Self>) -> AnyElement {
        let selected = report.kind;
        let tabs = [
            ("range-today", RangeKind::Today),
            ("range-week", RangeKind::Week),
            ("range-month", RangeKind::Month),
        ]
        .map(|(id, kind)| {
            range_tab(id, kind.label(), kind == selected).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.edit(cx, |state| state.set_range(kind));
                    cx.notify();
                },
            ))
        });

        div()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(theme::rule_soft())
            .child(div().flex().items_end().children(tabs))
            .child(
                div()
                    .pb(px(6.))
                    .font(self.fonts.numeric(FontWeight::MEDIUM))
                    .text_size(px(15.))
                    .text_color(theme::ink())
                    .child(format_duration(report.total_seconds)),
            )
            .into_any_element()
    }

    /// One line per project, longest first, scaled against the largest.
    fn totals(&self, report: &Report) -> AnyElement {
        if report.totals.is_empty() {
            return empty_note(
                self.fonts.serif(FontWeight::NORMAL),
                "No hours in this range yet.",
            );
        }

        let peak = report.peak_seconds().max(1);
        let rows = report.totals.iter().map(|total| {
            let share = total.seconds as f32 / peak as f32;

            div()
                .flex()
                .items_center()
                .gap(px(11.))
                .h(px(27.))
                .child(dot(total.color, 6.))
                .child(
                    div()
                        .w(px(146.))
                        .flex_none()
                        .overflow_hidden()
                        .font(self.fonts.serif(FontWeight::NORMAL))
                        .text_size(px(13.5))
                        .child(total.name.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .h(px(4.))
                        .bg(theme::rule_soft())
                        .child(
                            div()
                                .h_full()
                                .w(relative(share.clamp(0.02, 1.0)))
                                .bg(theme::swatch(total.color)),
                        ),
                )
                .child(
                    div()
                        .w(px(70.))
                        .flex_none()
                        .flex()
                        .justify_end()
                        .font(self.fonts.numeric(FontWeight::NORMAL))
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(format_duration(total.seconds)),
                )
        });

        div()
            .flex()
            .flex_col()
            .children(rows)
            .into_any_element()
    }

    /// Every entry in the range, newest first.
    ///
    /// The heading and its add button stay put even when the range is empty,
    /// because an empty range is exactly when someone reaches for the button.
    fn entry_list(&self, report: &Report, cx: &mut Context<Self>) -> AnyElement {
        let rows: Vec<AnyElement> = report
            .entries
            .iter()
            .map(|entry| self.entry_row(entry, cx))
            .collect();
        let empty = rows.is_empty();

        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pb(px(7.))
                    // With no rows beneath it, the rule would hang off the
                    // heading with nothing to divide. `totals` has already
                    // said the range is empty, so say nothing more here.
                    .when(!empty, |heading| {
                        heading.border_b_1().border_color(theme::rule())
                    })
                    .child(label("entries"))
                    .child(
                        ghost_button("add-hours", "+ Add hours").on_click(cx.listener(
                            |this, _, _, cx| this.open_entry_draft(cx),
                        )),
                    ),
            )
            .children(rows)
            .into_any_element()
    }

    fn entry_row(&self, entry: &ReportEntry, cx: &mut Context<Self>) -> AnyElement {
        let id = entry.id;
        let start = entry.started_at.with_timezone(&Local).format("%H:%M");
        let finish = match entry.ended_at {
            Some(end) => end.with_timezone(&Local).format("%H:%M").to_string(),
            None => "now".to_string(),
        };
        let running = entry.ended_at.is_none();

        div()
            .id(("entry", id.0 as u64))
            .group("entry-row")
            .flex()
            .items_center()
            .gap(px(13.))
            .h(px(31.))
            .border_b_1()
            .border_color(theme::rule_soft())
            .child(dot(entry.color, 5.))
            .child(
                div()
                    .font(self.fonts.numeric(FontWeight::NORMAL))
                    .text_size(px(11.5))
                    .text_color(theme::muted())
                    .child(format!("{start} – {finish}")),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .flex_1()
                    .overflow_hidden()
                    .child(
                        div()
                            .font(self.fonts.serif(FontWeight::NORMAL))
                            .text_size(px(13.))
                            .text_color(theme::ink())
                            .child(entry.project_name.clone()),
                    )
                    // Say so when the app stopped this entry rather than the
                    // user: an unexplained short entry looks like lost work.
                    .children(stop_note(entry.stop_reason).map(tag)),
            )
            .child(
                div()
                    .font(self.fonts.numeric(if running {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    }))
                    .text_size(px(12.))
                    .text_color(if running {
                        theme::ember()
                    } else {
                        theme::ink()
                    })
                    .child(format_duration(entry.seconds)),
            )
            .child(
                // The delete control stays hidden until the row is hovered,
                // so a list of hours does not read as a list of buttons.
                div()
                    .id(("delete-entry", id.0 as u64))
                    .w(px(20.))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .text_size(px(13.))
                    .text_color(theme::faint())
                    .opacity(0.)
                    .cursor_pointer()
                    .group_hover("entry-row", |style| style.opacity(1.))
                    .hover(|style| style.text_color(theme::alert()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.edit(cx, |state| state.delete_entry(id));
                        cx.notify();
                    }))
                    .child("×"),
            )
            .into_any_element()
    }
}

/// How an automatic stop is described in the entry list.
pub fn stop_note(reason: Option<StopReason>) -> Option<&'static str> {
    match reason? {
        StopReason::Sleep => Some("slept"),
        StopReason::Idle => Some("idle"),
        StopReason::Lock => Some("locked"),
        StopReason::Manual | StopReason::Switch => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_automatic_stops_are_worth_annotating() {
        assert_eq!(stop_note(Some(StopReason::Sleep)), Some("slept"));
        assert_eq!(stop_note(Some(StopReason::Idle)), Some("idle"));
        assert_eq!(stop_note(Some(StopReason::Manual)), None);
        assert_eq!(stop_note(None), None);
    }
}
