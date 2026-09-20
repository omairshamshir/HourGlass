//! A single-line text field, just enough for naming a project.
//!
//! gpui ships no text input, and a full one means selection, IME, and mouse
//! caret placement. A project name needs none of that, so this covers typing,
//! deleting, moving the caret, and pasting, and nothing else.

use gpui::Keystroke;

/// What the caller should do after a keystroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldAction {
    /// The text changed; redraw.
    Edited,
    /// Return was pressed.
    Submit,
    /// Escape was pressed.
    Cancel,
    /// The keystroke means nothing here; let it through.
    Ignored,
}

/// Editable text and a caret position, measured in bytes.
#[derive(Debug, Clone, Default)]
pub struct TextField {
    value: String,
    caret: usize,
}

impl TextField {
    pub fn new() -> Self {
        Self::default()
    }

    /// A field pre-filled with existing text, caret at the end.
    pub fn with_text(value: impl Into<String>) -> Self {
        let value = value.into();
        let caret = value.len();
        TextField { value, caret }
    }

    pub fn text(&self) -> &str {
        &self.value
    }

    /// True when the field holds nothing worth submitting.
    pub fn is_empty(&self) -> bool {
        self.value.trim().is_empty()
    }

    /// The text on either side of the caret, for drawing it in between.
    pub fn split_at_caret(&self) -> (&str, &str) {
        self.value.split_at(self.caret)
    }

    /// Apply a keystroke. `clipboard` is the current clipboard text, read by
    /// the caller so this stays free of platform calls.
    pub fn handle(&mut self, keystroke: &Keystroke, clipboard: Option<&str>) -> FieldAction {
        let modifiers = keystroke.modifiers;

        match keystroke.key.as_str() {
            "enter" => return FieldAction::Submit,
            "escape" => return FieldAction::Cancel,
            "backspace" if modifiers.alt => {
                self.delete_word_before_caret();
                return FieldAction::Edited;
            }
            "backspace" if modifiers.platform => {
                self.value.replace_range(..self.caret, "");
                self.caret = 0;
                return FieldAction::Edited;
            }
            "backspace" => {
                self.delete_char_before_caret();
                return FieldAction::Edited;
            }
            "delete" => {
                self.delete_char_after_caret();
                return FieldAction::Edited;
            }
            "left" => {
                self.caret = if modifiers.platform {
                    0
                } else {
                    self.previous_boundary()
                };
                return FieldAction::Edited;
            }
            "right" => {
                self.caret = if modifiers.platform {
                    self.value.len()
                } else {
                    self.next_boundary()
                };
                return FieldAction::Edited;
            }
            "home" => {
                self.caret = 0;
                return FieldAction::Edited;
            }
            "end" => {
                self.caret = self.value.len();
                return FieldAction::Edited;
            }
            "v" if modifiers.platform => {
                if let Some(pasted) = clipboard {
                    // A pasted newline would silently become part of the name.
                    let flattened: String =
                        pasted.chars().filter(|c| !c.is_control()).collect();
                    self.insert(&flattened);
                }
                return FieldAction::Edited;
            }
            "a" | "c" | "x" | "z" | "q" | "w" if modifiers.platform => {
                return FieldAction::Ignored;
            }
            _ => {}
        }

        // Anything that produced a printable character is typed text.
        if let Some(typed) = keystroke.key_char.as_deref()
            && !typed.is_empty()
            && !typed.chars().any(char::is_control)
        {
            self.insert(typed);
            return FieldAction::Edited;
        }

        FieldAction::Ignored
    }

    fn insert(&mut self, text: &str) {
        self.value.insert_str(self.caret, text);
        self.caret += text.len();
    }

    fn delete_char_before_caret(&mut self) {
        let start = self.previous_boundary();
        if start != self.caret {
            self.value.replace_range(start..self.caret, "");
            self.caret = start;
        }
    }

    fn delete_char_after_caret(&mut self) {
        let end = self.next_boundary();
        if end != self.caret {
            self.value.replace_range(self.caret..end, "");
        }
    }

