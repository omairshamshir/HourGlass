//! The form for hours that were worked but never timed.
//!
//! Four fields on one line, moved between with Tab. It is deliberately not a
//! dialog: it opens as a band across the page, keeps the day's figures visible
//! above it, and never steals the window.
//!
//! The form itself holds no opinion about whether an entry is valid. It
//! collects four strings and hands them to [`crate::state::AppState`], which
//! owns every rule about what counts as honest hours.

use crate::theme::{self, Fonts};
use crate::ui::kit::{action_button, ghost_button, label};
use crate::ui::root::{MARGIN, Root};
use crate::ui::text_field::{FieldAction, TextField};
use gpui::{
    Context, Div, FontWeight, IntoElement, Keystroke, ParentElement, StatefulInteractiveElement,
    Styled, div, px,
};

/// Which of the four fields has the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftField {
    Project,
    Date,
    Start,
    End,
}

impl DraftField {
    /// Tab order, which is also the reading order of the row.
    const ORDER: [DraftField; 4] = [
        DraftField::Project,
        DraftField::Date,
        DraftField::Start,
        DraftField::End,
    ];

    fn step(self, forward: bool) -> Self {
        let at = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        let count = Self::ORDER.len();
        let next = if forward {
            (at + 1) % count
        } else {
            (at + count - 1) % count
        };
        Self::ORDER[next]
    }

    fn caption(self) -> &'static str {
        match self {
            DraftField::Project => "project",
            DraftField::Date => "date",
            DraftField::Start => "from",
            DraftField::End => "to",
        }
    }

    /// Shown in the empty field, as an example rather than an instruction.
    fn placeholder(self) -> &'static str {
        match self {
            DraftField::Project => "Atlas",
            DraftField::Date => "today",
            DraftField::Start => "9:30",
            DraftField::End => "11:00",
        }
    }

    /// The project name needs room; the times need almost none.
    fn width(self) -> f32 {
        match self {
            DraftField::Project => 190.,
            DraftField::Date => 140.,
            DraftField::Start | DraftField::End => 92.,
        }
    }
}

/// Four fields and a caret, being filled in.
pub struct EntryDraft {
    project: TextField,
    date: TextField,
    start: TextField,
    end: TextField,
    focused: DraftField,
}

impl EntryDraft {
    /// Open the form, pre-filled with the project the user was most recently
    /// working on. That is nearly always the one they forgot to time, and it
    /// puts the caret straight on the first field they actually have to think
    /// about.
    pub fn new(project: Option<String>) -> Self {
        let known = project.is_some();
        EntryDraft {
            project: TextField::with_text(project.unwrap_or_default()),
            date: TextField::with_text("today"),
            start: TextField::new(),
            end: TextField::new(),
            focused: if known {
                DraftField::Start
            } else {
                DraftField::Project
            },
        }
    }

    fn field(&self, which: DraftField) -> &TextField {
        match which {
            DraftField::Project => &self.project,
            DraftField::Date => &self.date,
            DraftField::Start => &self.start,
            DraftField::End => &self.end,
        }
    }

    fn field_mut(&mut self, which: DraftField) -> &mut TextField {
        match which {
            DraftField::Project => &mut self.project,
            DraftField::Date => &mut self.date,
            DraftField::Start => &mut self.start,
            DraftField::End => &mut self.end,
        }
    }

    /// The four strings, exactly as typed.
    pub fn values(&self) -> (String, String, String, String) {
        (
            self.project.text().to_string(),
            self.date.text().to_string(),
            self.start.text().to_string(),
            self.end.text().to_string(),
        )
    }

    /// Route a keystroke to the focused field, after taking the keys that move
    /// between fields. Tab is the conventional one; the arrows are there
    /// because the row reads as a single line and people try them.
    pub fn handle(&mut self, keystroke: &Keystroke, clipboard: Option<&str>) -> FieldAction {
        match keystroke.key.as_str() {
            "tab" => {
                self.focused = self.focused.step(!keystroke.modifiers.shift);
                return FieldAction::Edited;
            }
            "down" => {
                self.focused = self.focused.step(true);
                return FieldAction::Edited;
            }
            "up" => {
                self.focused = self.focused.step(false);
                return FieldAction::Edited;
            }
            _ => {}
        }

        let focused = self.focused;
        self.field_mut(focused).handle(keystroke, clipboard)
    }
}

