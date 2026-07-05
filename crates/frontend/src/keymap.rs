//! Configurable mapping from Slint key events to WonderSwan [`Button`]s.
//!
//! Slint delivers each key as the text it produces: printable keys arrive as
//! their character, and named keys (arrows, Enter) arrive as the corresponding
//! [`slint::platform::Key`] code. [`Keymap`] resolves that text against a
//! user-editable table; the default table matches the classic horizontal
//! layout (arrows drive the X-pad, `W`/`A`/`S`/`D` the Y-pad, `Z`/`X` the B/A
//! buttons, Enter is Start). Letter keys are matched case-insensitively.

use std::collections::HashMap;

use input::Button;
use slint::platform::Key;

/// A user-editable keyboard binding table (`key text → Button`).
///
/// Keys are stored lowercased so a binding matches regardless of Shift/Caps;
/// arrow and Enter codes are single non-letter chars and lowercase to
/// themselves.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    by_text: HashMap<String, Button>,
}

impl Keymap {
    /// Build a keymap from `(button, key text)` pairs (later pairs win on a
    /// duplicate key, and rebind each key to a single button).
    pub fn from_pairs(pairs: impl IntoIterator<Item = (Button, String)>) -> Self {
        let mut map = Keymap::default();
        for (button, text) in pairs {
            map.rebind(button, &text);
        }
        map
    }

    /// The built-in default layout.
    pub fn defaults() -> Self {
        Keymap::from_pairs(default_bindings())
    }

    /// Resolve a Slint key-event text to a bound button, if any.
    pub fn resolve(&self, text: &str) -> Option<Button> {
        self.by_text.get(&normalise(text)).copied()
    }

    /// Bind `button` to `text`, removing any prior use of that key so each
    /// physical key drives at most one button.
    pub fn rebind(&mut self, button: Button, text: &str) {
        let key = normalise(text);
        if key.is_empty() {
            return;
        }
        self.by_text.retain(|_, b| *b != button);
        self.by_text.insert(key, button);
    }

    /// The key text currently bound to `button`, if any.
    pub fn binding_for(&self, button: Button) -> Option<String> {
        self.by_text
            .iter()
            .find(|(_, b)| **b == button)
            .map(|(text, _)| text.clone())
    }

    /// Export the table as `(button name, key text)` pairs for persistence.
    pub fn to_config(&self) -> Vec<(String, String)> {
        self.by_text
            .iter()
            .map(|(text, b)| (b.name().to_string(), text.clone()))
            .collect()
    }
}

/// The built-in `(button, key text)` bindings, in Slint key-text form.
pub fn default_bindings() -> Vec<(Button, String)> {
    vec![
        (Button::X1, char::from(Key::UpArrow).to_string()),
        (Button::X2, char::from(Key::RightArrow).to_string()),
        (Button::X3, char::from(Key::DownArrow).to_string()),
        (Button::X4, char::from(Key::LeftArrow).to_string()),
        (Button::Start, char::from(Key::Return).to_string()),
        (Button::Y1, "w".to_string()),
        (Button::Y2, "d".to_string()),
        (Button::Y3, "s".to_string()),
        (Button::Y4, "a".to_string()),
        (Button::A, "x".to_string()),
        (Button::B, "z".to_string()),
    ]
}

/// A human-facing label for a stored key text (for the settings UI).
///
/// Named keys (arrows, Enter, Space, Backspace…) render as words; a printable
/// character renders uppercased; empty text renders as "—".
pub fn key_display(text: &str) -> String {
    if text.is_empty() {
        return "—".to_string();
    }
    let Some(c) = text.chars().next() else {
        return "—".to_string();
    };
    match c {
        k if k == char::from(Key::UpArrow) => "↑".to_string(),
        k if k == char::from(Key::DownArrow) => "↓".to_string(),
        k if k == char::from(Key::LeftArrow) => "←".to_string(),
        k if k == char::from(Key::RightArrow) => "→".to_string(),
        k if k == char::from(Key::Return) => "Enter".to_string(),
        k if k == char::from(Key::Escape) => "Esc".to_string(),
        k if k == char::from(Key::Backspace) => "Backspace".to_string(),
        k if k == char::from(Key::Tab) => "Tab".to_string(),
        ' ' => "Space".to_string(),
        other => other.to_uppercase().to_string(),
    }
}

/// Lowercase a key text so bindings ignore Shift/Caps state.
fn normalise(text: &str) -> String {
    text.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_text(key: Key) -> String {
        char::from(key).to_string()
    }

    #[test]
    fn default_up_arrow_maps_to_x1() {
        let map = Keymap::defaults();
        assert_eq!(map.resolve(&key_text(Key::UpArrow)), Some(Button::X1));
    }

    #[test]
    fn default_enter_maps_to_start() {
        let map = Keymap::defaults();
        assert_eq!(map.resolve(&key_text(Key::Return)), Some(Button::Start));
    }

    #[test]
    fn default_z_maps_to_b_case_insensitively() {
        let map = Keymap::defaults();
        assert_eq!(map.resolve("z"), Some(Button::B));
        assert_eq!(map.resolve("Z"), Some(Button::B));
    }

    #[test]
    fn unmapped_key_resolves_to_none() {
        assert_eq!(Keymap::defaults().resolve("q"), None);
    }

    #[test]
    fn empty_text_resolves_to_none() {
        assert_eq!(Keymap::defaults().resolve(""), None);
    }

    #[test]
    fn rebind_replaces_old_key_for_button() {
        let mut map = Keymap::defaults();
        map.rebind(Button::A, "q");
        assert_eq!(map.resolve("q"), Some(Button::A));
        // The previous 'x' binding for A is gone.
        assert_eq!(map.resolve("x"), None);
    }

    #[test]
    fn rebind_steals_key_from_other_button() {
        let mut map = Keymap::defaults();
        // Bind B's key ('z') to A; A now owns 'z' and B keeps nothing on 'z'.
        map.rebind(Button::A, "z");
        assert_eq!(map.resolve("z"), Some(Button::A));
    }

    #[test]
    fn binding_for_reports_current_key() {
        let map = Keymap::defaults();
        assert_eq!(map.binding_for(Button::B).as_deref(), Some("z"));
    }

    #[test]
    fn key_display_names_special_keys() {
        assert_eq!(key_display(&key_text(Key::UpArrow)), "↑");
        assert_eq!(key_display(&key_text(Key::Return)), "Enter");
        assert_eq!(key_display("z"), "Z");
        assert_eq!(key_display(""), "—");
    }
}