    fn delete_word_before_caret(&mut self) {
        let head = &self.value[..self.caret];
        let trimmed = head.trim_end();
        let start = trimmed
            .rfind(char::is_whitespace)
            .map(|index| index + 1)
            .unwrap_or(0);
        self.value.replace_range(start..self.caret, "");
        self.caret = start;
    }

    /// The char boundary before the caret, so multi-byte characters are never
    /// split in half.
    fn previous_boundary(&self) -> usize {
        self.value[..self.caret]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    fn next_boundary(&self) -> usize {
        self.value[self.caret..]
            .chars()
            .next()
            .map(|c| self.caret + c.len_utf8())
            .unwrap_or(self.caret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Modifiers;

    fn key(key: &str) -> Keystroke {
        Keystroke {
            modifiers: Modifiers::default(),
            key: key.to_string(),
            key_char: None,
        }
    }

    fn typed(character: &str) -> Keystroke {
        Keystroke {
            modifiers: Modifiers::default(),
            key: character.to_string(),
            key_char: Some(character.to_string()),
        }
    }

    fn field_with(text: &str) -> TextField {
        TextField::with_text(text)
    }

    #[test]
    fn typing_appends_at_the_caret() {
        let mut field = TextField::new();
        for character in ["A", "t", "l"] {
            assert_eq!(field.handle(&typed(character), None), FieldAction::Edited);
        }
        assert_eq!(field.text(), "Atl");
    }

    #[test]
    fn backspace_removes_the_character_before_the_caret() {
        let mut field = field_with("Atlas");
        field.handle(&key("backspace"), None);
        assert_eq!(field.text(), "Atla");
    }

    #[test]
    fn backspace_on_an_empty_field_is_harmless() {
        let mut field = TextField::new();
        field.handle(&key("backspace"), None);
        assert_eq!(field.text(), "");
    }

    #[test]
    fn the_caret_moves_without_changing_the_text() {
        let mut field = field_with("Atlas");
        field.handle(&key("left"), None);
        field.handle(&key("left"), None);
        field.handle(&typed("X"), None);
        assert_eq!(field.text(), "AtlXas");
    }

    #[test]
    fn multi_byte_characters_are_deleted_whole() {
        let mut field = field_with("Café");
        field.handle(&key("backspace"), None);
        assert_eq!(field.text(), "Caf");
    }

    #[test]
    fn option_backspace_deletes_the_previous_word() {
        let mut field = field_with("Atlas Redesign");
        let mut keystroke = key("backspace");
        keystroke.modifiers.alt = true;
        field.handle(&keystroke, None);
        assert_eq!(field.text(), "Atlas ");
    }

    #[test]
    fn pasting_strips_newlines_so_a_name_stays_one_line() {
        let mut field = TextField::new();
        let mut keystroke = key("v");
        keystroke.modifiers.platform = true;
        field.handle(&keystroke, Some("Atlas\nRedesign"));
        assert_eq!(field.text(), "AtlasRedesign");
    }

    #[test]
    fn enter_and_escape_are_reported_to_the_caller() {
        let mut field = field_with("Atlas");
        assert_eq!(field.handle(&key("enter"), None), FieldAction::Submit);
        assert_eq!(field.handle(&key("escape"), None), FieldAction::Cancel);
        assert_eq!(field.text(), "Atlas");
    }

    #[test]
    fn a_field_of_spaces_counts_as_empty() {
        assert!(field_with("   ").is_empty());
        assert!(!field_with(" a ").is_empty());
    }

    #[test]
    fn select_all_is_left_for_the_system_to_handle() {
        let mut field = field_with("Atlas");
        let mut keystroke = key("a");
        keystroke.modifiers.platform = true;
        assert_eq!(field.handle(&keystroke, None), FieldAction::Ignored);
        assert_eq!(field.text(), "Atlas");
    }

    #[test]
    fn the_caret_splits_the_text_where_it_sits() {
        let mut field = field_with("Atlas");
        field.handle(&key("left"), None);
        assert_eq!(field.split_at_caret(), ("Atla", "s"));
    }
}
