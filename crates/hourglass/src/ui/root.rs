//! The window's root view: layout, keyboard handling, and the masthead.

use crate::state::AppState;
use crate::theme::{self, Fonts};
use crate::ui::band::day_band;
use crate::ui::entry_form::EntryDraft;
use crate::ui::kit::{action_button, dot, hairline, label, numeral};
use crate::ui::text_field::{FieldAction, TextField};
use chrono::{DateTime, Local, Utc};
use gpui::{
    Animation, AnimationExt, App, Context, Div, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, ease_in_out, prelude::FluentBuilder, px,
};
use hourglass_core::model::{ProjectId, ThemeChoice, format_clock, format_duration};
use hourglass_core::report::{RangeKind, Report};
use std::time::Duration;

/// Height of the strip along the top of the window.
///
/// The traffic lights are drawn into this strip by the system, so nothing may
/// be placed in its left-hand corner. The rail keeps its wordmark at the far
/// end of it and the page hangs its date on the other side of the rule.
pub(super) const TITLE_STRIP: f32 = 52.;

/// The page's left and right margins. Generous, because the whole look rests
/// on figures having room to breathe.
pub(super) const MARGIN: f32 = 40.;

/// How long a figure takes to settle after it changes.
///
/// gpui asks for a frame only while an animation is in flight, so this duration
/// *is* the cost: every extra millisecond is another full repaint of the
/// window, measured at roughly half a percent of a core each. At 180 ms the
/// clock paints for about a fifth of every second and is idle for the rest.
/// Shorter than that, the fade is over before the eye has registered it.
///
/// Pair it only with an easing that uses the whole span. A sharp ease-out such
/// as `ease_out_quint` is 97% finished at half time, which spends most of these
/// frames rendering a change too small to see.
const SETTLE: Duration = Duration::from_millis(180);

/// How faint a figure is at the start of its settle.
///
/// Not zero. A digit that appears out of nothing reads as a glitch; one that
/// arrives faint and firms up reads as ink meeting paper, which is the whole
/// metaphor. 0.10 is the floor that still reads as ink rather than a flash:
/// 0.32 was already a third there, which is why the settle was easy to miss.
const SETTLE_FROM: f32 = 0.10;

/// What the window is currently doing, beyond simply showing the day.
pub enum Mode {
    Browsing,
    AddingProject(TextField),
    RenamingProject(ProjectId, TextField),
    /// Deleting takes history with it, so it asks first.
    ConfirmingDelete(ProjectId),
    /// Writing down hours that were worked but never timed.
    AddingEntry(EntryDraft),
}

pub struct Root {
    pub(super) state: Entity<AppState>,
    pub(super) fonts: Fonts,
    pub(super) mode: Mode,
    focus: FocusHandle,
}

impl Root {
    pub fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Redraw whenever anything in the shared state changes, wherever the
        // change came from: this view, the menu bar, or a sleep signal.
        cx.observe(&state, |_, _, cx| cx.notify()).detach();

        let focus = cx.focus_handle();
        window.focus(&focus);

