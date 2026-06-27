//! Cartridge real-time-clock (RTC) interface.
//!
//! A handful of WonderSwan (and all WonderSwan Color) cartridges carry a
//! battery-backed RTC. The RTC is an *optional* cartridge feature, modelled as
//! [`Option<Rtc>`] on [`super::Cartridge`]: mono cartridges without one hold
//! `None`.
//!
//! Phase 6 defines this interface only. Timekeeping (BCD date/time registers,
//! alarm, the command protocol on ports 0xCA–0xCB) is implemented in Phase 8
//! alongside WonderSwan Color support — see `docs/dev/DevelopmentPlan.md`. The
//! command/status methods below are deliberate stubs so the port-dispatch and
//! save-data plumbing can be wired now and given behaviour later without an API
//! change.

/// The number of bytes of persistent state an RTC contributes to a save file.
///
/// Covers the RTC's battery-backed date/time registers. The exact layout is
/// defined in Phase 8; the size is fixed now so save-data framing is stable.
pub const RTC_STATE_LEN: usize = 8;

/// Cartridge real-time clock.
///
/// Holds the battery-backed register state. All timekeeping behaviour is
/// deferred to Phase 8; today the device only stores and returns its state so
/// save data round-trips losslessly.
#[derive(Clone, Debug, Default)]
pub struct Rtc {
    /// Battery-backed register state (BCD date/time; layout defined in Phase 8).
    state: [u8; RTC_STATE_LEN],
}

impl Rtc {
    /// Create an RTC with cleared register state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Restore RTC state from previously serialised save data.
    ///
    /// Bytes beyond [`RTC_STATE_LEN`] are ignored; a shorter slice leaves the
    /// remaining registers cleared.
    pub fn load_state(&mut self, data: &[u8]) {
        let n = data.len().min(RTC_STATE_LEN);
        self.state[..n].copy_from_slice(&data[..n]);
    }

    /// The RTC's battery-backed state, for save-data serialisation.
    pub fn state(&self) -> &[u8] {
        &self.state
    }
}
