//! gilrs gamepad backend.
//!
//! This is the concrete gamepad source the core must never see: it depends on
//! [`gilrs`], pumps its event queue, and folds the current controller state
//! into a [`KeyState`] each frame. The pure mapping helpers ([`map_button`],
//! [`stick_directions`]) are split out so the host-key-to-[`Button`] policy is
//! unit-testable without a physical pad.
//!
//! Mapping (horizontal orientation): the D-pad and left analog stick drive the
//! X-pad; the right analog stick drives the Y-pad; the bottom/right face buttons
//! are B/A and the menu button is Start.

use std::collections::{HashMap, HashSet};

use gilrs::{Axis, EventType, GamepadId, Gilrs};

use crate::{keys_from, Button};
use swanium_core::keypad::KeyState;

/// Analog-stick magnitude past which an axis counts as a held direction.
///
/// 0.5 is a generous dead zone: it ignores resting-stick drift yet still
/// triggers well before the stick is pushed fully to a corner.
const AXIS_THRESHOLD: f32 = 0.5;

/// The built-in digital-button mapping, as `(gilrs button, WS button)` pairs.
///
/// The D-pad drives the X-pad; the bottom/right face buttons are B/A (the
/// SNES-style layout WonderSwan players expect); the menu button is Start.
const DEFAULT_BINDINGS: [(gilrs::Button, Button); 7] = [
    (gilrs::Button::DPadUp, Button::X1),
    (gilrs::Button::DPadRight, Button::X2),
    (gilrs::Button::DPadDown, Button::X3),
    (gilrs::Button::DPadLeft, Button::X4),
    (gilrs::Button::South, Button::B),
    (gilrs::Button::East, Button::A),
    (gilrs::Button::Start, Button::Start),
];

/// The default `gilrs button → WS button` table.
pub fn default_gamepad_bindings() -> HashMap<gilrs::Button, Button> {
    DEFAULT_BINDINGS.into_iter().collect()
}

/// A connected-gamepad input source backed by gilrs.
///
/// Construct once with [`Gamepad::open`]; call [`Gamepad::poll`] once per frame
/// to drain pending events and read back the keys currently held across every
/// connected pad.
pub struct Gamepad {
    gilrs: Gilrs,
    /// Digital buttons currently held, keyed by pad so one controller's
    /// release or disconnect never clears another's still-held keys.
    held: HashMap<GamepadId, HashSet<Button>>,
    /// Active `gilrs button → WS button` map; overridable via [`Gamepad::set_bindings`].
    bindings: HashMap<gilrs::Button, Button>,
}

impl Gamepad {
    /// Initialise the gamepad subsystem.
    ///
    /// Fails if the platform has no usable gamepad backend; the frontend treats
    /// that as "no controller" and keeps running on keyboard input alone.
    ///
    /// The error is boxed because [`gilrs::Error`] is a large enum and would
    /// otherwise bloat every `Result` returned from here.
    pub fn open() -> Result<Self, Box<gilrs::Error>> {
        Ok(Self {
            gilrs: Gilrs::new().map_err(Box::new)?,
            held: HashMap::new(),
            bindings: default_gamepad_bindings(),
        })
    }

    /// Replace the digital-button map with a custom `gilrs button → WS button` table.
    ///
    /// Clears any currently-held state so a rebind can't leave a key stuck under
    /// its old mapping.
    pub fn set_bindings(&mut self, bindings: HashMap<gilrs::Button, Button>) {
        self.bindings = bindings;
        self.held.clear();
    }

