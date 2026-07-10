//! Unit tests for cartridge header parsing, mapper-aware banking, the serial
//! EEPROM device, the RTC interface, and save-data serialisation.
//!
//! One assertion per test (Apollo Rust best practices, Ch. 5.1).

use super::eeprom::Eeprom;
use super::header::FOOTER_LEN;
use super::{Cartridge, CartridgeHeader, Mapper, Rtc, SaveType};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Build a ROM of `size` bytes whose footer bytes are set from `(offset, value)`
/// pairs (offsets are relative to the start of the 16-byte footer).
fn rom_with_footer(size: usize, footer: &[(usize, u8)]) -> Vec<u8> {
    let mut rom = vec![0u8; size];
    let base = size - FOOTER_LEN;
    for &(off, val) in footer {
        rom[base + off] = val;
    }
    rom
}

/// 192 KiB ROM (3 banks of 64 KiB) carrying the given mapper code in its footer,
/// with a distinct marker byte 5 bytes into each bank.
fn three_bank_rom(mapper_code: u8) -> Vec<u8> {
    let mut rom = rom_with_footer(0x30000, &[(0xD, mapper_code)]);
    rom[0x00005] = 0xA0; // bank 0 marker
    rom[0x10005] = 0xA1; // bank 1 marker
    rom[0x20005] = 0xA2; // bank 2 marker
    rom
}

// ── 6a: header parsing ───────────────────────────────────────────────────────

#[test]
fn header_parse_returns_none_when_rom_shorter_than_footer() {
    assert_eq!(CartridgeHeader::parse(&[0u8; 8]), None);
}

#[test]
fn header_parse_reads_publisher() {
    let rom = rom_with_footer(0x100, &[(0x6, 0x42)]);
    assert_eq!(CartridgeHeader::parse(&rom).unwrap().publisher, 0x42);
}

#[test]
fn header_parse_reads_color_flag() {
    let rom = rom_with_footer(0x100, &[(0x7, 0x01)]);
    assert!(CartridgeHeader::parse(&rom).unwrap().color);
}

#[test]
fn header_parse_clears_color_flag_for_mono() {
    let rom = rom_with_footer(0x100, &[(0x7, 0x00)]);
    assert!(!CartridgeHeader::parse(&rom).unwrap().color);
}

#[test]
fn header_parse_reads_game_id() {
    let rom = rom_with_footer(0x100, &[(0x8, 0x7B)]);
    assert_eq!(CartridgeHeader::parse(&rom).unwrap().game_id, 0x7B);
}

#[test]
fn header_parse_reads_version() {
    let rom = rom_with_footer(0x100, &[(0x9, 0x03)]);
    assert_eq!(CartridgeHeader::parse(&rom).unwrap().version, 0x03);
}

#[test]
fn header_parse_reads_rom_size_code() {
    let rom = rom_with_footer(0x100, &[(0xA, 0x06)]);
    assert_eq!(CartridgeHeader::parse(&rom).unwrap().rom_size_code, 0x06);
}

#[test]
fn header_parse_decodes_sram_save_type() {
    let rom = rom_with_footer(0x100, &[(0xB, 0x02)]);
    assert_eq!(
        CartridgeHeader::parse(&rom).unwrap().save_type,
        SaveType::Sram(32 * 1024)
    );
}

#[test]
fn header_parse_decodes_eeprom_save_type() {
    let rom = rom_with_footer(0x100, &[(0xB, 0x10)]);
    assert_eq!(
        CartridgeHeader::parse(&rom).unwrap().save_type,
        SaveType::Eeprom(128)
    );
}

#[test]
fn header_parse_decodes_no_save_type() {
    let rom = rom_with_footer(0x100, &[(0xB, 0x00)]);
    assert_eq!(
        CartridgeHeader::parse(&rom).unwrap().save_type,
        SaveType::None
    );
}

#[test]
fn header_parse_reads_vertical_flag() {
    let rom = rom_with_footer(0x100, &[(0xC, 0x01)]);
    assert!(CartridgeHeader::parse(&rom).unwrap().vertical);
}

#[test]
fn header_parse_decodes_bandai_2001_mapper() {
    let rom = rom_with_footer(0x100, &[(0xD, 0x00)]);
    assert_eq!(
        CartridgeHeader::parse(&rom).unwrap().mapper,
        Mapper::Bandai2001
    );
}

#[test]
fn header_parse_decodes_bandai_2003_mapper() {
    let rom = rom_with_footer(0x100, &[(0xD, 0x01)]);
    assert_eq!(
        CartridgeHeader::parse(&rom).unwrap().mapper,
        Mapper::Bandai2003
    );
}

#[test]
fn header_parse_reads_checksum_little_endian() {
    let rom = rom_with_footer(0x100, &[(0xE, 0x34), (0xF, 0x12)]);
    assert_eq!(CartridgeHeader::parse(&rom).unwrap().checksum, 0x1234);
}

