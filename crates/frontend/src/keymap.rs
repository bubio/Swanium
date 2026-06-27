//! Translate Slint key events into WonderSwan [`Button`]s.
//!
//! Slint delivers each key as the text it produces; printable keys arrive as
//! their character, and named keys (arrows, Enter) arrive as the corresponding
//! [`slint::platform::Key`] code. We map the default horizontal-orientation
//! layout: arrow keys drive the X-pad, `W`/`A`/`S`/`D` the Y-pad, `Z`/`X` the
//! B/A buttons, and Enter is Start. Unmapped keys return `None`.

use input::Button;
use slint::platform::Key;

/// Map the text of a Slint key event to a WonderSwan button, if bound.
pub fn button_from_text(text: &str) -> Option<Button> {
    let key = text.chars().next()?;
    match key {
        k if k == char::from(Key::UpArrow) => Some(Button::X1),
        k if k == char::from(Key::RightArrow) => Some(Button::X2),
        k if k == char::from(Key::DownArrow) => Some(Button::X3),
        k if k == char::from(Key::LeftArrow) => Some(Button::X4),
        k if k == char::from(Key::Return) => Some(Button::Start),
        'w' | 'W' => Some(Button::Y1),
        'd' | 'D' => Some(Button::Y2),
        's' | 'S' => Some(Button::Y3),
        'a' | 'A' => Some(Button::Y4),
        'x' | 'X' => Some(Button::A),
        'z' | 'Z' => Some(Button::B),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_text(key: Key) -> String {
        char::from(key).to_string()
    }

    #[test]
    fn up_arrow_maps_to_x1() {
        assert_eq!(button_from_text(&key_text(Key::UpArrow)), Some(Button::X1));
    }

    #[test]
    fn left_arrow_maps_to_x4() {
        assert_eq!(
            button_from_text(&key_text(Key::LeftArrow)),
            Some(Button::X4)
        );
    }

    #[test]
    fn enter_maps_to_start() {
        assert_eq!(
            button_from_text(&key_text(Key::Return)),
            Some(Button::Start)
        );
    }

    #[test]
    fn z_maps_to_b() {
        assert_eq!(button_from_text("z"), Some(Button::B));
    }

    #[test]
    fn x_maps_to_a() {
        assert_eq!(button_from_text("x"), Some(Button::A));
    }

    #[test]
    fn uppercase_letter_is_accepted() {
        assert_eq!(button_from_text("W"), Some(Button::Y1));
    }

    #[test]
    fn unmapped_key_returns_none() {
        assert_eq!(button_from_text("q"), None);
    }

    #[test]
    fn empty_text_returns_none() {
        assert_eq!(button_from_text(""), None);
    }
}
