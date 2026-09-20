//! The project rail: every project, today's time against each, one click to
//! start. The number on the left is the key that starts it.

use crate::theme;
use crate::ui::kit::{dot, ghost_button, hairline, label, ring};
use crate::ui::root::{Mode, Root, TITLE_STRIP};
use crate::ui::text_field::TextField;
use chrono::{DateTime, Utc};
use gpui::{
    AnyElement, App, Context, FontWeight, InteractiveElement, IntoElement, MouseButton,
    ParentElement, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};
use hourglass_core::model::{Project, ProjectId, format_duration};
use hourglass_core::report::Report;

/// Width of the rail. Wide enough for a name and a duration, narrow enough
/// that the day band keeps the room.
const RAIL_WIDTH: f32 = 236.;

impl Root {
    pub(super) fn rail(
        &self,
        now: DateTime<Utc>,
        today: &Report,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = self.state.read(cx);
        let running = state.running_project().map(|p| p.id);
        let projects: Vec<Project> = state.projects().to_vec();

        let rows: Vec<AnyElement> = projects
            .iter()
            .filter(|project| !project.archived)
            .enumerate()
            .map(|(index, project)| self.project_row(project, index, running, today, cx))
            .collect();

        let archived: Vec<AnyElement> = projects
            .iter()
            .filter(|project| project.archived)
            .map(|project| self.archived_row(project, cx))
            .collect();

        div()
            .flex()
            .flex_col()
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_none()
            .bg(theme::margin())
            .border_r_1()
            .border_color(theme::rule())
            .child(self.wordmark(running.is_some()))
            .child(hairline())
            .child(
                div()
                    .id("project-list")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_y_scroll()
                    .py(px(10.))
                    .children(if rows.is_empty() && archived.is_empty() {
                        vec![self.rail_empty_note()]
                    } else {
                        rows
                    })
                    .children(if archived.is_empty() {
                        None
                    } else {
                        Some(
                            div()
                                .px(px(18.))
                                .pt(px(16.))
                                .pb(px(7.))
                                .child(label("archived"))
                                .into_any_element(),
                        )
                    })
                    .children(archived),
            )
            .child(hairline())
            .child(self.rail_footer(now, cx))
            .into_any_element()
    }

