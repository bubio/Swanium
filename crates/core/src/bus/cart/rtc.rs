//! Cartridge real-time-clock (RTC) device.
//!
//! A handful of WonderSwan (and many WonderSwan Color) cartridges carry a
//! battery-backed RTC. It is an *optional* cartridge feature, modelled as
//! [`Option<Rtc>`] on [`super::Cartridge`]: cartridges without one hold `None`.
//!
//! # Determinism (no wall clock in `core`)
//!
//! The emulator core must stay deterministic and FFI-friendly for the planned
//! RetroAchievements integration (`docs/dev/DevelopmentPlan.md` §7), so the RTC
//! never reads `std::time`. Instead:
//!
//! * the initial date/time is **injected** once by the frontend via
//!   [`Rtc::set_datetime`] (typically from the host clock at ROM load), and
//! * the clock free-runs off the **emulated master clock**: [`Rtc::tick`] is fed
//!   the per-frame cycle budget and rolls the seconds register forward (with full
//!   BCD carry into minute / hour / day / month / year and the weekday counter).
//!
//! If the frontend never injects a time the clock starts at a fixed epoch
//! (2000-01-01 00:00:00, a Saturday), keeping headless runs reproducible.
//!
//! # Command protocol (ports 0xCA / 0xCB)
//!
//! The CPU talks to the RTC through two I/O ports:
//!
//! * **0xCA — command/status**: a write selects the active operation; a read
//!   returns the command protocol status bits: ready (bit 7) and busy (bit 4).
//! * **0xCB — data**: sequential access to the payload of the active command.
//!   Each read or write advances an internal byte pointer. The ready/busy bits
//!   track how many bytes remain in the active command's payload.
//!
//! The command byte values and payload ordering below follow the public WSdev /
//! emulator documentation but are **unverified against hardware or a test ROM**
//! (no WonderSwan Color RTC test ROM is available); see the 実装メモ（8e） block
//! in `docs/dev/DevelopmentPlan.md`.

/// The number of bytes of battery-backed state the RTC exposes for save data.
///
/// Covers the persistable registers: 7 date/time bytes, the status byte, and the
/// 2 alarm bytes. Transient command-protocol state (the active command and data
/// pointer) and the sub-second cycle accumulator are *not* included.
///
/// [`Rtc::state`] / [`Rtc::load_state`] serialise exactly these bytes. Folding
/// them into the cartridge's on-disk save stream (`Cartridge::save_data`) needs a
/// versioned composite-save format and is a deferred follow-up — see the
/// 「セーブデータ形式」 note and 実装メモ（8e） in `docs/dev/DevelopmentPlan.md`.
pub const RTC_STATE_LEN: usize = 10;

/// Master-clock cycles in one wall-second (also the emulated RTC's tick rate).
const CYCLES_PER_SECOND: u32 = crate::system::MASTER_CLOCK_HZ;

/// "Command ready / active" bit returned when reading the command port (0xCA).
const CMD_READY: u8 = 0x80;
/// "Command busy / more bytes pending" bit returned on the command port (0xCA).
const CMD_BUSY: u8 = 0x10;
/// Mask selecting the operation from a command byte (low 5 bits).
const CMD_MASK: u8 = 0x1F;

// Command codes (low 5 bits of the 0xCA byte).
const CMD_RESET: u8 = 0x10;
const CMD_STATUS_WRITE: u8 = 0x12;
const CMD_STATUS_READ: u8 = 0x13;
const CMD_DATETIME_WRITE: u8 = 0x14;
const CMD_DATETIME_READ: u8 = 0x15;
const CMD_UNKNOWN_3_WRITE: u8 = 0x16;
const CMD_UNKNOWN_3_READ: u8 = 0x17;
const CMD_ALARM_A_WRITE: u8 = 0x18;
const CMD_ALARM_A_READ: u8 = 0x19;
const CMD_ALARM_B_WRITE: u8 = 0x1A;
const CMD_ALARM_B_READ: u8 = 0x1B;

