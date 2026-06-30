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
        })
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
                    if let Some(mapped) = map_button(button) {
                        self.held.entry(event.id).or_default().insert(mapped);
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    if let Some(mapped) = map_button(button) {
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

/// Map a gilrs digital button to a WonderSwan [`Button`], or `None` if unbound.
///
/// The D-pad drives the X-pad; the bottom/right face buttons are B/A (the
/// SNES-style layout WonderSwan players expect); the menu button is Start.
fn map_button(button: gilrs::Button) -> Option<Button> {
    use gilrs::Button as G;
    Some(match button {
        G::DPadUp => Button::X1,
        G::DPadRight => Button::X2,
        G::DPadDown => Button::X3,
        G::DPadLeft => Button::X4,
        G::South => Button::B,
        G::East => Button::A,
        G::Start => Button::Start,
        _ => return None,
    })
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
        assert_eq!(map_button(gilrs::Button::DPadUp), Some(Button::X1));
        assert_eq!(map_button(gilrs::Button::DPadRight), Some(Button::X2));
        assert_eq!(map_button(gilrs::Button::DPadDown), Some(Button::X3));
        assert_eq!(map_button(gilrs::Button::DPadLeft), Some(Button::X4));
    }

    #[test]
    fn face_buttons_map_to_a_b_and_start() {
        assert_eq!(map_button(gilrs::Button::South), Some(Button::B));
        assert_eq!(map_button(gilrs::Button::East), Some(Button::A));
        assert_eq!(map_button(gilrs::Button::Start), Some(Button::Start));
    }

    #[test]
    fn unbound_button_maps_to_none() {
        assert_eq!(map_button(gilrs::Button::Mode), None);
        assert_eq!(map_button(gilrs::Button::LeftTrigger), None);
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
