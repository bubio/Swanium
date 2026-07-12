//! WonderSwan cartridge: ROM, optional save medium (SRAM **or** serial EEPROM),
//! optional RTC, and the bank-switch registers.
//!
//! Memory map (physical address ranges handled by [`super::Bus`]):
//! - 0x10000–0x1FFFF: SRAM bank (selected by the RAM bank register)
//! - 0x20000–0x2FFFF: ROM bank 0 (selected by the ROM-bank-0 register)
//! - 0x30000–0x3FFFF: ROM bank 1 (selected by the ROM-bank-1 register)
//! - 0x40000–0xFFFFF: ROM linear range (offset by `linear_off`)
//!
//! Bank addressing uses OR semantics: `(bank << 16) | (addr & 0xFFFF)`, taken
//! modulo the medium length. This lets ROMs/SRAM of any power-of-two size be
//! addressed correctly and matches WonderCrab's reference implementation.
//!
//! The effective bank value depends on the [`Mapper`]: the Bandai 2001 uses
//! 8-bit bank registers, while the Bandai 2003 widens them to 16 bits via the
//! high-byte ports 0xD1/0xD3/0xD5. At power-on all bank registers initialise to
//! 0xFF, so the ROM linear range maps to the last bytes of the cartridge ROM
//! (the reset vector / header area).

mod eeprom;
mod header;
mod rtc;

#[cfg(test)]
mod tests;

pub use eeprom::Eeprom;
pub use header::{CartridgeHeader, Mapper, SaveType};
pub use rtc::Rtc;

use super::OPEN_BUS;

/// EEPROM control-port operation nibbles (high nibble of port 0xC8 writes).
const EEPROM_OP_READ: u8 = 0b0001;
const EEPROM_OP_WRITE: u8 = 0b0010;
const EEPROM_OP_COMMAND: u8 = 0b0100;

/// WonderSwan cartridge: ROM, optional SRAM, and bank-switch registers.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Cartridge {
    /// Cartridge ROM image.
    pub rom: Vec<u8>,
    /// Battery-backed SRAM (empty if the save medium is EEPROM or absent).
    pub sram: Vec<u8>,
    /// Serial EEPROM save medium (`None` unless the header selects EEPROM).
    eeprom: Option<Eeprom>,
    /// Optional cartridge real-time clock.
    rtc: Option<Rtc>,
    /// Bank-switch chip; selects the bank-register width.
    mapper: Mapper,
    /// Parsed header, if the ROM carried a valid footer.
    header: Option<CartridgeHeader>,

    /// I/O port 0xC0: linear ROM address offset (bits 5:0; masked on write).
    pub linear_off: u8,
    /// I/O port 0xC1 / 0xD0: SRAM bank, low byte.
    pub ram_bank: u8,
    /// I/O port 0xD1: SRAM bank, high byte (Bandai 2003 only).
    pub ram_bank_hi: u8,
    /// I/O port 0xC2 / 0xD2: ROM bank 0, low byte.
    pub rom_bank0: u8,
    /// I/O port 0xD3: ROM bank 0, high byte (Bandai 2003 only).
    pub rom_bank0_hi: u8,
    /// I/O port 0xC3 / 0xD4: ROM bank 1, low byte.
    pub rom_bank1: u8,
    /// I/O port 0xD5: ROM bank 1, high byte (Bandai 2003 only).
    pub rom_bank1_hi: u8,
}

impl Cartridge {
    /// Create a cartridge from raw ROM bytes with an explicit SRAM buffer.
    ///
    /// The mapper is taken from the ROM footer (falling back to Bandai 2001);
    /// the save medium is the provided SRAM (EEPROM and RTC are absent). Used by
    /// the SRAM-oriented [`Bus`](super::Bus) constructors and by tests.
    pub fn new(rom: Vec<u8>, sram: Vec<u8>) -> Self {
        let header = CartridgeHeader::parse(&rom);
        let mapper = header.map_or(Mapper::Bandai2001, |h| h.mapper);
        Self {
            rom,
            sram,
            eeprom: None,
            rtc: None,
            mapper,
            header,
            linear_off: 0xFF,
            ram_bank: 0xFF,
            ram_bank_hi: 0xFF,
            rom_bank0: 0xFF,
            rom_bank0_hi: 0xFF,
            rom_bank1: 0xFF,
            rom_bank1_hi: 0xFF,
        }
    }