/// Value returned when reading the data port with no readable command active.
const OPEN_BUS: u8 = 0x90;

/// Cartridge real-time clock.
///
/// Holds the battery-backed BCD date/time registers plus the alarm and status
/// registers, and drives them forward off the emulated master clock. All fields
/// are plain data so the device is trivially serialisable for save states.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Rtc {
    // Battery-backed BCD registers.
    year: u8,    // 00–99 ⇒ calendar year 2000–2099
    month: u8,   // 01–12
    day: u8,     // 01–31
    weekday: u8, // 0–6 (0 = Sunday)
    hour: u8,    // 00–23
    minute: u8,  // 00–59
    second: u8,  // 00–59
    status: u8,  // device status / alarm-enable flags
    alarm_hour: u8,
    alarm_minute: u8,

    // Transient command-protocol state (not persisted).
    command: u8,
    index: u8,
    remaining: u8,
    unsupported_busy: bool,

    // Sub-second accumulator, in master-clock cycles (not persisted).
    cycle_accum: u32,
}

impl Default for Rtc {
    fn default() -> Self {
        // Fixed epoch: 2000-01-01 00:00:00, a Saturday (weekday 6).
        Self {
            year: 0x00,
            month: 0x01,
            day: 0x01,
            weekday: 6,
            hour: 0x00,
            minute: 0x00,
            second: 0x00,
            status: 0x00,
            alarm_hour: 0x00,
            alarm_minute: 0x00,
            command: 0x00,
            index: 0,
            remaining: 0,
            unsupported_busy: false,
            cycle_accum: 0,
        }
    }
}

impl Rtc {
    /// Create an RTC at the default epoch (2000-01-01 00:00:00).
    pub fn new() -> Self {
        Self::default()
    }

    // ── Time injection & free-run ─────────────────────────────────────────

    /// Inject an absolute date/time from decimal (non-BCD) components.
    ///
    /// The frontend calls this once from the host clock at ROM load; the core
    /// itself never reads wall-clock time. Components are decimal: `year` is the
    /// two-digit calendar year within 2000–2099 (e.g. `26` for 2026), `month`
    /// 1–12, `day` 1–31, `weekday` 0–6 (0 = Sunday), `hour` 0–23, `minute` and
    /// `second` 0–59. Out-of-range values are clamped into their register width
    /// by the BCD conversion and wrap naturally on the next tick.
    #[allow(clippy::too_many_arguments)]
    pub fn set_datetime(
        &mut self,
        year: u8,
        month: u8,
        day: u8,
        weekday: u8,
        hour: u8,
        minute: u8,
        second: u8,
    ) {
        self.year = to_bcd(year % 100);
        self.month = to_bcd(month.clamp(1, 12));
        self.day = to_bcd(day.clamp(1, 31));
        self.weekday = weekday % 7;
        self.hour = to_bcd(hour.min(23));
        self.minute = to_bcd(minute.min(59));
        self.second = to_bcd(second.min(59));
        self.cycle_accum = 0;
    }

    /// Advance the clock by `cycles` master-clock ticks (the emulated time base).
    ///
    /// Called once per frame with the frame's cycle budget. Whole elapsed seconds
    /// roll the second register forward with full BCD carry; the sub-second
    /// remainder is retained so the average rate matches the master clock exactly.
    pub fn tick(&mut self, cycles: u32) {
        self.cycle_accum += cycles;
        while self.cycle_accum >= CYCLES_PER_SECOND {
            self.cycle_accum -= CYCLES_PER_SECOND;
            self.advance_one_second();
        }
    }