        Root {
            state,
            fonts: Fonts::resolve(cx),
            mode: Mode::Browsing,
            focus,
        }
    }

    /// Run a change against the shared state and redraw every view of it.
    pub(super) fn edit(&self, cx: &mut Context<Self>, change: impl FnOnce(&mut AppState)) {
        self.state.update(cx, |state, cx| {
            change(state);
            cx.notify();
        });
    }

    // -- keyboard ---------------------------------------------------------

    /// One key handler for the window. Typing goes to the open field if there
    /// is one; otherwise the keys are shortcuts.
    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let clipboard = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();

        match &mut self.mode {
            Mode::AddingProject(field) => {
                match field.handle(&event.keystroke, Some(&clipboard)) {
                    FieldAction::Submit => {
                        let name = field.text().trim().to_string();
                        if !field.is_empty() {
                            self.edit(cx, |state| {
                                state.create_project(&name);
                            });
                        }
                        self.mode = Mode::Browsing;
                    }
                    FieldAction::Cancel => self.mode = Mode::Browsing,
                    FieldAction::Edited => {}
                    FieldAction::Ignored => return,
                }
                cx.notify();
            }
            Mode::RenamingProject(id, field) => {
                let id = *id;
                match field.handle(&event.keystroke, Some(&clipboard)) {
                    FieldAction::Submit => {
                        let name = field.text().trim().to_string();
                        if !field.is_empty() {
                            self.edit(cx, |state| state.rename_project(id, &name));
                        }
                        self.mode = Mode::Browsing;
                    }
                    FieldAction::Cancel => self.mode = Mode::Browsing,
                    FieldAction::Edited => {}
                    FieldAction::Ignored => return,
                }
                cx.notify();
            }
            Mode::ConfirmingDelete(id) => {
                let id = *id;
                match event.keystroke.key.as_str() {
                    "enter" => {
                        self.edit(cx, |state| state.delete_project(id, Utc::now()));
                        self.mode = Mode::Browsing;
                    }
                    "escape" => self.mode = Mode::Browsing,
                    _ => return,
                }
                cx.notify();
            }
            Mode::AddingEntry(draft) => {
                match draft.handle(&event.keystroke, Some(&clipboard)) {
                    // A refused entry keeps the form open, because the fix is
                    // almost always one field rather than all four.
                    FieldAction::Submit => self.submit_entry_draft(cx),
                    FieldAction::Cancel => self.mode = Mode::Browsing,
                    FieldAction::Edited => {}
                    FieldAction::Ignored => return,
                }
                cx.notify();
            }
            Mode::Browsing => self.on_shortcut(event, cx),
        }
    }

    /// Try to save the open draft, closing the form only if it was accepted.
    pub(super) fn submit_entry_draft(&mut self, cx: &mut Context<Self>) {
        let Mode::AddingEntry(draft) = &self.mode else {
            return;
        };
        let (project, date, start, end) = draft.values();

        let mut saved = false;
        self.state.update(cx, |state, cx| {
            saved = state.add_manual_entry(&project, &date, &start, &end, Utc::now());
            cx.notify();
        });

        if saved {
            self.mode = Mode::Browsing;
        }
        cx.notify();
    }

    pub(super) fn close_entry_draft(&mut self, cx: &mut Context<Self>) {
        self.mode = Mode::Browsing;
        cx.notify();
    }

    /// Open the add-hours form, unless it is already open.
    pub(super) fn open_entry_draft(&mut self, cx: &mut Context<Self>) {
        if matches!(self.mode, Mode::AddingEntry(_)) {
            return;
        }
        let state = self.state.read(cx);
        let suggested = state
            .running_project()
            .or_else(|| state.active_projects().next())
            .map(|project| project.name.clone());

        self.mode = Mode::AddingEntry(EntryDraft::new(suggested));
        cx.notify();
    }

    /// Shortcuts for the things done dozens of times a day. Keyboard actions
    /// are never animated: at this frequency, motion only adds delay.
    fn on_shortcut(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let modified = event.keystroke.modifiers.platform
            || event.keystroke.modifiers.control
            || event.keystroke.modifiers.alt;

        // Command-E exports; every other shortcut is a bare key.
        if event.keystroke.modifiers.platform && key == "e" {
            self.edit(cx, |state| {
                state.export_csv(Utc::now());
            });
            cx.notify();
            return;
        }
        if modified {
            return;
        }

        let handled = match key {
            "space" => {
                self.edit(cx, |state| state.toggle(Utc::now()));
                true
            }
            "n" => {
                self.mode = Mode::AddingProject(TextField::new());
                true
            }
            "a" => {
                self.open_entry_draft(cx);
                true
            }
            "escape" => {
                let prompted = self.state.read(cx).pending_resume(Utc::now()).is_some();
                if prompted {
                    self.edit(cx, |state| state.dismiss_prompt());
                }
                prompted
            }
            "enter" => {
                let prompted = self.state.read(cx).pending_resume(Utc::now()).is_some();
                if prompted {
                    self.edit(cx, |state| state.resume(Utc::now()));
                }
                prompted
            }
            "t" => self.pick_range(RangeKind::Today, cx),
            "w" => self.pick_range(RangeKind::Week, cx),
            "m" => self.pick_range(RangeKind::Month, cx),
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" => {
                let index = key.parse::<usize>().unwrap_or(1) - 1;
                let target = self
                    .state
                    .read(cx)
                    .active_projects()
                    .nth(index)
                    .map(|project| project.id);
                if let Some(id) = target {
                    self.edit(cx, |state| state.start(id, Utc::now()));
                }
                target.is_some()
            }
            _ => false,
        };

        if handled {
            cx.notify();
        }
    }

    fn pick_range(&mut self, range: RangeKind, cx: &mut Context<Self>) -> bool {
        self.edit(cx, |state| state.set_range(range));
        true
    }

    // -- masthead ---------------------------------------------------------

    /// The date, hung in the top strip beside the rail's wordmark, with the
    /// appearance switch at the far end. It occupies the space the traffic
    /// lights force the page to leave empty anyway.
    fn masthead(&self, now: DateTime<Utc>, cx: &mut Context<Self>) -> impl IntoElement {
        let date = now.with_timezone(&Local).format("%A %-d %B %Y").to_string();

        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(20.))
            .h(px(TITLE_STRIP))
            .px(px(MARGIN))
            .flex_none()
            .child(
                div()
                    .text_size(px(9.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::muted())
                    .child(theme::letterspaced(&date.to_uppercase())),
            )
            .child(self.appearance_switch(cx))
    }

    /// Light, dark, or follow the system. Set as three small capitals divided
    /// by rules, the way a printed page offers alternatives, rather than as a
    /// switch or a dropdown that would need its own chrome.
    fn appearance_switch(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.state.read(cx).settings().theme;

        let options = ThemeChoice::ALL.map(|choice| {
            let selected = choice == current;
            let (id, name) = match choice {
                ThemeChoice::Light => ("theme-light", "light"),
                ThemeChoice::Dark => ("theme-dark", "dark"),
                ThemeChoice::System => ("theme-auto", "auto"),
            };

            div()
                .id(id)
                .px(px(9.))
                .py(px(4.))
                .cursor_pointer()
                .text_size(px(9.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if selected {
                    theme::ink()
                } else {
                    theme::faint()
                })
                .when(!selected, |style| {
                    style.hover(|style| style.text_color(theme::muted()))
                })
                .child(theme::letterspaced(&name.to_uppercase()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.edit(cx, |state| state.set_theme(choice));
                    cx.notify();
                }))
        });

        div()
            .flex()
            .items_center()
            .border_1()
            .border_color(theme::rule())
            .children(options)
    }

    /// The running clock, set large in the serif. The one place burnt sienna
    /// appears while working.
    fn header(
        &self,
        now: DateTime<Utc>,
        today: &Report,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = self.state.read(cx);
        let running = state
            .running_project()
            .map(|project| (project.name.clone(), project.color));
        let elapsed = state.elapsed_seconds(now);
        let has_projects = state.active_projects().next().is_some();
        let total_today = today.total_seconds;

        let title = match &running {
            Some((name, color)) => div()
                .flex()
                .items_center()
                .gap(px(9.))
                .child(dot(*color, 7.))
                .child(
                    div()
                        .font(self.fonts.serif(FontWeight::NORMAL))
                        .text_size(px(15.))
                        .text_color(theme::ink())
                        .child(name.clone()),
                ),
            None => div().flex().items_center().child(
                div()
                    .font(self.fonts.serif(FontWeight::NORMAL))
                    .text_size(px(15.))
                    .text_color(theme::faint())
                    .child(if has_projects {
                        "Nothing running"
                    } else {
                        "Add a project to start the clock"
                    }),
            ),
        };

        let is_running = running.is_some();
        let button = if is_running {
            action_button("stop", "Stop", true)
        } else {
            action_button("start", "Start", false)
        }
        .on_click(cx.listener(|this, _, _, cx| {
            this.edit(cx, |state| state.toggle(Utc::now()));
            cx.notify();
        }));

        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(title)
                    .child(button),
            )
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(self.hero_clock(elapsed, is_running))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(11.))
                            .child(label("today"))
                            .child(numeral(
                                self.fonts.display_numeric(FontWeight::NORMAL),
                                format_duration(total_today),
                                22.,
                                theme::ink(),
                            )),
                    ),
            )
    }

    /// The running clock, set large in the serif.
    ///
    /// Only the figures that changed this second are animated. The rest of the
    /// reading is one static run, so the clock's left edge and the spacing
    /// between its digits cannot move — a clock that reshuffles itself once a
    /// second is the exact cheapness `numeral` and `tnum` exist to prevent.
    fn hero_clock(&self, elapsed: i64, is_running: bool) -> Div {
        let reading = format_clock(elapsed);
        let row = div()
            .flex()
            .items_baseline()
            .font(self.fonts.display_numeric(FontWeight::LIGHT))
            .text_size(px(76.))
            // A stopped clock recedes. Burnt sienna is the only thing on the
            // page allowed to shout, and it does so exactly when the timer is
            // running.
            .text_color(if is_running {
                theme::ember()
            } else {
                theme::muted()
            });

        // A stopped clock is not changing, so it has nothing to settle and
        // must not ask for a single frame it does not need.
        if !is_running {
            return row.child(reading);
        }

        let settled = settled_prefix(&reading, &format_clock(elapsed - 1));
        if settled == reading.len() {
            return row.child(reading);
        }
        let (steady, changed) = reading.split_at(settled);

        row.child(steady.to_string()).child(
            div().child(changed.to_string()).with_animation(
                // Keyed on the second, so each tick starts a fresh settle and
                // gpui stops asking for frames the moment that settle lands.
                // A key that changed every frame would pin the window at the
                // display's refresh rate forever.
                ("clock-settle", elapsed as u64),
                Animation::new(SETTLE).with_easing(ease_in_out),
                |digits, delta| digits.opacity(SETTLE_FROM + (1. - SETTLE_FROM) * delta),
            ),
        )
    }

    /// The bar that appears when the app paused itself and needs an answer.
    fn resume_prompt(
        &self,
        now: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let (name, away) = {
            let state = self.state.read(cx);
            let (project, away) = state.pending_resume(now)?;
            (project.name.clone(), away)
        };
        let message = format!(
            "Away {}. The clock stopped on {}.",
            format_duration(away),
            name
        );

        Some(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .px(px(MARGIN))
                .py(px(12.))
                .flex_none()
                // A wash of the running colour, so the bar reads as part of
                // the timer rather than as an unrelated alert.
                .bg(theme::ember_wash(0x12))
                .border_b_1()
                .border_color(theme::rule())
                .child(
                    div()
                        .font(self.fonts.serif(FontWeight::NORMAL))
                        .text_size(px(14.))
                        .text_color(theme::ink())
                        .child(message),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            action_button("resume", "Resume", true).on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.edit(cx, |state| state.resume(Utc::now()));
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            action_button("leave-stopped", "Leave it stopped", false).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.edit(cx, |state| state.dismiss_prompt());
                                    cx.notify();
                                }),
                            ),
                        ),
                ),
        )
    }

    /// A confirmation or an error, shown for a few seconds under the report.
    fn status_bar(&self, cx: &App) -> Option<impl IntoElement> {
        let status = self.state.read(cx).status()?;
        let message = status.message.clone();
        let color = if status.is_error {
            theme::alert()
        } else {
            theme::muted()
        };

        Some(
            div()
                .flex()
                .items_center()
                .h(px(34.))
                .px(px(MARGIN))
                .flex_none()
                .border_t_1()
                .border_color(theme::rule())
                .font(self.fonts.serif(FontWeight::NORMAL))
                .text_size(px(13.))
                .text_color(color)
                .child(message),
        )
    }
}