    /// Create a cartridge from a ROM image, allocating the save medium and
    /// configuring the mapper according to the parsed header.
    ///
    /// An unparseable footer yields a Bandai 2001 cartridge with no save medium.
    pub fn from_rom(rom: Vec<u8>) -> Self {
        let header = CartridgeHeader::parse(&rom);
        let mapper = header.map_or(Mapper::Bandai2001, |h| h.mapper);
        let save_type = header.map_or(SaveType::None, |h| h.save_type);

        let (sram, eeprom) = match save_type {
            SaveType::Sram(n) => (vec![0u8; n], None),
            SaveType::Eeprom(n) => {
                let eeprom = Eeprom::address_bits_for(n)
                    .map(|bits| Eeprom::new_locked(vec![0xFFu8; n], bits));
                (Vec::new(), eeprom)
            }
            SaveType::None => (Vec::new(), None),
        };

        let rtc = header.is_some_and(|h| h.rtc).then(Rtc::new);

        Self {
            rom,
            sram,
            eeprom,
            rtc,
            mapper,
            header,
            linear_off: 0xFF,
            ram_bank: 0xFF,
            ram_bank_hi: 0xFF,
            rom_bank0: 0xFF,
            rom_bank0_hi: 0xFF,
            rom_bank1: 0xFF,
            rom_bank1_hi: 0xFF,
        }
    }

    /// Empty 64 KiB ROM / 32 KiB SRAM cartridge for unit tests.
    pub fn for_test() -> Self {
        Self::new(vec![0u8; 0x10000], vec![0u8; 0x8000])
    }

    /// The parsed cartridge header, if the ROM carried a valid footer.
    pub fn header(&self) -> Option<&CartridgeHeader> {
        self.header.as_ref()
    }

    /// The cartridge's bank-switch mapper chip.
    pub fn mapper(&self) -> Mapper {
        self.mapper
    }

    // ── Bank-value resolution (mapper-aware) ──────────────────────────────

    /// Combine a low/high bank-register pair into the effective bank number.
    ///
    /// The Bandai 2001 ignores the high byte (8-bit banks); the Bandai 2003
    /// uses both (16-bit banks).
    fn bank(&self, lo: u8, hi: u8) -> u32 {
        match self.mapper {
            Mapper::Bandai2001 => lo as u32,
            Mapper::Bandai2003 => u16::from_le_bytes([lo, hi]) as u32,
        }
    }

    fn rom_offset(&self, bank: u32, addr: u32) -> usize {
        ((bank << 16) | (addr & 0xFFFF)) as usize % self.rom.len()
    }

    // ── SRAM ──────────────────────────────────────────────────────────────

    /// Read a byte from the selected SRAM bank (open bus if no SRAM).
    pub fn read_sram(&self, addr: u32) -> u8 {
        if self.sram.is_empty() {
            return OPEN_BUS;
        }
        let bank = self.bank(self.ram_bank, self.ram_bank_hi);
        let offset = ((bank << 16) | (addr & 0xFFFF)) as usize % self.sram.len();
        self.sram[offset]
    }

    /// Write a byte to the selected SRAM bank (ignored if no SRAM).
    pub fn write_sram(&mut self, addr: u32, value: u8) {
        if self.sram.is_empty() {
            return;
        }
        let bank = self.bank(self.ram_bank, self.ram_bank_hi);
        let offset = ((bank << 16) | (addr & 0xFFFF)) as usize % self.sram.len();
        self.sram[offset] = value;
    }

    // ── ROM ───────────────────────────────────────────────────────────────