    fn advance_one_second(&mut self) {
        let mut s = from_bcd(self.second) + 1;
        if s < 60 {
            self.second = to_bcd(s);
            return;
        }
        s = 0;
        self.second = to_bcd(s);

        let mut mi = from_bcd(self.minute) + 1;
        if mi < 60 {
            self.minute = to_bcd(mi);
            return;
        }
        mi = 0;
        self.minute = to_bcd(mi);

        let mut h = from_bcd(self.hour) + 1;
        if h < 24 {
            self.hour = to_bcd(h);
            return;
        }
        h = 0;
        self.hour = to_bcd(h);

        // Day rollover: advance weekday and the calendar day/month/year.
        self.weekday = (self.weekday + 1) % 7;

        let year = from_bcd(self.year);
        let month = from_bcd(self.month);
        let mut d = from_bcd(self.day) + 1;
        if d <= days_in_month(month, year) {
            self.day = to_bcd(d);
            return;
        }
        d = 1;
        self.day = to_bcd(d);

        let mut mo = month + 1;
        if mo <= 12 {
            self.month = to_bcd(mo);
            return;
        }
        mo = 1;
        self.month = to_bcd(mo);
        self.year = to_bcd((year + 1) % 100);
    }

    // ── Port 0xCA (command / status) ──────────────────────────────────────

    /// Handle a write to the command port (0xCA).
    pub fn write_command(&mut self, value: u8) {
        self.command = value;
        self.index = 0;

        if value & CMD_READY != 0 {
            self.remaining = 0;
            self.unsupported_busy = false;
            return;
        }

        let command = value & CMD_MASK;
        self.remaining = command_transfer_len(command);
        self.unsupported_busy = self.remaining == 0;

        if command == CMD_RESET {
            self.reset_registers();
        }
    }

    /// Read the command port (0xCA): ready/busy status for the active command.
    pub fn read_command(&self) -> u8 {
        let ready = u8::from(self.remaining > 0) * CMD_READY;
        let busy = u8::from(self.remaining > 1 || self.unsupported_busy) * CMD_BUSY;
        ready | busy
    }

    /// Reset the battery-backed registers to the epoch, leaving the transient
    /// command-protocol state (active command, data pointer, sub-second
    /// accumulator) untouched so the in-flight RESET command still reads back.
    fn reset_registers(&mut self) {
        self.apply_state(&Self::default().state_bytes());
    }

    // ── Port 0xCB (data) ──────────────────────────────────────────────────

    /// Handle a write to the data port (0xCB) for the active command.
    pub fn write_data(&mut self, value: u8) {
        match self.command & CMD_MASK {
            CMD_DATETIME_WRITE | CMD_DATETIME_READ => {
                self.set_datetime_byte(self.index, value);
                self.index = (self.index + 1) % 7;
                self.consume_transfer_byte();
            }
            CMD_ALARM_A_WRITE | CMD_ALARM_B_WRITE => {
                if self.index == 0 {
                    self.alarm_hour = value;
                } else {
                    self.alarm_minute = value;
                }
                self.index = (self.index + 1) % 2;
                self.consume_transfer_byte();
            }
            CMD_STATUS_WRITE | CMD_STATUS_READ => {
                self.status = value;
                self.consume_transfer_byte();
            }
            CMD_UNKNOWN_3_WRITE | CMD_UNKNOWN_3_READ => {
                self.consume_transfer_byte();
            }
            _ => {}
        }
    }

    /// Read the data port (0xCB) for the active command, advancing the pointer.
    pub fn read_data(&mut self) -> u8 {
        match self.command & CMD_MASK {
            CMD_DATETIME_WRITE | CMD_DATETIME_READ => {
                let v = self.datetime_byte(self.index);
                self.index = (self.index + 1) % 7;
                self.consume_transfer_byte();
                v
            }
            CMD_ALARM_A_READ | CMD_ALARM_B_READ => {
                let v = if self.index == 0 {
                    self.alarm_hour
                } else {
                    self.alarm_minute
                };
                self.index = (self.index + 1) % 2;
                self.consume_transfer_byte();
                v
            }
            CMD_STATUS_WRITE | CMD_STATUS_READ => {
                self.consume_transfer_byte();
                self.status
            }
            CMD_UNKNOWN_3_WRITE | CMD_UNKNOWN_3_READ => {
                self.consume_transfer_byte();
                0
            }
            _ => OPEN_BUS,
        }
    }