    /// The app's mark, set at the far end of the top strip.
    ///
    /// The system draws the traffic lights into the near corner of this strip,
    /// so the mark keeps to the right where nothing can land on top of it. The
    /// dot fills burnt sienna while the clock runs, which makes the window's
    /// state readable from the corner of the eye.
    fn wordmark(&self, running: bool) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(7.))
            .h(px(TITLE_STRIP))
            .px(px(18.))
            .flex_none()
            .child(
                div()
                    .size(px(6.))
                    .rounded_full()
                    .bg(if running {
                        theme::ember()
                    } else {
                        theme::faint()
                    })
                    .flex_none(),
            )
            .child(
                div()
                    .text_size(px(9.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::muted())
                    .child(theme::letterspaced("HOURGLASS")),
            )
            .into_any_element()
    }

    fn rail_empty_note(&self) -> AnyElement {
        div()
            .px(px(18.))
            .py(px(12.))
            .font(self.fonts.serif(FontWeight::NORMAL))
            .text_size(px(13.))
            .text_color(theme::faint())
            .child("No projects yet. Press N to add the first one.")
            .into_any_element()
    }

    /// One project. Click starts it; the row shows today's time against it.
    fn project_row(
        &self,
        project: &Project,
        index: usize,
        running: Option<ProjectId>,
        today: &Report,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = project.id;
        let is_running = running == Some(id);

        // A field open on this row means it is being renamed.
        if let Mode::RenamingProject(editing, field) = &self.mode
            && *editing == id
        {
            return self.rail_field(field, project.color);
        }
        if let Mode::ConfirmingDelete(pending) = &self.mode
            && *pending == id
        {
            return self.delete_confirmation(project, cx);
        }

        let seconds = today
            .totals
            .iter()
            .find(|total| total.project_id == id)
            .map(|total| total.seconds)
            .unwrap_or(0);

        let shortcut = if index < 8 {
            (index + 1).to_string()
        } else {
            String::new()
        };

        div()
            .id(("project", id.0 as u64))
            .group("project-row")
            .relative()
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(32.))
            .mx(px(10.))
            .px(px(8.))
            .rounded(px(3.))
            .cursor_pointer()
            .when(is_running, |row| row.bg(theme::ember_wash(0x14)))
            .hover(|row| row.bg(theme::wash(0x08)))
            .active(|row| row.opacity(0.7))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.edit(cx, |state| state.start(id, Utc::now()));
                cx.notify();
            }))
            // Right-click is the only place rename and delete live; they are
            // rare next to starting a timer and should not take up room.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    let current = this
                        .state
                        .read(cx)
                        .project(id)
                        .map(|project| project.name.clone())
                        .unwrap_or_default();
                    this.mode = Mode::RenamingProject(id, TextField::with_text(current));
                    cx.notify();
                }),
            )
            .child(
                div()
                    .w(px(9.))
                    .flex_none()
                    .font(self.fonts.numeric(FontWeight::NORMAL))
                    .text_size(px(9.5))
                    .text_color(theme::faint())
                    .child(shortcut),
            )
            .child(if is_running {
                dot(project.color, 7.)
            } else {
                ring(project.color, 7.)
            })
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .font(self.fonts.serif(if is_running {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    }))
                    .text_size(px(13.5))
                    .text_color(theme::ink())
                    .child(project.name.clone()),
            )
            .child(
                div()
                    .font(self.fonts.numeric(FontWeight::NORMAL))
                    .text_size(px(11.))
                    .text_color(if seconds > 0 {
                        theme::muted()
                    } else {
                        theme::faint()
                    })
                    .group_hover("project-row", |style| style.opacity(0.))
                    .child(if seconds > 0 {
                        format_duration(seconds)
                    } else {
                        "—".to_string()
                    }),
            )
            // Archive and delete sit on top of the duration and only appear on
            // hover, so the rail reads as a list of projects rather than a
            // list of controls.
            .child(
                div()
                    .absolute()
                    .right(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .opacity(0.)
                    .group_hover("project-row", |style| style.opacity(1.))
                    .child(
                        div()
                            .id(("archive", id.0 as u64))
                            .px(px(5.))
                            .py(px(2.))
                            .rounded(px(3.))
                            .text_size(px(10.))
                            .text_color(theme::muted())
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::wash(0x10)).text_color(theme::ink()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.edit(cx, |state| {
                                    state.set_project_archived(id, true, Utc::now())
                                });
                                cx.notify();
                            }))
                            .child("Archive"),
                    )
                    .child(
                        div()
                            .id(("ask-delete", id.0 as u64))
                            .px(px(5.))
                            .py(px(2.))
                            .rounded(px(3.))
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .cursor_pointer()
                            .hover(|style| style.bg(theme::wash(0x10)).text_color(theme::alert()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.mode = Mode::ConfirmingDelete(id);
                                cx.notify();
                            }))
                            .child("×"),
                    ),
            )
            .into_any_element()
    }

    /// An archived project: history only, one click to bring it back.
    fn archived_row(&self, project: &Project, cx: &mut Context<Self>) -> AnyElement {
        let id = project.id;
        div()
            .id(("archived", id.0 as u64))
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(28.))
            .mx(px(10.))
            .px(px(8.))
            .rounded(px(3.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::wash(0x08)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.edit(cx, |state| {
                    state.set_project_archived(id, false, Utc::now())
                });
                cx.notify();
            }))
            .child(div().w(px(9.)).flex_none())
            .child(ring(project.color, 7.))
            .child(
                div()
                    .flex_1()
                    .font(self.fonts.serif(FontWeight::NORMAL))
                    .text_size(px(12.5))
                    .text_color(theme::faint())
                    .child(project.name.clone()),
            )
            .into_any_element()
    }

    /// The row turns into a text field while a name is being typed.
    fn rail_field(&self, field: &TextField, color: u8) -> AnyElement {
        let (before, after) = field.split_at_caret();

        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(32.))
            .mx(px(10.))
            .px(px(8.))
            .rounded(px(3.))
            .bg(theme::raised())
            .border_1()
            .border_color(theme::swatch_soft(color, 0.5))
            .child(div().w(px(9.)).flex_none())
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_1()
                    .overflow_hidden()
                    .font(self.fonts.serif(FontWeight::NORMAL))
                    .text_size(px(13.5))
                    .text_color(theme::ink())
                    .child(before.to_string())
                    .child(
                        div()
                            .w(px(1.5))
                            .h(px(15.))
                            .mx(px(1.))
                            .bg(theme::ember())
                            .flex_none(),
                    )
                    .child(after.to_string()),
            )
            .into_any_element()
    }

    /// Deleting removes recorded hours, so the row asks before doing it.
    fn delete_confirmation(&self, project: &Project, cx: &mut Context<Self>) -> AnyElement {
        let id = project.id;
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(32.))
            .mx(px(10.))
            .px(px(8.))
            .rounded(px(3.))
            .bg(theme::raised())
            .border_1()
            .border_color(theme::rule())
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .font(self.fonts.serif(FontWeight::NORMAL))
                    .text_size(px(12.5))
                    .text_color(theme::ink())
                    .child(format!("Delete {}?", project.name)),
            )
            .child(
                div()
                    .id(("keep-project", id.0 as u64))
                    .px(px(7.))
                    .py(px(3.))
                    .rounded(px(3.))
                    .text_size(px(11.))
                    .text_color(theme::muted())
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::wash(0x0F)).text_color(theme::ink()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.mode = Mode::Browsing;
                        cx.notify();
                    }))
                    .child("Keep"),
            )
            .child(
                div()
                    .id(("confirm-delete", id.0 as u64))
                    .px(px(7.))
                    .py(px(3.))
                    .rounded(px(3.))
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme::alert())
                    .cursor_pointer()
                    .hover(|style| style.bg(theme::wash(0x0F)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.edit(cx, |state| state.delete_project(id, Utc::now()));
                        this.mode = Mode::Browsing;
                        cx.notify();
                    }))
                    .child("Delete"),
            )
            .into_any_element()
    }

    /// New project and export live at the foot of the rail.
    fn rail_footer(&self, now: DateTime<Utc>, cx: &mut Context<Self>) -> AnyElement {
        if let Mode::AddingProject(field) = &self.mode {
            return div()
                .py(px(10.))
                .flex_none()
                .child(self.rail_field(field, self.next_swatch(cx)))
                .into_any_element();
        }

        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(4.))
            .p(px(10.))
            .flex_none()
            .child(
                ghost_button("new-project", "+ New project").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.mode = Mode::AddingProject(TextField::new());
                        cx.notify();
                    },
                )),
            )
            .child(
                ghost_button("export", "Export CSV").on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.edit(cx, |state| {
                            state.export_csv(now);
                        });
                        cx.notify();
                    },
                )),
            )
            .into_any_element()
    }

    /// The swatch a new project would be given, so the field previews it.
    fn next_swatch(&self, cx: &App) -> u8 {
        (self.state.read(cx).projects().len() % 8) as u8
    }
}