    /// Apply bindings given as `(WS button name, gilrs button name)` string pairs.
    ///
    /// Lets the frontend drive the map from its config without ever naming the
    /// `gilrs::Button` type. Unresolvable names are skipped.
    pub fn set_named_bindings<'a>(&mut self, pairs: impl IntoIterator<Item = (&'a str, &'a str)>) {
        let mut map = HashMap::new();
        for (ws, g) in pairs {
            if let (Some(btn), Some(gbtn)) = (Button::from_name(ws), gilrs_button_from_name(g)) {
                map.insert(gbtn, btn);
            }
        }
        self.set_bindings(map);
    }

    /// Drain pending events and return the name of the first newly-pressed,
    /// bindable button.
    ///
    /// Used by the settings UI to capture a controller button for rebinding:
    /// unlike [`poll`](Gamepad::poll) it ignores the configured map and does not
    /// touch the held-key state, so it never feeds the emulator while listening.
    /// Non-bindable buttons (outside [`gilrs_button_name`]) are ignored.
    pub fn poll_capture(&mut self) -> Option<&'static str> {
        let mut captured = None;
        while let Some(event) = self.gilrs.next_event() {
            if let EventType::ButtonPressed(button, _) = event.event {
                if captured.is_none() {
                    captured = gilrs_button_name(button);
                }
            }
        }
        captured
    }

    /// Drain pending gamepad events and return the keys currently held.
    ///
    /// Digital buttons come from the event stream; analog-stick directions are
    /// sampled from each pad's current axis values (a stick pushed past the dead
    /// zone counts as the matching direction press).
    pub fn poll(&mut self) -> KeyState {
        while let Some(event) = self.gilrs.next_event() {
            match event.event {
                EventType::ButtonPressed(button, _) => {
                    if let Some(&mapped) = self.bindings.get(&button) {
                        self.held.entry(event.id).or_default().insert(mapped);
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    if let Some(&mapped) = self.bindings.get(&button) {
                        if let Some(set) = self.held.get_mut(&event.id) {
                            set.remove(&mapped);
                        }
                    }
                }
                // A pad vanishing mid-press would otherwise leave its keys
                // stuck; drop only that pad's state, not every pad's.
                EventType::Disconnected => {
                    self.held.remove(&event.id);
                }
                _ => {}
            }
        }

        let mut buttons: HashSet<Button> = self.held.values().flatten().copied().collect();
        for (_id, pad) in self.gilrs.gamepads() {
            // Left stick drives the X-pad.
            stick_directions(
                &mut buttons,
                pad.value(Axis::LeftStickX),
                pad.value(Axis::LeftStickY),
                Button::X1,
                Button::X2,
                Button::X3,
                Button::X4,
            );
            // Some pads/backends report the D-pad as a hat axis rather than as
            // discrete buttons; fold that onto the X-pad too. When the D-pad
            // arrives as buttons (handled above) these axes read 0, so there is
            // no double-counting.
            stick_directions(
                &mut buttons,
                pad.value(Axis::DPadX),
                pad.value(Axis::DPadY),
                Button::X1,
                Button::X2,
                Button::X3,
                Button::X4,
            );
            // Right stick drives the Y-pad.
            stick_directions(
                &mut buttons,
                pad.value(Axis::RightStickX),
                pad.value(Axis::RightStickY),
                Button::Y1,
                Button::Y2,
                Button::Y3,
                Button::Y4,
            );
        }
        keys_from(buttons)
    }
}

/// The set of gilrs buttons the settings UI can bind, with stable names.
///
/// Names round-trip through [`gilrs_button_from_name`] so they can be persisted
/// to the config file (which must not depend on gilrs). Any button outside this
/// set simply cannot be bound.
const NAMED_GILRS_BUTTONS: [(gilrs::Button, &str); 17] = [
    (gilrs::Button::South, "South"),
    (gilrs::Button::East, "East"),
    (gilrs::Button::North, "North"),
    (gilrs::Button::West, "West"),
    (gilrs::Button::LeftTrigger, "LeftTrigger"),
    (gilrs::Button::LeftTrigger2, "LeftTrigger2"),
    (gilrs::Button::RightTrigger, "RightTrigger"),
    (gilrs::Button::RightTrigger2, "RightTrigger2"),
    (gilrs::Button::Select, "Select"),
    (gilrs::Button::Start, "Start"),
    (gilrs::Button::Mode, "Mode"),
    (gilrs::Button::LeftThumb, "LeftThumb"),
    (gilrs::Button::RightThumb, "RightThumb"),
    (gilrs::Button::DPadUp, "DPadUp"),
    (gilrs::Button::DPadDown, "DPadDown"),
    (gilrs::Button::DPadLeft, "DPadLeft"),
    (gilrs::Button::DPadRight, "DPadRight"),
];