    fn consume_transfer_byte(&mut self) {
        self.remaining = self.remaining.saturating_sub(1);
    }

    fn datetime_byte(&self, index: u8) -> u8 {
        match index {
            0 => self.year,
            1 => self.month,
            2 => self.day,
            3 => self.weekday,
            4 => self.hour,
            5 => self.minute,
            _ => self.second,
        }
    }

    fn set_datetime_byte(&mut self, index: u8, value: u8) {
        match index {
            0 => self.year = value,
            1 => self.month = value,
            2 => self.day = value,
            3 => self.weekday = value & 0x07,
            4 => self.hour = value,
            5 => self.minute = value,
            _ => self.second = value,
        }
    }

    // ── Save-state serialisation ──────────────────────────────────────────

    /// Restore RTC registers from previously serialised save data.
    ///
    /// Bytes beyond [`RTC_STATE_LEN`] are ignored; a shorter slice leaves the
    /// remaining registers at their current value.
    pub fn load_state(&mut self, data: &[u8]) {
        let n = data.len().min(RTC_STATE_LEN);
        let mut regs = self.state_bytes();
        regs[..n].copy_from_slice(&data[..n]);
        self.apply_state(&regs);
    }

    /// The RTC's battery-backed register state, for save-data serialisation.
    pub fn state(&self) -> [u8; RTC_STATE_LEN] {
        self.state_bytes()
    }

    fn state_bytes(&self) -> [u8; RTC_STATE_LEN] {
        [
            self.year,
            self.month,
            self.day,
            self.weekday,
            self.hour,
            self.minute,
            self.second,
            self.status,
            self.alarm_hour,
            self.alarm_minute,
        ]
    }

    fn apply_state(&mut self, regs: &[u8; RTC_STATE_LEN]) {
        self.year = regs[0];
        self.month = regs[1];
        self.day = regs[2];
        self.weekday = regs[3] & 0x07;
        self.hour = regs[4];
        self.minute = regs[5];
        self.second = regs[6];
        self.status = regs[7];
        self.alarm_hour = regs[8];
        self.alarm_minute = regs[9];
    }
}

/// Convert a 0–99 binary value to two-digit packed BCD.
fn to_bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

/// Convert a two-digit packed-BCD byte back to binary (0–99).
fn from_bcd(value: u8) -> u8 {
    (value >> 4) * 10 + (value & 0x0F)
}

/// Number of days in `month` (1–12) of calendar year `2000 + year` (0–99).
///
/// Every year divisible by 4 in 2000–2099 is a leap year (2100 is out of range),
/// so the leap test reduces to `year.is_multiple_of(4)`.
fn days_in_month(month: u8, year: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) => 29,
        2 => 28,
        _ => 30, // out-of-range guard; should not occur with valid registers
    }
}

