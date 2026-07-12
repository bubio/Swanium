//! Integration tests for cartridge banking, the serial EEPROM, and save-data
//! persistence, driven through the public `Bus` + `MemoryBus` API (I/O ports
//! and the physical memory map) rather than the cartridge internals.
//!
//! One assertion per test (Apollo Rust best practices, Ch. 5.1).

use swanium_core::bus::Bus;
use swanium_core::cpu::MemoryBus;

const FOOTER_LEN: usize = 16;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Build a ROM of `size` bytes with the given footer `(offset, value)` pairs.
fn rom_with_footer(size: usize, footer: &[(usize, u8)]) -> Vec<u8> {
    let mut rom = vec![0u8; size];
    let base = size - FOOTER_LEN;
    for &(off, val) in footer {
        rom[base + off] = val;
    }
    rom
}

/// 192 KiB ROM (3 banks) with the given mapper code and a marker in each bank.
fn three_bank_rom(mapper_code: u8) -> Vec<u8> {
    let mut rom = rom_with_footer(0x30000, &[(0xD, mapper_code)]);
    rom[0x00005] = 0xA0;
    rom[0x10005] = 0xA1;
    rom[0x20005] = 0xA2;
    rom
}

// ── ROM bank switching through I/O ports ─────────────────────────────────────

#[test]
fn rom_bank0_register_selects_bank() {
    let mut bus = Bus::new(three_bank_rom(0x00));
    bus.write_io(0xC2, 2); // ROM bank 0 ← 2
    assert_eq!(bus.read_u8(0x20005), 0xA2);
}

#[test]
fn rom_bank1_register_selects_bank() {
    let mut bus = Bus::new(three_bank_rom(0x00));
    bus.write_io(0xC3, 1); // ROM bank 1 ← 1
    assert_eq!(bus.read_u8(0x30005), 0xA1);
}

#[test]
fn bandai_2001_ignores_high_byte_bank_port() {
    let mut bus = Bus::new(three_bank_rom(0x00)); // mapper 2001
    bus.write_io(0xC2, 0);
    bus.write_io(0xD3, 1); // high byte port ignored on 2001
    assert_eq!(bus.read_u8(0x20005), 0xA0);
}

#[test]
fn bandai_2003_high_byte_bank_port_extends_bank() {
    let mut bus = Bus::new(three_bank_rom(0x01)); // mapper 2003
    bus.write_io(0xC2, 0);
    bus.write_io(0xD3, 1); // bank ← 0x0100 = 256; 256 mod 3 = bank 1
    assert_eq!(bus.read_u8(0x20005), 0xA1);
}

#[test]
fn bandai_2003_high_byte_selects_large_rom_bank_without_wrapping() {
    let mut rom = rom_with_footer(0x0110_0000, &[(0xD, 0x01)]);
    rom[0x0100_0005] = 0xD3;
    let mut bus = Bus::new(rom);
    bus.write_io(0xC2, 0);
    bus.write_io(0xD3, 1);
    assert_eq!(bus.read_u8(0x20005), 0xD3);
}

// ── SRAM persistence ─────────────────────────────────────────────────────────

#[test]
fn from_rom_maps_writable_sram() {
    let mut bus = Bus::from_rom(rom_with_footer(0x10000, &[(0xB, 0x01)])); // 8 KiB SRAM
    bus.write_u8(0x10040, 0x9E);
    assert_eq!(bus.read_u8(0x10040), 0x9E);
}

#[test]
fn sram_writes_appear_in_save_data() {
    let mut bus = Bus::from_rom(rom_with_footer(0x10000, &[(0xB, 0x01)]));
    bus.write_u8(0x10040, 0x9E);
    assert_eq!(bus.save_data()[0x40], 0x9E);
}

#[test]
fn load_save_data_restores_sram_into_memory_map() {
    let mut bus = Bus::from_rom(rom_with_footer(0x10000, &[(0xB, 0x01)]));
    let mut blob = vec![0u8; 8 * 1024];
    blob[0x40] = 0x3C;
    bus.load_save_data(&blob);
    assert_eq!(bus.read_u8(0x10040), 0x3C);
}

#[test]
fn save_data_is_empty_for_save_less_cartridge() {
    let bus = Bus::from_rom(rom_with_footer(0x10000, &[(0xB, 0x00)]));
    assert!(bus.save_data().is_empty());
}

// ── Serial EEPROM through I/O ports ──────────────────────────────────────────

/// Build a bus whose cartridge carries a 128-byte EEPROM (save code 0x10).
fn eeprom_bus() -> Bus {
    Bus::from_rom(rom_with_footer(0x10000, &[(0xB, 0x10)]))
}

/// Issue an EEPROM command through ports 0xC4–0xC8 and return nothing; the data
/// latch (0xC4/0xC5) holds any READ result afterwards.
fn eeprom_op(bus: &mut Bus, op_nibble: u8, data: u16, comm: u16) {
    let [data_lo, data_hi] = data.to_le_bytes();
    let [comm_lo, comm_hi] = comm.to_le_bytes();
    bus.write_io(0xC4, data_lo);
    bus.write_io(0xC5, data_hi);
    bus.write_io(0xC6, comm_lo);
    bus.write_io(0xC7, comm_hi);
    bus.write_io(0xC8, op_nibble << 4);
}