#[test]
fn save_type_size_reports_sram_capacity() {
    assert_eq!(SaveType::Sram(0x8000).size(), 0x8000);
}

#[test]
fn save_type_size_reports_zero_for_none() {
    assert_eq!(SaveType::None.size(), 0);
}

// ── 6b: mapper-aware banking ─────────────────────────────────────────────────

#[test]
fn bandai_2001_selects_rom_bank_by_low_byte() {
    let mut cart = Cartridge::new(three_bank_rom(0x00), Vec::new());
    cart.rom_bank0 = 2;
    assert_eq!(cart.read_rom0(0x20005), 0xA2);
}

#[test]
fn bandai_2001_ignores_rom_bank_high_byte() {
    let mut cart = Cartridge::new(three_bank_rom(0x00), Vec::new());
    cart.rom_bank0 = 0;
    cart.rom_bank0_hi = 1; // ignored on the 8-bit 2001 mapper
    assert_eq!(cart.read_rom0(0x20005), 0xA0);
}

#[test]
fn bandai_2003_combines_rom_bank_high_byte() {
    // High byte 1, low byte 0 → bank 256; 256 mod 3 banks selects bank 1.
    let mut cart = Cartridge::new(three_bank_rom(0x01), Vec::new());
    cart.rom_bank0 = 0;
    cart.rom_bank0_hi = 1;
    assert_eq!(cart.read_rom0(0x20005), 0xA1);
}

#[test]
fn rom_bank1_uses_its_own_register() {
    let mut cart = Cartridge::new(three_bank_rom(0x00), Vec::new());
    cart.rom_bank1 = 1;
    assert_eq!(cart.read_rom1(0x30005), 0xA1);
}

#[test]
fn linear_offset_addresses_extended_rom_range() {
    let mut rom = rom_with_footer(0x200000, &[(0xD, 0x00)]);
    // linear offset 1 → (1 << 20) | (0x40005 & 0xFFFFF) = 0x140005
    rom[0x140005] = 0x5C;
    let mut cart = Cartridge::new(rom, Vec::new());
    cart.linear_off = 1;
    assert_eq!(cart.read_rom_ex(0x40005), 0x5C);
}

#[test]
fn sram_read_uses_selected_bank() {
    let mut cart = Cartridge::new(rom_with_footer(0x100, &[]), vec![0u8; 0x20000]);
    cart.sram[0x10005] = 0x77; // bank 1, offset 5
    cart.ram_bank = 1;
    assert_eq!(cart.read_sram(0x10005), 0x77);
}

#[test]
fn sram_write_then_read_round_trips() {
    let mut cart = Cartridge::new(rom_with_footer(0x100, &[]), vec![0u8; 0x8000]);
    cart.write_sram(0x10010, 0x5A);
    assert_eq!(cart.read_sram(0x10010), 0x5A);
}

#[test]
fn sram_read_is_open_bus_without_sram() {
    let cart = Cartridge::new(rom_with_footer(0x100, &[]), Vec::new());
    assert_eq!(cart.read_sram(0x10000), super::OPEN_BUS);
}

// ── 6c: serial EEPROM device ─────────────────────────────────────────────────

/// 128-byte EEPROM (64 words, 6 address bits).
fn small_eeprom() -> Eeprom {
    Eeprom::new(vec![0xFFu8; 128], 6)
}

/// Command word for a 6-address-bit EEPROM: start bit, 2-bit opcode, 6-bit addr.
fn cmd(opcode: u16, word_addr: u16) -> u16 {
    (1 << 8) | (opcode << 6) | word_addr
}

#[test]
fn eeprom_address_bits_for_known_sizes() {
    assert_eq!(Eeprom::address_bits_for(2048), Some(10));
}

#[test]
fn eeprom_address_bits_for_unknown_size_is_none() {
    assert_eq!(Eeprom::address_bits_for(777), None);
}

#[test]
fn eeprom_write_then_read_round_trips() {
    let mut eeprom = small_eeprom();
    eeprom.write_data(0xABCD);
    eeprom.execute(cmd(1, 0)); // WRITE word 0
    eeprom.execute(cmd(2, 0)); // READ word 0
    assert_eq!(eeprom.read_data(), 0xABCD);
}

#[test]
fn eeprom_erase_sets_word_to_ones() {
    let mut eeprom = small_eeprom();
    eeprom.write_data(0x0000);
    eeprom.execute(cmd(1, 1)); // WRITE word 1 = 0
    eeprom.execute(cmd(3, 1)); // ERASE word 1
    eeprom.execute(cmd(2, 1)); // READ word 1
    assert_eq!(eeprom.read_data(), 0xFFFF);
}