impl Focusable for Root {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One clock reading for the whole frame, so the masthead, the band,
        // and the totals can never disagree by a tick.
        let now = Utc::now();
        let today = self.state.read(cx).today(now);
        let report = self.state.read(cx).report(now);

        div()
            .key_context("Hourglass")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .flex()
            .size_full()
            .bg(theme::paper())
            .font_family(self.fonts.ui.clone())
            .text_color(theme::ink())
            .child(self.rail(now, &today, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .overflow_hidden()
                    .child(self.masthead(now, cx))
                    .child(hairline())
                    .children(self.resume_prompt(now, cx))
                    .children(match &self.mode {
                        Mode::AddingEntry(draft) => Some(self.entry_form(draft, cx)),
                        _ => None,
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_none()
                            .gap(px(22.))
                            .px(px(MARGIN))
                            .pt(px(24.))
                            .pb(px(26.))
                            .child(self.header(now, &today, cx))
                            .child(day_band(&today, now)),
                    )
                    .child(hairline())
                    .child(self.report_pane(now, &report, cx))
                    .children(self.status_bar(cx)),
            )
    }
}

/// How much of a clock reading is unchanged from the second before it.
///
/// A clock that counts upward only ever rewrites a *suffix* of its own text —
/// `1:29:59` becomes `1:30:00`, never something with a fresh middle — so the
/// figures that moved can be found by comparing against the previous second
/// and settled on their own. Deriving it this way rather than remembering the
/// last reading keeps the invariant that a frame is drawn purely from `now`.
///
/// Returns a byte index, which is also a character index: `format_clock`
/// emits nothing but ASCII digits and colons.
fn settled_prefix(current: &str, previous: &str) -> usize {
    // Passing an hour digit rewrites the whole reading rather than a suffix.
    if current.len() != previous.len() {
        return 0;
    }
    current
        .bytes()
        .zip(previous.bytes())
        .take_while(|(now, before)| now == before)
        .count()
}

/// The menu bar's short reading: hours and minutes only.
pub fn menu_bar_title(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_tick_settles_the_seconds_digit_alone() {
        assert_eq!(settled_prefix("0:12:35", "0:12:34"), 6);
    }

    #[test]
    fn a_rolling_ten_seconds_settles_both_seconds_digits() {
        assert_eq!(settled_prefix("0:12:40", "0:12:39"), 5);
    }

    #[test]
    fn a_rolling_minute_settles_everything_below_the_hour() {
        assert_eq!(settled_prefix("1:30:00", "1:29:59"), 2);
    }

    #[test]
    fn a_rolling_hour_settles_the_whole_reading() {
        assert_eq!(settled_prefix("3:00:00", "2:59:59"), 0);
    }

    #[test]
    fn passing_ten_hours_settles_the_whole_reading() {
        // The reading grows a digit, so nothing is where it was.
        assert_eq!(settled_prefix("10:00:00", "9:59:59"), 0);
    }

    #[test]
    fn a_reading_that_did_not_change_settles_nothing() {
        // Guards the redraw that happens for some other reason mid-second: it
        // must not restart the animation or animate an empty string.
        let reading = "0:00:00";
        assert_eq!(settled_prefix(reading, reading), reading.len());
    }

    #[test]
    fn every_second_of_an_hour_settles_at_most_the_figures_that_moved() {
        // The suffix claim the whole animation rests on: whatever changed is
        // always at the end, so the steady part can be drawn as one run.
        for second in 1..3_600i64 {
            let current = format_clock(second);
            let previous = format_clock(second - 1);
            let settled = settled_prefix(&current, &previous);

            assert_eq!(current[..settled], previous[..settled]);
            assert_ne!(
                current, previous,
                "consecutive seconds should never read the same"
            );
            assert!(settled < current.len(), "something must have changed");
        }
    }

    #[test]
    fn the_menu_bar_shows_hours_and_minutes_without_seconds() {
        assert_eq!(menu_bar_title(0), "0:00");
        assert_eq!(menu_bar_title(59), "0:00");
        assert_eq!(menu_bar_title(60), "0:01");
        assert_eq!(menu_bar_title(3_600), "1:00");
        assert_eq!(menu_bar_title(14_760), "4:06");
    }
}