/// Command word for the 6-address-bit cartridge EEPROM.
fn cmd(opcode: u16, word_addr: u16) -> u16 {
    (1 << 8) | (opcode << 6) | word_addr
}

#[test]
fn eeprom_status_port_reports_ready_when_present() {
    let mut bus = eeprom_bus();
    assert_eq!(bus.read_io(0xC8), 0x02);
}

#[test]
fn eeprom_write_then_read_through_ports() {
    let mut bus = eeprom_bus();
    eeprom_op(&mut bus, 0b0100, 0x0000, cmd(0, 0b11 << 4)); // EWEN
    eeprom_op(&mut bus, 0b0010, 0xBEEF, cmd(1, 0)); // WRITE word 0 = 0xBEEF
    eeprom_op(&mut bus, 0b0001, 0x0000, cmd(2, 0)); // READ word 0 → data latch
    assert_eq!(bus.read_io(0xC4), 0xEF);
}

#[test]
fn eeprom_read_latches_high_byte() {
    let mut bus = eeprom_bus();
    eeprom_op(&mut bus, 0b0100, 0x0000, cmd(0, 0b11 << 4)); // EWEN
    eeprom_op(&mut bus, 0b0010, 0xBEEF, cmd(1, 0));
    eeprom_op(&mut bus, 0b0001, 0x0000, cmd(2, 0));
    assert_eq!(bus.read_io(0xC5), 0xBE);
}

#[test]
fn absent_eeprom_status_port_reads_open_bus() {
    let mut bus = Bus::new(three_bank_rom(0x00)); // no EEPROM
    assert_eq!(bus.read_io(0xC8), 0x90); // open bus
}

#[test]
fn absent_eeprom_data_port_reads_open_bus() {
    let mut bus = Bus::new(three_bank_rom(0x00)); // no EEPROM
    bus.write_io(0xC4, 0x5A);
    assert_eq!(bus.read_io(0xC4), 0x90);
}

#[test]
fn eeprom_save_data_round_trips_through_bus() {
    let mut bus = eeprom_bus();
    let mut blob = vec![0u8; 128];
    blob[0] = 0xCD;
    blob[1] = 0xAB;
    bus.load_save_data(&blob);
    eeprom_op(&mut bus, 0b0001, 0x0000, cmd(2, 0)); // READ word 0
    assert_eq!(bus.read_io(0xC4), 0xCD);
}

#[test]
fn eeprom_write_all_initialization_fills_selected_word() {
    let mut bus = eeprom_bus();
    eeprom_op(&mut bus, 0b0100, 0x0000, cmd(0, 0b11 << 4)); // EWEN
    eeprom_op(&mut bus, 0b0010, 0x1234, cmd(0, 0b01 << 4)); // WRAL
    eeprom_op(&mut bus, 0b0001, 0x0000, cmd(2, 17)); // READ word 17
    assert_eq!(bus.read_io(0xC4), 0x34);
}

#[test]
fn eeprom_write_disable_initialization_blocks_later_write() {
    let mut bus = eeprom_bus();
    eeprom_op(&mut bus, 0b0100, 0x0000, cmd(0, 0b00 << 4)); // EWDS
    eeprom_op(&mut bus, 0b0010, 0x1234, cmd(1, 0)); // WRITE word 0
    eeprom_op(&mut bus, 0b0001, 0x0000, cmd(2, 0)); // READ word 0
    assert_eq!(bus.read_io(0xC4), 0xFF);
}

// ── Cartridge RTC through I/O ports ─────────────────────────────────────────

/// Build a bus whose cartridge footer marks an RTC device as present.
fn rtc_bus() -> Bus {
    Bus::from_rom(rom_with_footer(0x10000, &[(0xC, 0x04), (0xD, 0x01)]))
}

#[test]
fn rtc_footer_bit_exposes_ready_command_port() {
    let mut bus = rtc_bus();
    bus.write_io(0xCA, 0x14);
    assert_eq!(bus.read_io(0xCA), 0x90);
}

#[test]
fn rtc_datetime_read_returns_bcd_bytes_in_register_order() {
    let mut bus = rtc_bus();
    bus.set_rtc_datetime(26, 7, 3, 5, 12, 34, 56);
    bus.write_io(0xCA, 0x14);
    let got: Vec<u8> = (0..7).map(|_| bus.read_io(0xCB)).collect();
    assert_eq!(got, vec![0x26, 0x07, 0x03, 5, 0x12, 0x34, 0x56]);
}

#[test]
fn rtc_status_write_then_read_round_trips() {
    let mut bus = rtc_bus();
    bus.write_io(0xCA, 0x13);
    bus.write_io(0xCB, 0xA5);
    bus.write_io(0xCA, 0x12);
    assert_eq!(bus.read_io(0xCB), 0xA5);
}

#[test]
fn absent_rtc_command_port_reads_open_bus() {
    let mut bus = Bus::from_rom(rom_with_footer(0x10000, &[]));
    bus.write_io(0xCA, 0x14);
    assert_eq!(bus.read_io(0xCA), 0x90);
}

#[test]
fn rtc_bearing_cart_keeps_cartridge_save_data_empty() {
    let bus = rtc_bus();
    assert!(bus.save_data().is_empty());
}
