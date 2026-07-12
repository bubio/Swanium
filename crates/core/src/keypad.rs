//! WonderSwan key matrix (I/O port 0xB5).
//!
//! The console has eleven physical keys arranged in three scan groups: two
//! four-way pads (`X1`–`X4` and `Y1`–`Y4`) and the action buttons (`Start`,
//! `A`, `B`). A game selects one or more groups by writing the high nibble of
//! port 0xB5, then reads the low nibble to obtain the four keys of the selected
//! group(s) OR-combined.
//!
//! [`KeyState`] is the platform-independent representation the frontend hands to
//! the core each frame via [`Bus::set_keys`](crate::bus::Bus::set_keys); the
//! mapping from a host keyboard/gamepad lives in the `input` crate, not here.
//!
//! Bit layout (matches the hardware scan order so the bus read is a simple
//! shift-and-mask):
//!
//! ```text
//! bit:  0   1   2   3    4   5   6   7    8      9      10  11
//!       Y1  Y2  Y3  Y4   X1  X2  X3  X4   --     Start  A   B
//!       └─ group 0x10 ┘  └─ group 0x20 ┘  └──── group 0x40 ────┘
//! ```

/// The set of WonderSwan keys currently held down.
///
/// A lightweight bit set over the eleven hardware keys. Construct it from the
/// associated key constants combined with `|`, e.g.
/// `KeyState::X1 | KeyState::A`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct KeyState(u16);

impl KeyState {
    /// No keys held.
    pub const NONE: Self = Self(0);

    /// Y-pad key 1 (top in vertical orientation).
    pub const Y1: Self = Self(1 << 0);
    /// Y-pad key 2 (right in vertical orientation).
    pub const Y2: Self = Self(1 << 1);
    /// Y-pad key 3 (bottom in vertical orientation).
    pub const Y3: Self = Self(1 << 2);
    /// Y-pad key 4 (left in vertical orientation).
    pub const Y4: Self = Self(1 << 3);

    /// X-pad key 1 (up in horizontal orientation).
    pub const X1: Self = Self(1 << 4);
    /// X-pad key 2 (right in horizontal orientation).
    pub const X2: Self = Self(1 << 5);
    /// X-pad key 3 (down in horizontal orientation).
    pub const X3: Self = Self(1 << 6);
    /// X-pad key 4 (left in horizontal orientation).
    pub const X4: Self = Self(1 << 7);

    /// Start button.
    pub const START: Self = Self(1 << 9);
    /// A button.
    pub const A: Self = Self(1 << 10);
    /// B button.
    pub const B: Self = Self(1 << 11);

    /// Scan-group selector bit (port 0xB5 bit 4): the Y pad.
    pub(crate) const SCAN_Y: u8 = 0x10;
    /// Scan-group selector bit (port 0xB5 bit 5): the X pad.
    pub(crate) const SCAN_X: u8 = 0x20;
    /// Scan-group selector bit (port 0xB5 bit 6): the action buttons.
    pub(crate) const SCAN_BUTTONS: u8 = 0x40;

    /// Build a key set from the raw 16-bit hardware bit pattern.
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits)
    }

    /// The raw 16-bit hardware bit pattern.
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Whether no keys are held.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether every key in `other` is held in `self`.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The 0xB5 read result for the given scan-group selector (high nibble).
    ///
    /// Returns the selector bits OR-combined with the low nibble of each
    /// selected group, exactly as the hardware presents them on the port.
    pub(crate) fn scan(self, select: u8) -> u8 {
        let mut result = select;
        if select & Self::SCAN_Y != 0 {
            result |= (self.0 & 0x0F) as u8;
        }
        if select & Self::SCAN_X != 0 {
            result |= ((self.0 >> 4) & 0x0F) as u8;
        }
        if select & Self::SCAN_BUTTONS != 0 {
            result |= ((self.0 >> 8) & 0x0F) as u8;
        }
        result
    }
}

impl std::ops::BitOr for KeyState {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for KeyState {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_is_empty() {
        assert!(KeyState::NONE.is_empty());
    }

    #[test]
    fn combined_keys_are_not_empty() {
        assert!(!(KeyState::X1 | KeyState::A).is_empty());
    }

    #[test]
    fn contains_reports_held_key() {
        assert!((KeyState::X1 | KeyState::A).contains(KeyState::A));
    }

    #[test]
    fn contains_rejects_absent_key() {
        assert!(!(KeyState::X1 | KeyState::A).contains(KeyState::B));
    }

    #[test]
    fn bits_round_trip_through_from_bits() {
        let keys = KeyState::X2 | KeyState::START;
        assert_eq!(KeyState::from_bits(keys.bits()), keys);
    }

    #[test]
    fn scan_y_group_returns_y_keys_in_low_nibble() {
        let keys = KeyState::Y1 | KeyState::Y3;
        assert_eq!(keys.scan(KeyState::SCAN_Y), KeyState::SCAN_Y | 0b0101);
    }

    #[test]
    fn scan_x_group_returns_x_keys_in_low_nibble() {
        let keys = KeyState::X1 | KeyState::X4;
        assert_eq!(keys.scan(KeyState::SCAN_X), KeyState::SCAN_X | 0b1001);
    }

    #[test]
    fn scan_buttons_group_maps_start_to_bit1() {
        assert_eq!(
            KeyState::START.scan(KeyState::SCAN_BUTTONS),
            KeyState::SCAN_BUTTONS | 0b0010
        );
    }

    #[test]
    fn scan_buttons_group_maps_a_to_bit2() {
        assert_eq!(
            KeyState::A.scan(KeyState::SCAN_BUTTONS),
            KeyState::SCAN_BUTTONS | 0b0100
        );
    }

    #[test]
    fn scan_buttons_group_maps_b_to_bit3() {
        assert_eq!(
            KeyState::B.scan(KeyState::SCAN_BUTTONS),
            KeyState::SCAN_BUTTONS | 0b1000
        );
    }

    #[test]
    fn unselected_group_contributes_nothing() {
        // X keys held, but only the Y group is scanned.
        assert_eq!(
            (KeyState::X1 | KeyState::X2).scan(KeyState::SCAN_Y),
            KeyState::SCAN_Y
        );
    }

    #[test]
    fn multiple_groups_or_combine() {
        let keys = KeyState::Y1 | KeyState::X2;
        // Y1 -> bit0, X2 -> bit1.
        let select = KeyState::SCAN_Y | KeyState::SCAN_X;
        assert_eq!(keys.scan(select), select | 0b0011);
    }
}