#[test]
fn eeprom_write_disable_blocks_writes() {
    let mut eeprom = small_eeprom();
    eeprom.execute(cmd(0, 0b00 << 4)); // EWDS (write disable)
    eeprom.write_data(0x1234);
    eeprom.execute(cmd(1, 0)); // WRITE attempt
    eeprom.execute(cmd(2, 0)); // READ
    assert_eq!(eeprom.read_data(), 0xFFFF);
}

#[test]
fn eeprom_write_enable_restores_writes() {
    let mut eeprom = small_eeprom();
    eeprom.execute(cmd(0, 0b00 << 4)); // EWDS
    eeprom.execute(cmd(0, 0b11 << 4)); // EWEN (write enable)
    eeprom.write_data(0x1234);
    eeprom.execute(cmd(1, 0));
    eeprom.execute(cmd(2, 0));
    assert_eq!(eeprom.read_data(), 0x1234);
}

#[test]
fn eeprom_erase_all_clears_every_word() {
    let mut eeprom = small_eeprom();
    eeprom.write_data(0x0000);
    eeprom.execute(cmd(1, 5)); // WRITE word 5 = 0
    eeprom.execute(cmd(0, 0b10 << 4)); // ERAL (erase all)
    eeprom.execute(cmd(2, 5)); // READ word 5
    assert_eq!(eeprom.read_data(), 0xFFFF);
}

#[test]
fn eeprom_command_without_start_bit_is_ignored() {
    let mut eeprom = small_eeprom();
    eeprom.write_data(0x1234);
    eeprom.execute(1 << 6); // WRITE opcode, word 0, but start bit clear
    eeprom.execute(cmd(2, 0)); // READ
    assert_eq!(eeprom.read_data(), 0xFFFF);
}

// ── 6c (cont.): EEPROM via the cartridge control interface ───────────────────

/// 128-byte EEPROM cartridge built through the header path (footer code 0x10).
fn eeprom_cart() -> Cartridge {
    Cartridge::from_rom(rom_with_footer(0x100, &[(0xB, 0x10)]))
}

#[test]
fn from_rom_allocates_eeprom_for_eeprom_save_type() {
    assert!(eeprom_cart().has_eeprom());
}

#[test]
fn cartridge_eeprom_control_write_then_read() {
    let mut cart = eeprom_cart();
    cart.eeprom_control(0b0100, 0x0000, cmd(0, 0b11 << 4)); // EWEN
    cart.eeprom_control(0b0010, 0xBEEF, cmd(1, 0)); // WRITE word 0 = 0xBEEF
    let read = cart.eeprom_control(0b0001, 0x0000, cmd(2, 0)); // READ word 0
    assert_eq!(read, 0xBEEF);
}

// ── 6d/8e: RTC interface ─────────────────────────────────────────────────────

#[test]
fn cartridge_has_no_rtc_in_phase_6() {
    assert!(!eeprom_cart().has_rtc());
}

#[test]
fn cartridge_rtc_accessor_is_none() {
    assert!(eeprom_cart().rtc().is_none());
}

#[test]
fn rtc_state_round_trips_through_load() {
    let mut rtc = Rtc::new();
    rtc.load_state(&[1, 2, 3, 4]);
    assert_eq!(&rtc.state()[..4], &[1, 2, 3, 4]);
}

// ── 6e: save-data serialisation ──────────────────────────────────────────────

#[test]
fn from_rom_allocates_sram_for_sram_save_type() {
    // Footer save code 0x01 → 8 KiB SRAM.
    let cart = Cartridge::from_rom(rom_with_footer(0x100, &[(0xB, 0x01)]));
    assert_eq!(cart.save_data().len(), 8 * 1024);
}

#[test]
fn save_data_is_empty_without_save_medium() {
    let cart = Cartridge::from_rom(rom_with_footer(0x100, &[(0xB, 0x00)]));
    assert!(cart.save_data().is_empty());
}

#[test]
fn load_save_data_restores_sram_contents() {
    let mut cart = Cartridge::from_rom(rom_with_footer(0x100, &[(0xB, 0x01)]));
    let mut blob = vec![0u8; 8 * 1024];
    blob[0x40] = 0x9E;
    cart.load_save_data(&blob);
    assert_eq!(cart.read_sram(0x10040), 0x9E);
}

#[test]
fn load_save_data_restores_eeprom_contents() {
    let mut cart = eeprom_cart();
    let mut blob = vec![0u8; 128];
    blob[0] = 0xCD;
    blob[1] = 0xAB;
    cart.load_save_data(&blob);
    let read = cart.eeprom_control(0b0001, 0x0000, cmd(2, 0)); // READ word 0
    assert_eq!(read, 0xABCD);
}

#[test]
fn has_save_is_false_for_save_less_cartridge() {
    let cart = Cartridge::from_rom(rom_with_footer(0x100, &[(0xB, 0x00)]));
    assert!(!cart.has_save());
}