/// The stable name for a gilrs button, or `None` if it is not bindable.
pub fn gilrs_button_name(button: gilrs::Button) -> Option<&'static str> {
    NAMED_GILRS_BUTTONS
        .into_iter()
        .find(|(b, _)| *b == button)
        .map(|(_, name)| name)
}

/// Parse a gilrs button from its [`gilrs_button_name`], or `None` if unknown.
pub fn gilrs_button_from_name(name: &str) -> Option<gilrs::Button> {
    NAMED_GILRS_BUTTONS
        .into_iter()
        .find(|(_, n)| *n == name)
        .map(|(b, _)| b)
}

/// Fold one analog stick's `(x, y)` into directional [`Button`] presses.
///
/// gilrs axes are normalised to `[-1, 1]` with `+x` right and `+y` up. Any axis
/// pushed past [`AXIS_THRESHOLD`] inserts its direction; both axes can fire at
/// once for a diagonal.
fn stick_directions(
    out: &mut HashSet<Button>,
    x: f32,
    y: f32,
    up: Button,
    right: Button,
    down: Button,
    left: Button,
) {
    if y > AXIS_THRESHOLD {
        out.insert(up);
    }
    if y < -AXIS_THRESHOLD {
        out.insert(down);
    }
    if x > AXIS_THRESHOLD {
        out.insert(right);
    }
    if x < -AXIS_THRESHOLD {
        out.insert(left);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpad_maps_to_x_pad() {
        let b = default_gamepad_bindings();
        assert_eq!(b.get(&gilrs::Button::DPadUp), Some(&Button::X1));
        assert_eq!(b.get(&gilrs::Button::DPadRight), Some(&Button::X2));
        assert_eq!(b.get(&gilrs::Button::DPadDown), Some(&Button::X3));
        assert_eq!(b.get(&gilrs::Button::DPadLeft), Some(&Button::X4));
    }

    #[test]
    fn face_buttons_map_to_a_b_and_start() {
        let b = default_gamepad_bindings();
        assert_eq!(b.get(&gilrs::Button::South), Some(&Button::B));
        assert_eq!(b.get(&gilrs::Button::East), Some(&Button::A));
        assert_eq!(b.get(&gilrs::Button::Start), Some(&Button::Start));
    }

    #[test]
    fn unbound_button_has_no_default_mapping() {
        let b = default_gamepad_bindings();
        assert_eq!(b.get(&gilrs::Button::Mode), None);
        assert_eq!(b.get(&gilrs::Button::LeftTrigger), None);
    }

    #[test]
    fn gilrs_button_name_round_trips() {
        for (button, name) in NAMED_GILRS_BUTTONS {
            assert_eq!(gilrs_button_name(button), Some(name));
            assert_eq!(gilrs_button_from_name(name), Some(button));
        }
    }

    #[test]
    fn gilrs_button_from_name_rejects_unknown() {
        assert_eq!(gilrs_button_from_name("Nope"), None);
    }

    #[test]
    fn centred_stick_presses_nothing() {
        let mut out = HashSet::new();
        stick_directions(
            &mut out,
            0.0,
            0.0,
            Button::X1,
            Button::X2,
            Button::X3,
            Button::X4,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn stick_past_threshold_presses_direction() {
        let mut out = HashSet::new();
        stick_directions(
            &mut out,
            0.9,
            0.0,
            Button::X1,
            Button::X2,
            Button::X3,
            Button::X4,
        );
        assert_eq!(out, HashSet::from([Button::X2]));
    }

    #[test]
    fn stick_diagonal_presses_two_directions() {
        let mut out = HashSet::new();
        stick_directions(
            &mut out,
            -0.8,
            0.8,
            Button::X1,
            Button::X2,
            Button::X3,
            Button::X4,
        );
        assert_eq!(out, HashSet::from([Button::X1, Button::X4]));
    }

    #[test]
    fn stick_just_inside_dead_zone_presses_nothing() {
        let mut out = HashSet::new();
        stick_directions(
            &mut out,
            AXIS_THRESHOLD,
            -AXIS_THRESHOLD,
            Button::X1,
            Button::X2,
            Button::X3,
            Button::X4,
        );
        assert!(out.is_empty());
    }
}
