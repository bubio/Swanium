//! WonderSwan cartridge header (ROM footer) parsing.
//!
//! Every WonderSwan ROM ends with a 16-byte footer occupying the last 16 bytes
//! of the image (physical 0xFFFF0–0xFFFFF in the linear mapping, which is also
//! where the CPU begins executing at reset). The footer encodes the publisher,
//! hardware model, ROM/save sizes, mapper chip, and a checksum.
//!
//! Byte layout (offsets relative to the start of the 16-byte footer):
//!
//! | Offset | Field                                                |
//! |--------|------------------------------------------------------|
//! | 0x0–5  | Boot entry point (`JMP FAR`) — not parsed here       |
//! | 0x6    | Publisher (maker) ID                                 |
//! | 0x7    | System: bit 0 set ⇒ WonderSwan Color required        |
//! | 0x8    | Game (cartridge) ID                                  |
//! | 0x9    | Game revision / version                              |
//! | 0xA    | ROM size code                                        |
//! | 0xB    | Save type code (SRAM / EEPROM size, or none)         |
//! | 0xC    | Flags: bit 0 orientation, bit 2 bus width, bit 3 speed |
//! | 0xD    | Mapper chip (0 ⇒ Bandai 2001, 1 ⇒ Bandai 2003)       |
//! | 0xE–F  | 16-bit checksum (little-endian)                      |
//!
//! The mapper byte at offset 0xD follows WonderCrab's verified interpretation.
//! Auto-detection of an on-cartridge RTC from the header is left unverified and
//! tracked as a Phase 6 follow-up; see `docs/dev/DevelopmentPlan.md`.

/// Length of the WonderSwan ROM footer in bytes.
pub const FOOTER_LEN: usize = 16;

// Footer byte offsets (relative to the start of the 16-byte footer).
const OFF_PUBLISHER: usize = 0x6;
const OFF_SYSTEM: usize = 0x7;
const OFF_GAME_ID: usize = 0x8;
const OFF_VERSION: usize = 0x9;
const OFF_ROM_SIZE: usize = 0xA;
const OFF_SAVE_TYPE: usize = 0xB;
const OFF_FLAGS: usize = 0xC;
const OFF_MAPPER: usize = 0xD;
const OFF_CHECKSUM: usize = 0xE;

/// Bit 0 of the system byte: cartridge requires WonderSwan Color.
const SYSTEM_COLOR: u8 = 0x01;
/// Bit 0 of the flags byte: screen orientation (1 ⇒ vertical).
const FLAG_VERTICAL: u8 = 0x01;

/// The mapper (bank-switch) chip a cartridge uses.
///
/// The bank-register width differs between the two Bandai mappers: see
/// [`super::Cartridge`] for how each one resolves a physical ROM/SRAM address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mapper {
    /// Bandai 2001: 8-bit bank registers (the common mono/early-Color mapper).
    Bandai2001,
    /// Bandai 2003: 16-bit bank registers (high bytes via ports 0xD0–0xD5).
    Bandai2003,
}

impl Mapper {
    /// Decode the footer mapper byte (offset 0xD).
    ///
    /// Unknown codes fall back to [`Mapper::Bandai2001`], the most common chip,
    /// rather than panicking, so that homebrew or malformed footers still load.
    fn from_code(code: u8) -> Self {
        match code {
            1 => Mapper::Bandai2003,
            _ => Mapper::Bandai2001,
        }
    }
}

/// The persistent-save backing a cartridge provides, with its capacity in bytes.
///
/// A cartridge has at most one save device. SRAM is byte-addressable through the
/// memory map (0x10000–0x1FFFF); EEPROM is a serial device accessed through I/O
/// ports. The capacity drives both allocation and (for EEPROM) the command
/// address width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveType {
    /// No persistent save.
    None,
    /// Battery-backed SRAM of the given size in bytes.
    Sram(usize),
    /// Serial EEPROM of the given size in bytes.
    Eeprom(usize),
}

impl SaveType {
    /// Decode the footer save-type byte (offset 0xB).
    ///
    /// Codes follow the documented WonderSwan save-type table. Unknown codes
    /// decode to [`SaveType::None`] rather than panicking, so an unusual footer
    /// still loads (the game simply has no persistence).
    fn from_code(code: u8) -> Self {
        match code {
            0x01 => SaveType::Sram(8 * 1024),
            0x02 => SaveType::Sram(32 * 1024),
            0x03 => SaveType::Sram(128 * 1024),
            0x04 => SaveType::Sram(256 * 1024),
            0x05 => SaveType::Sram(512 * 1024),
            0x10 => SaveType::Eeprom(128),
            0x20 => SaveType::Eeprom(2 * 1024),
            0x50 => SaveType::Eeprom(1024),
            _ => SaveType::None,
        }
    }

    /// The save device's capacity in bytes (0 for [`SaveType::None`]).
    pub fn size(self) -> usize {
        match self {
            SaveType::None => 0,
            SaveType::Sram(n) | SaveType::Eeprom(n) => n,
        }
    }
}

/// Parsed WonderSwan cartridge header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CartridgeHeader {
    /// Publisher (maker) ID.
    pub publisher: u8,
    /// Cartridge requires WonderSwan Color hardware.
    pub color: bool,
    /// Game (cartridge) ID.
    pub game_id: u8,
    /// Game revision / version.
    pub version: u8,
    /// Raw ROM size code (footer offset 0xA).
    pub rom_size_code: u8,
    /// Persistent-save device described by the footer.
    pub save_type: SaveType,
    /// Screen orientation is vertical (otherwise horizontal).
    pub vertical: bool,
    /// Bank-switch mapper chip.
    pub mapper: Mapper,
    /// 16-bit footer checksum.
    pub checksum: u16,
}

impl CartridgeHeader {
    /// Parse the header from a full ROM image.
    ///
    /// Returns `None` if `rom` is shorter than the 16-byte footer.
    pub fn parse(rom: &[u8]) -> Option<Self> {
        let footer = rom.get(rom.len().checked_sub(FOOTER_LEN)?..)?;
        Some(Self {
            publisher: footer[OFF_PUBLISHER],
            color: footer[OFF_SYSTEM] & SYSTEM_COLOR != 0,
            game_id: footer[OFF_GAME_ID],
            version: footer[OFF_VERSION],
            rom_size_code: footer[OFF_ROM_SIZE],
            save_type: SaveType::from_code(footer[OFF_SAVE_TYPE]),
            vertical: footer[OFF_FLAGS] & FLAG_VERTICAL != 0,
            mapper: Mapper::from_code(footer[OFF_MAPPER]),
            checksum: u16::from_le_bytes([footer[OFF_CHECKSUM], footer[OFF_CHECKSUM + 1]]),
        })
    }
}
