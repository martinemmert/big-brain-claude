//! A minimal single-line text field driven by key-down events.
//! GPUI 0.2.2 ships no text input; Brain only needs typing, deleting and pasting.

use gpui::{Keystroke, Modifiers};

use crate::i18n::t;

/// ⌘ on macOS, Ctrl elsewhere.
pub fn primary(m: &Modifiers) -> bool {
    if cfg!(target_os = "macos") { m.platform } else { m.control }
}

pub fn primary_label() -> &'static str {
    if cfg!(target_os = "macos") { "⌘" } else { t("Strg+", "Ctrl+") }
}

pub fn shift_label() -> &'static str {
    if cfg!(target_os = "macos") { "⇧" } else { "Shift+" }
}

#[derive(Debug, PartialEq, Eq)]
pub enum InputAction {
    Changed,
    Submit,
    Cancel,
    /// Not handled by the field (e.g. arrow keys); the caller may use it.
    Ignored,
}

#[derive(Debug, Default, Clone)]
pub struct LineInput {
    pub text: String,
}

impl LineInput {
    pub fn with_text(text: &str) -> Self {
        Self { text: text.to_string() }
    }

    /// `clipboard` is only read for ⌘V (Ctrl+V outside macOS).
    pub fn handle(&mut self, keystroke: &Keystroke, clipboard: impl FnOnce() -> Option<String>) -> InputAction {
        let m = &keystroke.modifiers;
        match keystroke.key.as_str() {
            "enter" => return InputAction::Submit,
            "escape" => return InputAction::Cancel,
            "backspace" if primary(m) || m.alt => {
                // ⌘⌫ clears, ⌥⌫ removes the last word.
                if primary(m) {
                    self.text.clear();
                } else {
                    let trimmed = self.text.trim_end();
                    let cut = trimmed.rfind(' ').map_or(0, |i| i + 1);
                    self.text.truncate(cut);
                }
                return InputAction::Changed;
            }
            "backspace" => {
                self.text.pop();
                return InputAction::Changed;
            }
            "v" if primary(m) => {
                if let Some(pasted) = clipboard() {
                    self.text.push_str(&pasted.replace(['\n', '\r'], " "));
                    return InputAction::Changed;
                }
                return InputAction::Ignored;
            }
            _ => {}
        }
        if m.platform || m.control {
            return InputAction::Ignored;
        }
        match keystroke.key_char.as_deref() {
            Some(ch) if !ch.is_empty() && ch.chars().all(|c| !c.is_control()) => {
                self.text.push_str(ch);
                InputAction::Changed
            }
            _ => InputAction::Ignored,
        }
    }
}