impl Root {
    /// The add-hours band, shown while the form is open.
    pub(super) fn entry_form(&self, draft: &EntryDraft, cx: &mut Context<Self>) -> impl IntoElement {
        let fields = DraftField::ORDER.map(|which| {
            draft_field(
                &self.fonts,
                which,
                draft.field(which),
                draft.focused == which,
            )
        });

        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(MARGIN))
            .py(px(16.))
            .flex_none()
            // The band sits in the margin tone rather than on a card, so it
            // reads as part of the page folding open.
            .bg(theme::margin())
            .border_b_1()
            .border_color(theme::rule())
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(px(20.))
                    .child(div().flex().items_end().gap(px(14.)).children(fields))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(action_button("save-hours", "Add", true).on_click(
                                cx.listener(|this, _, _, cx| this.submit_entry_draft(cx)),
                            ))
                            .child(ghost_button("cancel-hours", "Cancel").on_click(
                                cx.listener(|this, _, _, cx| this.close_entry_draft(cx)),
                            )),
                    ),
            )
            .child(
                div()
                    .font(self.fonts.serif(FontWeight::NORMAL))
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(
                        "Tab moves between fields. Times take 9:30, 0930, or 9:30am. \
                         Return saves, Escape closes.",
                    ),
            )
    }
}

/// One labelled input. The focused field is the only one drawn with a caret,
/// and its rule thickens to ink so the eye finds it without a colour cue.
fn draft_field(fonts: &Fonts, which: DraftField, field: &TextField, focused: bool) -> Div {
    let (before, after) = field.split_at_caret();
    let showing_placeholder = field.text().is_empty() && !focused;

    let mut line = div()
        .flex()
        .items_center()
        .h(px(30.))
        .px(px(9.))
        .w(px(which.width()))
        .bg(theme::raised())
        .border_1()
        .border_color(if focused {
            theme::ink()
        } else {
            theme::rule()
        })
        .font(fonts.serif(FontWeight::NORMAL))
        .text_size(px(13.5))
        .overflow_hidden();

    if showing_placeholder {
        line = line
            .text_color(theme::faint())
            .child(which.placeholder().to_string());
    } else {
        line = line
            .text_color(theme::ink())
            .child(before.to_string())
            .children(focused.then(|| {
                div()
                    .w(px(1.5))
                    .h(px(15.))
                    .mx(px(1.))
                    .bg(theme::ember())
                    .flex_none()
            }))
            .child(after.to_string());
    }

    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(label(which.caption()))
        .child(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> EntryDraft {
        EntryDraft::new(None)
    }

    fn key(name: &str) -> Keystroke {
        Keystroke::parse(name).expect("a parseable keystroke")
    }

    /// A keystroke that carries a character, as one from the window would.
    /// `Keystroke::parse` fills in the key but not the text it produces.
    fn typed(character: &str) -> Keystroke {
        let mut keystroke = key(character);
        keystroke.key_char = Some(character.to_string());
        keystroke
    }

    #[test]
    fn an_opened_form_defaults_the_date_to_today() {
        assert_eq!(draft().date.text(), "today");
    }

    #[test]
    fn a_known_project_is_filled_in_and_skipped_past() {
        let draft = EntryDraft::new(Some("Atlas".to_string()));
        assert_eq!(draft.project.text(), "Atlas");
        assert_eq!(draft.focused, DraftField::Start);
    }

    #[test]
    fn without_a_project_the_caret_starts_on_the_project_field() {
        assert_eq!(draft().focused, DraftField::Project);
    }

    #[test]
    fn tab_walks_the_fields_and_wraps_around() {
        let mut draft = draft();
        for expected in [
            DraftField::Date,
            DraftField::Start,
            DraftField::End,
            DraftField::Project,
        ] {
            draft.handle(&key("tab"), None);
            assert_eq!(draft.focused, expected);
        }
    }

    #[test]
    fn shift_tab_walks_the_other_way() {
        let mut draft = draft();
        draft.handle(&key("shift-tab"), None);
        assert_eq!(draft.focused, DraftField::End);
    }

    #[test]
    fn typing_lands_in_the_focused_field_only() {
        let mut draft = draft();
        draft.handle(&key("tab"), None);
        draft.handle(&key("tab"), None);
        assert_eq!(draft.focused, DraftField::Start);

        draft.handle(&typed("9"), None);
        assert_eq!(draft.start.text(), "9");
        assert_eq!(draft.project.text(), "");
    }

    #[test]
    fn return_and_escape_reach_the_caller_rather_than_the_field() {
        let mut draft = draft();
        assert_eq!(draft.handle(&key("enter"), None), FieldAction::Submit);
        assert_eq!(draft.handle(&key("escape"), None), FieldAction::Cancel);
    }
}