fn command_transfer_len(command: u8) -> u8 {
    match command {
        0x10..=0x13 => 1,
        0x14 | 0x15 => 7,
        0x16 | 0x17 => 3,
        0x18..=0x1B => 2,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcd_round_trips() {
        for v in 0..=99u8 {
            assert_eq!(from_bcd(to_bcd(v)), v);
        }
    }

    #[test]
    fn default_epoch_is_2000_01_01_saturday() {
        let rtc = Rtc::new();
        assert_eq!(rtc.datetime_byte(0), 0x00); // year 2000
        assert_eq!(rtc.datetime_byte(1), 0x01); // month
        assert_eq!(rtc.datetime_byte(2), 0x01); // day
        assert_eq!(rtc.datetime_byte(3), 6); // Saturday
    }

    #[test]
    fn tick_below_one_second_does_not_advance() {
        let mut rtc = Rtc::new();
        rtc.tick(CYCLES_PER_SECOND - 1);
        assert_eq!(rtc.second, 0x00);
    }

    #[test]
    fn tick_one_second_increments_seconds() {
        let mut rtc = Rtc::new();
        rtc.tick(CYCLES_PER_SECOND);
        assert_eq!(rtc.second, 0x01);
    }

    #[test]
    fn sub_second_remainder_accumulates_across_ticks() {
        let mut rtc = Rtc::new();
        let two_thirds = CYCLES_PER_SECOND * 2 / 3;
        rtc.tick(two_thirds); // 0.66s
        assert_eq!(rtc.second, 0x00);
        rtc.tick(two_thirds); // 1.33s total → one whole second
        assert_eq!(rtc.second, 0x01);
    }

    #[test]
    fn seconds_carry_into_minutes() {
        let mut rtc = Rtc::new();
        rtc.set_datetime(26, 7, 3, 5, 12, 30, 59);
        rtc.tick(CYCLES_PER_SECOND);
        assert_eq!(rtc.second, 0x00);
        assert_eq!(rtc.minute, 0x31);
    }

    #[test]
    fn full_carry_rolls_midnight_and_weekday() {
        let mut rtc = Rtc::new();
        // 2026-07-03 (Friday, weekday 5) 23:59:59 → +1s → 2026-07-04 00:00:00 Sat.
        rtc.set_datetime(26, 7, 3, 5, 23, 59, 59);
        rtc.tick(CYCLES_PER_SECOND);
        assert_eq!(rtc.hour, 0x00);
        assert_eq!(rtc.minute, 0x00);
        assert_eq!(rtc.second, 0x00);
        assert_eq!(rtc.day, 0x04);
        assert_eq!(rtc.weekday, 6);
    }

    #[test]
    fn month_end_rolls_to_next_month() {
        let mut rtc = Rtc::new();
        // 2026-01-31 23:59:59 → 2026-02-01 00:00:00.
        rtc.set_datetime(26, 1, 31, 6, 23, 59, 59);
        rtc.tick(CYCLES_PER_SECOND);
        assert_eq!(rtc.month, 0x02);
        assert_eq!(rtc.day, 0x01);
    }

    #[test]
    fn february_leap_year_has_29_days() {
        let mut rtc = Rtc::new();
        // 2024 is a leap year: 2024-02-28 → 2024-02-29.
        rtc.set_datetime(24, 2, 28, 4, 23, 59, 59);
        rtc.tick(CYCLES_PER_SECOND);
        assert_eq!(rtc.month, 0x02);
        assert_eq!(rtc.day, 0x29);
    }

    #[test]
    fn february_non_leap_year_has_28_days() {
        let mut rtc = Rtc::new();
        // 2026 is not a leap year: 2026-02-28 → 2026-03-01.
        rtc.set_datetime(26, 2, 28, 6, 23, 59, 59);
        rtc.tick(CYCLES_PER_SECOND);
        assert_eq!(rtc.month, 0x03);
        assert_eq!(rtc.day, 0x01);
    }

    #[test]
    fn year_end_rolls_over() {
        let mut rtc = Rtc::new();
        // 2026-12-31 23:59:59 → 2027-01-01.
        rtc.set_datetime(26, 12, 31, 4, 23, 59, 59);
        rtc.tick(CYCLES_PER_SECOND);
        assert_eq!(rtc.year, 0x27);
        assert_eq!(rtc.month, 0x01);
        assert_eq!(rtc.day, 0x01);
    }

    #[test]
    fn datetime_read_protocol_returns_seven_bytes_in_order() {
        let mut rtc = Rtc::new();
        rtc.set_datetime(26, 7, 3, 5, 12, 34, 56);
        rtc.write_command(CMD_DATETIME_READ);
        let bytes: Vec<u8> = (0..7).map(|_| rtc.read_data()).collect();
        assert_eq!(bytes, vec![0x26, 0x07, 0x03, 5, 0x12, 0x34, 0x56]);
        // Pointer wraps back to the year for the eighth read.
        assert_eq!(rtc.read_data(), 0x26);
    }

    #[test]
    fn datetime_write_protocol_sets_all_registers() {
        let mut rtc = Rtc::new();
        rtc.write_command(CMD_DATETIME_WRITE);
        for b in [0x26, 0x07, 0x03, 0x05, 0x12, 0x34, 0x56] {
            rtc.write_data(b);
        }
        rtc.write_command(CMD_DATETIME_READ);
        let bytes: Vec<u8> = (0..7).map(|_| rtc.read_data()).collect();
        assert_eq!(bytes, vec![0x26, 0x07, 0x03, 0x05, 0x12, 0x34, 0x56]);
    }

    #[test]
    fn command_read_sets_ready_bit() {
        let mut rtc = Rtc::new();
        rtc.write_command(CMD_DATETIME_READ);
        assert_eq!(rtc.read_command(), CMD_READY | CMD_BUSY);
    }

    #[test]
    fn reset_command_clears_registers_to_epoch() {
        let mut rtc = Rtc::new();
        rtc.set_datetime(26, 7, 3, 5, 12, 34, 56);
        rtc.write_command(CMD_RESET);
        assert_eq!(rtc.datetime_byte(0), 0x00);
        assert_eq!(rtc.datetime_byte(1), 0x01);
        assert_eq!(rtc.datetime_byte(2), 0x01);
        // The RESET command itself remains ready for its one-byte payload.
        assert_eq!(rtc.read_command(), CMD_READY);
    }

    #[test]
    fn status_bits_track_command_payload_length() {
        let mut rtc = Rtc::new();
        rtc.write_command(CMD_DATETIME_READ);
        for _ in 0..6 {
            assert_eq!(rtc.read_command(), CMD_READY | CMD_BUSY);
            rtc.read_data();
        }
        assert_eq!(rtc.read_command(), CMD_READY);
        rtc.read_data();
        assert_eq!(rtc.read_command(), 0x00);
    }

    #[test]
    fn unsupported_command_stays_busy_without_ready() {
        let mut rtc = Rtc::new();
        rtc.write_command(0x1C);
        assert_eq!(rtc.read_command(), CMD_BUSY);
        rtc.read_data();
        assert_eq!(rtc.read_command(), CMD_BUSY);
    }

    #[test]
    fn writing_ready_bit_does_not_force_ready_status() {
        let mut rtc = Rtc::new();
        rtc.write_command(CMD_RESET);
        assert_eq!(rtc.read_command(), CMD_READY);
        rtc.write_command(CMD_READY | CMD_STATUS_READ);
        assert_eq!(rtc.read_command(), 0x00);
    }

    #[test]
    fn alarm_protocol_round_trips() {
        let mut rtc = Rtc::new();
        rtc.write_command(CMD_ALARM_A_WRITE);
        rtc.write_data(0x07);
        rtc.write_data(0x30);
        rtc.write_command(CMD_ALARM_A_READ);
        assert_eq!(rtc.read_data(), 0x07);
        assert_eq!(rtc.read_data(), 0x30);
    }

    #[test]
    fn status_protocol_round_trips() {
        let mut rtc = Rtc::new();
        rtc.write_command(CMD_STATUS_WRITE);
        rtc.write_data(0xA5);
        rtc.write_command(CMD_STATUS_READ);
        assert_eq!(rtc.read_data(), 0xA5);
    }

    #[test]
    fn data_read_without_readable_command_is_open_bus() {
        let mut rtc = Rtc::new();
        rtc.write_command(0x1C);
        assert_eq!(rtc.read_data(), OPEN_BUS);
    }

    #[test]
    fn state_round_trips_through_load() {
        let mut rtc = Rtc::new();
        rtc.set_datetime(26, 7, 3, 5, 12, 34, 56);
        rtc.alarm_hour = 0x07;
        rtc.alarm_minute = 0x30;
        rtc.status = 0x81;
        let saved = rtc.state();

        let mut restored = Rtc::new();
        restored.load_state(&saved);
        assert_eq!(restored.state(), saved);
    }
}
