//! Mapping host input onto the WonderSwan key matrix.
//!
//! The core exposes its keys as [`swanium_core::keypad::KeyState`]. This crate
//! sits between a host input source and that representation, providing a
//! backend-neutral [`Button`] enum for the eleven hardware keys and helpers to
//! fold a set of pressed buttons into a [`KeyState`]. The concrete keyboard and
//! gilrs gamepad backends — which depend on the windowing/gamepad libraries the
//! core must never see — are wired in a later step (see
//! `docs/dev/DevelopmentPlan.md` Phase 7 後続課題). The frontend translates its
//! own keycodes into [`Button`]s and feeds them here, so this layer stays pure
//! and unit-testable.

use swanium_core::keypad::KeyState;

/// A logical WonderSwan key, independent of the host input device.
///
/// The X-pad is the primary direction pad in horizontal orientation; the Y-pad
/// is the secondary pad (used as the direction pad in vertical orientation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    /// X-pad up.
    X1,
    /// X-pad right.
    X2,
    /// X-pad down.
    X3,
    /// X-pad left.
    X4,
    /// Y-pad 1 (top in vertical orientation).
    Y1,
    /// Y-pad 2 (right in vertical orientation).
    Y2,
    /// Y-pad 3 (bottom in vertical orientation).
    Y3,
    /// Y-pad 4 (left in vertical orientation).
    Y4,
    /// Start button.
    Start,
    /// A button.
    A,
    /// B button.
    B,
}

impl Button {
    /// Every hardware key, in a stable order (useful for binding tables/tests).
    pub const ALL: [Button; 11] = [
        Button::X1,
        Button::X2,
        Button::X3,
        Button::X4,
        Button::Y1,
        Button::Y2,
        Button::Y3,
        Button::Y4,
        Button::Start,
        Button::A,
        Button::B,
    ];

    /// The single-key [`KeyState`] this button corresponds to.
    pub fn key(self) -> KeyState {
        match self {
            Button::X1 => KeyState::X1,
            Button::X2 => KeyState::X2,
            Button::X3 => KeyState::X3,
            Button::X4 => KeyState::X4,
            Button::Y1 => KeyState::Y1,
            Button::Y2 => KeyState::Y2,
            Button::Y3 => KeyState::Y3,
            Button::Y4 => KeyState::Y4,
            Button::Start => KeyState::START,
            Button::A => KeyState::A,
            Button::B => KeyState::B,
        }
    }
}

/// Fold a collection of pressed [`Button`]s into a single [`KeyState`].
pub fn keys_from(buttons: impl IntoIterator<Item = Button>) -> KeyState {
    buttons
        .into_iter()
        .fold(KeyState::NONE, |acc, button| acc | button.key())
}

/// A suggested default keyboard binding, as `(key name, button)` pairs.
///
/// The names are backend-neutral strings (matching common winit `KeyCode`
/// debug spellings) so the frontend can resolve them against whatever
/// windowing library it uses without this crate depending on one. Horizontal
/// orientation: the arrow keys drive the X-pad.
pub fn default_keyboard_bindings() -> [(&'static str, Button); 11] {
    [
        ("ArrowUp", Button::X1),
        ("ArrowRight", Button::X2),
        ("ArrowDown", Button::X3),
        ("ArrowLeft", Button::X4),
        ("KeyW", Button::Y1),
        ("KeyD", Button::Y2),
        ("KeyS", Button::Y3),
        ("KeyA", Button::Y4),
        ("Enter", Button::Start),
        ("KeyX", Button::A),
        ("KeyZ", Button::B),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_maps_to_matching_keystate() {
        assert_eq!(Button::Start.key(), KeyState::START);
    }

    #[test]
    fn keys_from_empty_is_none() {
        assert_eq!(keys_from([]), KeyState::NONE);
    }

    #[test]
    fn keys_from_single_button() {
        assert_eq!(keys_from([Button::A]), KeyState::A);
    }

    #[test]
    fn keys_from_combines_buttons() {
        assert_eq!(
            keys_from([Button::X1, Button::B]),
            KeyState::X1 | KeyState::B
        );
    }

    #[test]
    fn keys_from_is_idempotent_for_repeats() {
        assert_eq!(keys_from([Button::A, Button::A]), KeyState::A);
    }

    #[test]
    fn all_lists_eleven_keys() {
        assert_eq!(Button::ALL.len(), 11);
    }

    #[test]
    fn all_buttons_are_distinct_keys() {
        let combined = keys_from(Button::ALL);
        assert_eq!(combined.bits().count_ones(), 11);
    }

    #[test]
    fn default_bindings_cover_every_button() {
        let mut bound: Vec<Button> = default_keyboard_bindings()
            .iter()
            .map(|(_, b)| *b)
            .collect();
        bound.sort_by_key(|b| format!("{b:?}"));
        let mut all = Button::ALL.to_vec();
        all.sort_by_key(|b| format!("{b:?}"));
        assert_eq!(bound, all);
    }

    #[test]
    fn default_bindings_have_no_duplicate_keys() {
        let names: Vec<&str> = default_keyboard_bindings()
            .iter()
            .map(|(n, _)| *n)
            .collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(names.len(), unique.len());
    }
}