    /// Read from ROM bank 0 (mapped at 0x20000–0x2FFFF).
    pub fn read_rom0(&self, addr: u32) -> u8 {
        if self.rom.is_empty() {
            return OPEN_BUS;
        }
        let bank = self.bank(self.rom_bank0, self.rom_bank0_hi);
        self.rom[self.rom_offset(bank, addr)]
    }

    /// Read from ROM bank 1 (mapped at 0x30000–0x3FFFF).
    pub fn read_rom1(&self, addr: u32) -> u8 {
        if self.rom.is_empty() {
            return OPEN_BUS;
        }
        let bank = self.bank(self.rom_bank1, self.rom_bank1_hi);
        self.rom[self.rom_offset(bank, addr)]
    }

    /// Read from the linear ROM range (0x40000–0xFFFFF) using `linear_off`.
    pub fn read_rom_ex(&self, addr: u32) -> u8 {
        if self.rom.is_empty() {
            return OPEN_BUS;
        }
        let hi = (self.linear_off as u32) << 20;
        let offset = (hi | (addr & 0xFFFFF)) as usize % self.rom.len();
        self.rom[offset]
    }

    // ── Serial EEPROM (cartridge save) ────────────────────────────────────

    /// Whether this cartridge carries a serial EEPROM save medium.
    pub fn has_eeprom(&self) -> bool {
        self.eeprom.is_some()
    }

    /// Drive an EEPROM control-port (0xC8) operation.
    ///
    /// `operation` is the high nibble written to the control port; `data` and
    /// `comm` are the latched data (0xC4/0xC5) and command (0xC6/0xC7) words.
    /// Returns the value to latch back onto the data port — the freshly read
    /// word for a READ, or `data` unchanged otherwise.
    pub fn eeprom_control(&mut self, operation: u8, data: u16, comm: u16) -> u16 {
        let Some(eeprom) = self.eeprom.as_mut() else {
            return data;
        };
        match operation {
            EEPROM_OP_READ => {
                eeprom.execute(comm);
                eeprom.read_data()
            }
            EEPROM_OP_WRITE => {
                eeprom.write_data(data);
                eeprom.execute(comm);
                data
            }
            EEPROM_OP_COMMAND => {
                eeprom.execute(comm);
                data
            }
            _ => data,
        }
    }

    // ── RTC (see rtc.rs) ──────────────────────────────────────────────────

    /// Whether this cartridge carries a real-time clock.
    pub fn has_rtc(&self) -> bool {
        self.rtc.is_some()
    }

    /// The cartridge's real-time clock, if present.
    pub fn rtc(&self) -> Option<&Rtc> {
        self.rtc.as_ref()
    }

    /// Mutable access to the cartridge's real-time clock, if present.
    ///
    /// Used by the bus to service the 0xCA/0xCB command protocol and to advance
    /// the clock off the emulated master clock.
    pub fn rtc_mut(&mut self) -> Option<&mut Rtc> {
        self.rtc.as_mut()
    }

    // ── Save-data serialisation ───────────────────────────────────────────

    /// Whether the cartridge has any persistent save medium.
    pub fn has_save(&self) -> bool {
        !self.sram.is_empty() || self.eeprom.is_some()
    }

    /// The raw persistent save bytes (SRAM contents, or EEPROM contents, or an
    /// empty slice). Stable zero-copy read API for the frontend's file I/O.
    pub fn save_data(&self) -> &[u8] {
        if !self.sram.is_empty() {
            &self.sram
        } else if let Some(eeprom) = &self.eeprom {
            eeprom.contents()
        } else {
            &[]
        }
    }

    /// Overwrite the save medium from previously serialised bytes.
    ///
    /// Copies up to the medium's capacity; a shorter slice leaves the remaining
    /// bytes untouched, a longer slice is truncated. No-op if the cartridge has
    /// no save medium.
    pub fn load_save_data(&mut self, data: &[u8]) {
        if !self.sram.is_empty() {
            let n = data.len().min(self.sram.len());
            self.sram[..n].copy_from_slice(&data[..n]);
        } else if let Some(eeprom) = self.eeprom.as_mut() {
            eeprom.load_contents(data);
        }
    }
}
