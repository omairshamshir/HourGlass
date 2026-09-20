//! Small shared pieces. Structure here comes from rules and whitespace rather
//! than from boxes: no cards, no shadows, one accent.

use crate::theme;
use gpui::{
    AnyElement, Div, FontStyle, FontWeight, Hsla, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};

/// A one-pixel rule. The app's only divider.
pub fn hairline() -> Div {
    div().h(px(1.)).w_full().bg(theme::rule())
}

/// A section marker, set as spaced capitals the way a printed table heads its
/// columns. Short words only: the spacing makes long ones unreadable.
pub fn label(text: &str) -> Div {
    div()
        .text_size(px(9.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::muted())
        .child(theme::letterspaced(&text.to_uppercase()))
}

/// A project's colour, as a small filled circle.
pub fn dot(color_index: u8, size: f32) -> Div {
    div()
        .size(px(size))
        .rounded_full()
        .bg(theme::swatch(color_index))
        .flex_none()
}

/// A ring rather than a filled dot, for a project that is not running.
pub fn ring(color_index: u8, size: f32) -> Div {
    div()
        .size(px(size))
        .rounded_full()
        .border_1()
        .border_color(theme::swatch_soft(color_index, 0.55))
        .flex_none()
}

/// A number set in a face with tabular figures, so digits never change width.
///
/// Every figure in the app goes through here. A clock whose seconds column
/// jitters once a second is the fastest way to make software feel cheap.
pub fn numeral(
    font: gpui::Font,
    text: impl Into<SharedString>,
    size: f32,
    color: impl Into<Hsla>,
) -> Div {
    div()
        .font(font)
        .text_size(px(size))
        .text_color(color.into())
        .child(text.into())
}

/// The primary action: start or stop. Burnt sienna while running, quiet
/// otherwise.
pub fn action_button(
    id: &'static str,
    text: impl Into<SharedString>,
    running: bool,
) -> gpui::Stateful<Div> {
    let base = div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(32.))
        .px(px(20.))
        .rounded(px(3.))
        .text_size(px(12.5))
        .cursor_pointer()
        // Feedback on the press itself, not on release, so the button never
        // feels a frame behind the finger.
        .active(|style| style.opacity(0.7))
        .child(text.into());

    if running {
        base.bg(theme::ember())
            .text_color(theme::on_ember())
            .font_weight(FontWeight::MEDIUM)
            .hover(|style| style.bg(theme::ember_hover()))
    } else {
        base.bg(theme::raised())
            .text_color(theme::ink())
            .border_1()
            .border_color(theme::rule())
            .hover(|style| style.border_color(theme::ink()))
    }
}

/// A quiet text button for secondary actions.
pub fn ghost_button(id: &'static str, text: impl Into<SharedString>) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .h(px(26.))
        .px(px(8.))
        .rounded(px(3.))
        .text_size(px(12.))
        .text_color(theme::muted())
        .cursor_pointer()
        .hover(|style| style.bg(theme::wash(0x0A)).text_color(theme::ink()))
        .active(|style| style.opacity(0.7))
        .child(text.into())
}

/// A tab in the report's range switcher.
///
/// A printed table changes section with a rule, not with a pill, so the
/// selected range is marked by the line under it.
pub fn range_tab(
    id: &'static str,
    text: &str,
    selected: bool,
) -> gpui::Stateful<Div> {
    let base = div()
        .id(id)
        .flex()
        .items_center()
        .h(px(28.))
        .mr(px(18.))
        .pb(px(5.))
        .border_b_2()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .child(theme::letterspaced(&text.to_uppercase()));

    if selected {
        base.text_color(theme::ink()).border_color(theme::ember())
    } else {
        base.text_color(theme::faint())
            .border_color(theme::rule_soft())
            .hover(|style| style.text_color(theme::muted()))
    }
}

/// An explanation shown where content would be, phrased as an invitation and
/// set in italic serif so an empty pane still reads as typeset.
pub fn empty_note(font: gpui::Font, text: impl Into<SharedString>) -> AnyElement {
    div()
        .py(px(30.))
        .font(gpui::Font {
            style: FontStyle::Italic,
            ..font
        })
        .text_size(px(14.))
        .text_color(theme::faint())
        .child(text.into())
        .into_any_element()
}

/// A small marginal note, used to say why an entry stopped on its own.
pub fn tag(text: &str) -> Div {
    div()
        .text_size(px(9.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::faint())
        .child(theme::letterspaced(&text.to_uppercase()))
}
