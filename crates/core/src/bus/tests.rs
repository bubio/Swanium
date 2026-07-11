use super::{Bus, IrqSource};
use crate::cpu::{Cpu, MemoryBus};
use crate::model::HardwareModel;

// ── Hardware model selection ─────────────────────────────────────────────────

/// A minimal 16-byte ROM whose footer's system byte (offset 0x7) marks it as
/// requiring WonderSwan Color when `color` is set.
fn rom_with_color_flag(color: bool) -> Vec<u8> {
    let mut rom = vec![0u8; 16];
    rom[0x7] = if color { 0x01 } else { 0x00 };
    rom
}

#[test]
fn new_bus_defaults_to_mono() {
    let bus = Bus::new(vec![0u8; 0x10000]);
    assert_eq!(bus.model(), HardwareModel::Mono);
}

#[test]
fn from_rom_selects_color_for_color_flagged_footer() {
    let bus = Bus::from_rom(rom_with_color_flag(true));
    assert_eq!(bus.model(), HardwareModel::Color);
}

#[test]
fn from_rom_selects_mono_for_mono_footer() {
    let bus = Bus::from_rom(rom_with_color_flag(false));
    assert_eq!(bus.model(), HardwareModel::Mono);
}

#[test]
fn set_model_overrides_the_model() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.set_model(HardwareModel::Crystal);
    assert_eq!(bus.model(), HardwareModel::Crystal);
}

// ── Memory map ───────────────────────────────────────────────────────────────

#[test]
fn wram_write_reads_back_at_base_address() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_u8(0x0000, 0xAB);
    assert_eq!(bus.read_u8(0x0000), 0xAB);
}

#[test]
fn wram_write_reads_back_at_mid_address() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_u8(0x03FF, 0xCD);
    assert_eq!(bus.read_u8(0x03FF), 0xCD);
}

#[test]
fn wram_write_reads_back_at_top_address() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_u8(0x3FFF, 0xEF);
    assert_eq!(bus.read_u8(0x3FFF), 0xEF);
}

#[test]
fn wram_16bit_write_reads_back_same_value() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_u16(0x0100, 0xBEEF);
    assert_eq!(bus.read_u16(0x0100), 0xBEEF);
}

#[test]
fn wram_16bit_write_stores_low_byte_first() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_u16(0x0100, 0xBEEF);
    assert_eq!(bus.read_u8(0x0100), 0xEF);
}

#[test]
fn wram_16bit_write_stores_high_byte_second() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_u16(0x0100, 0xBEEF);
    assert_eq!(bus.read_u8(0x0101), 0xBE);
}

#[test]
fn open_bus_returns_0x90_at_start_of_unmapped_range() {
    let bus = Bus::new(vec![0u8; 0x10000]);
    assert_eq!(bus.read_u8(0x04000), 0x90);
}

#[test]
fn open_bus_returns_0x90_at_end_of_unmapped_range() {
    let bus = Bus::new(vec![0u8; 0x10000]);
    assert_eq!(bus.read_u8(0x0FFFF), 0x90);
}

// ── Color 64 KiB internal RAM window (0x04000–0x0FFFF) ────────────────────────

/// Build a Color bus with a zeroed 16-byte ROM (footer marks Color).
fn color_bus() -> Bus {
    Bus::from_rom(rom_with_color_flag(true))
}

fn color_video_bus(rom: Vec<u8>) -> Bus {
    let mut bus = Bus::new(rom);
    bus.set_model(HardwareModel::Color);
    bus.write_io(0x60, 0x80);
    bus
}

#[test]
fn hw_flags_port_a0_reports_color_hardware() {
    // Color/Crystal read 0x87 (bit0 = colour), mono reads 0x86. Games poll this
    // at boot to detect a WonderSwan Color and enable the colour video path.
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    assert_eq!(bus.read_io(0xA0), 0x86); // default model is Mono
    bus.set_model(HardwareModel::Color);
    assert_eq!(bus.read_io(0xA0), 0x87);
    bus.set_model(HardwareModel::Crystal);
    assert_eq!(bus.read_io(0xA0), 0x87);
}

#[test]
fn hw_flags_port_a0_ignores_writes() {
    // 0xA0 is a computed read; writes do not change what it reports.
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xA0, 0x00);
    assert_eq!(bus.read_io(0xA0), 0x86);
}

#[test]
fn internal_eeprom_read_reports_ready_and_returns_default_word() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xBC, 0x00);
    bus.write_io(0xBD, 0x18); // READ word 0 for 10-bit internal EEPROM.
    bus.write_io(0xBE, 0x10);
    assert_eq!(bus.read_io(0xBE) & 0x01, 0x00);
    assert_eq!(bus.read_io(0xBE) & 0x01, 0x01);
    assert_eq!(bus.read_io(0xBA), 0x00);
    assert_eq!(bus.read_io(0xBB), 0x00);
}

#[test]
fn internal_eeprom_write_then_read_round_trips() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xBA, 0xEF);
    bus.write_io(0xBB, 0xBE);
    bus.write_io(0xBC, 0x00);
    bus.write_io(0xBD, 0x14); // WRITE word 0.
    bus.write_io(0xBE, 0x20);
    assert_eq!(bus.read_io(0xBE) & 0x02, 0x02);

    bus.write_io(0xBC, 0x00);
    bus.write_io(0xBD, 0x18); // READ word 0.
    bus.write_io(0xBE, 0x10);
    assert_eq!(bus.read_io(0xBE) & 0x01, 0x00);
    assert_eq!(bus.read_io(0xBE) & 0x01, 0x01);
    assert_eq!(bus.read_io(0xBA), 0xEF);
    assert_eq!(bus.read_io(0xBB), 0xBE);
}

#[test]
fn internal_eeprom_accepts_mono_width_commands_for_low_addresses() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xBA, 0x55);
    bus.write_io(0xBB, 0xAA);
    bus.write_io(0xBC, 0x40);
    bus.write_io(0xBD, 0x01); // WRITE word 0 using the 6-bit mono-compatible command width.
    bus.write_io(0xBE, 0x20);

    bus.write_io(0xBC, 0x00);
    bus.write_io(0xBD, 0x18); // READ word 0 using the 10-bit Color command width.
    bus.write_io(0xBE, 0x10);
    bus.read_io(0xBE);
    bus.read_io(0xBE);
    assert_eq!(bus.read_io(0xBA), 0x55);
    assert_eq!(bus.read_io(0xBB), 0xAA);
}

#[test]
fn internal_eeprom_protected_byte_range_rejects_writes() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xBA, 0x34);
    bus.write_io(0xBB, 0x12);
    bus.write_io(0xBC, 0x30);
    bus.write_io(0xBD, 0x14); // WRITE byte address 0x60 / word address 0x30.
    bus.write_io(0xBE, 0x20);

    assert_eq!(bus.read_io(0xBE) & 0x80, 0x80);
    bus.write_io(0xBC, 0x30);
    bus.write_io(0xBD, 0x18); // READ byte address 0x60 / word address 0x30.
    bus.write_io(0xBE, 0x10);
    bus.read_io(0xBE);
    bus.read_io(0xBE);
    assert_eq!(bus.read_io(0xBA), 0x00);
    assert_eq!(bus.read_io(0xBB), 0x00);
}

#[test]
fn color_retains_hypervoice_register_writes() {
    // On Color the HyperVoice control/routing registers are writable; 0x6B is
    // masked to 0x6F like Mednafen's `HVoiceChanCtrl`. The PCM data latch feeds
    // the audio path but reads back as 0.
    let mut bus = color_bus();
    bus.write_io(0x69, 0x40);
    bus.write_io(0x6A, 0x8A);
    bus.write_io(0x6B, 0xFF);
    assert_eq!(bus.read_io(0x69), 0x00);
    assert_eq!(bus.read_io(0x6A), 0x8A);
    assert_eq!(bus.read_io(0x6B), 0x6F);
}

#[test]
fn color_hypervoice_direct_register_writes_clear_8_bit_latch() {
    let mut bus = color_bus();
    bus.write_io(0x69, 0x40);
    bus.write_io(0x64, 0x34);
    bus.write_io(0x65, 0x12);
    assert_eq!(bus.read_io(0x64), 0x00);
    assert_eq!(bus.read_io(0x65), 0x00);
    assert_eq!(bus.read_io(0x69), 0x00);
}

#[test]
fn color_hypervoice_8_bit_latch_writes_clear_direct_registers() {
    let mut bus = color_bus();
    bus.write_io(0x64, 0x34);
    bus.write_io(0x65, 0x12);
    bus.write_io(0x69, 0x40);
    assert_eq!(bus.read_io(0x64), 0x00);
    assert_eq!(bus.read_io(0x65), 0x00);
    assert_eq!(bus.read_io(0x69), 0x00);
}

#[test]
fn mono_drops_hypervoice_register_writes() {
    // HyperVoice does not exist on mono hardware: writes to 0x64–0x6B are
    // dropped, so the enable bit is never set and the APU stays silent. Promote
    // to Color afterwards and confirm nothing was stored.
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x64, 0x34);
    bus.write_io(0x6A, 0x80);
    bus.set_model(HardwareModel::Color);
    assert_eq!(bus.read_io(0x64), 0x00);
    assert_eq!(bus.read_io(0x6A), 0x00);
}

#[test]
fn speaker_volume_register_defaults_to_max() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    assert_eq!(bus.read_io(0x9E), 0x03);
}

#[test]
fn speaker_volume_register_keeps_low_two_bits() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x9E, 0xFF);
    assert_eq!(bus.read_io(0x9E), 0x03);
}

#[test]
fn speaker_volume_register_survives_model_switch() {
    let mut bus = color_bus();
    bus.write_io(0x9E, 0x02);
    bus.set_model(HardwareModel::Mono);
    assert_eq!(bus.read_io(0x9E), 0x02);
    bus.set_model(HardwareModel::Color);
    assert_eq!(bus.read_io(0x9E), 0x02);
}

#[test]
fn noise_reset_write_self_clears_and_resets_random_port() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x90, 0x88);
    bus.write_io(0x8E, 0x10);
    bus.tick_apu(1);
    assert_eq!(bus.read_io(0x92), 1);
    bus.write_io(0x8E, 0x18);
    assert_eq!(
        (
            bus.read_io(0x8E) & 0x08,
            bus.read_io(0x92),
            bus.read_io(0x93)
        ),
        (0, 0, 0)
    );
}

#[test]
fn color_wram_write_reads_back_just_above_mono_window() {
    let mut bus = color_bus();
    bus.write_u8(0x04000, 0x5A);
    assert_eq!(bus.read_u8(0x04000), 0x5A);
}

#[test]
fn color_wram_write_reads_back_at_palette_ram() {
    let mut bus = color_bus();
    bus.write_u8(0x0FE00, 0x34);
    bus.write_u8(0x0FE01, 0x02);
    assert_eq!(bus.read_u8(0x0FE00), 0x34);
    assert_eq!(bus.read_u8(0x0FE01), 0x02);
}

#[test]
fn color_wram_write_reads_back_at_top_address() {
    let mut bus = color_bus();
    bus.write_u8(0x0FFFF, 0xC3);
    assert_eq!(bus.read_u8(0x0FFFF), 0xC3);
}

#[test]
fn mono_write_to_upper_window_is_dropped_not_just_read_masked() {
    // On mono the upper window is open bus: the write must not reach the RAM
    // buffer. Promote to Color afterwards and confirm the byte was never stored.
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_u8(0x04000, 0x77);
    bus.set_model(HardwareModel::Color);
    assert_eq!(bus.read_u8(0x04000), 0x00);
}

#[test]
fn mono_still_reads_open_bus_in_upper_window() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_u8(0x08000, 0x11);
    assert_eq!(bus.read_u8(0x08000), 0x90);
}

fn make_reset_vector_rom() -> Bus {
    let mut rom = vec![0u8; 0x10000];
    rom[0xFFF0] = 0x55; // marker at last 16 bytes (reset vector)
    rom[0xFFF1] = 0xAA;
    Bus::new(rom)
}

#[test]
fn rom_ex_maps_first_reset_byte_at_power_on() {
    let bus = make_reset_vector_rom();
    assert_eq!(bus.read_u8(0xFFFF0), 0x55);
}

#[test]
fn rom_ex_maps_second_reset_byte_at_power_on() {
    let bus = make_reset_vector_rom();
    assert_eq!(bus.read_u8(0xFFFF1), 0xAA);
}

#[test]
fn sram_read_write() {
    let rom = vec![0u8; 0x10000];
    let sram = vec![0u8; 0x10000];
    let mut bus = Bus::with_sram(rom, sram);
    // ram_bank defaults to 0xFF; effective SRAM index = (0xFF << 16 | offset) % len
    bus.write_u8(0x10000, 0x77);
    assert_eq!(bus.read_u8(0x10000), 0x77);
}

fn make_3bank_rom() -> Bus {
    let mut rom = vec![0u8; 0x30000]; // 192 KiB ROM
    rom[0x0000] = 0x11; // bank 0
    rom[0x10000] = 0x22; // bank 1
    rom[0x20000] = 0x33; // bank 2
    Bus::new(rom)
}

#[test]
fn rom_bank0_register_bank0_maps_to_0x20000() {
    let mut bus = make_3bank_rom();
    bus.write_io(0xC2, 0x00);
    assert_eq!(bus.read_u8(0x20000), 0x11);
}

#[test]
fn rom_bank0_register_bank1_maps_to_0x20000() {
    let mut bus = make_3bank_rom();
    bus.write_io(0xC2, 0x01);
    assert_eq!(bus.read_u8(0x20000), 0x22);
}

#[test]
fn rom_bank0_register_bank2_maps_to_0x20000() {
    let mut bus = make_3bank_rom();
    bus.write_io(0xC2, 0x02);
    assert_eq!(bus.read_u8(0x20000), 0x33);
}

#[test]
fn writes_to_rom_are_silently_ignored() {
    let mut bus = Bus::new(vec![0xAA; 0x10000]);
    bus.write_u8(0xFFFF0, 0x00);
    assert_eq!(bus.read_u8(0xFFFF0), 0xAA);
}

// ── I/O port basics ──────────────────────────────────────────────────────────

#[test]
fn io_port_raw_write_reads_back() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Port 0x00 (DISP_CTRL) exposes only the low layer/window bits.
    bus.write_io(0x00, 0x42);
    assert_eq!(bus.read_io(0x00), 0x02);
}

#[test]
fn int_enable_preserves_written_bits() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB2, 0b0000_0001);
    assert_eq!(bus.read_io(0xB2), 0b0000_0001);
}

fn bus_with_vblank_and_hblank_requested_then_vblank_cleared() -> Bus {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(
        0xB2,
        (1 << IrqSource::VBlank as u8) | (1 << IrqSource::HBlankTimer as u8),
    );
    bus.request_irq(IrqSource::VBlank);
    bus.request_irq(IrqSource::HBlankTimer);
    bus.write_io(0xB6, 1 << IrqSource::VBlank as u8);
    bus
}

#[test]
fn int_cause_clear_clears_targeted_bit() {
    let mut bus = bus_with_vblank_and_hblank_requested_then_vblank_cleared();
    let cause = bus.read_io(0xB4);
    assert_eq!(cause & (1 << IrqSource::VBlank as u8), 0);
}

#[test]
fn int_cause_clear_leaves_other_bits_intact() {
    let mut bus = bus_with_vblank_and_hblank_requested_then_vblank_cleared();
    let cause = bus.read_io(0xB4);
    assert_eq!(cause & (1 << IrqSource::HBlankTimer as u8), 1 << 7);
}

#[test]
fn serial_tx_irq_latches_when_enabled_uart_is_interrupt_enabled() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB3, 0x84);
    assert_eq!(bus.read_io(0xB4) & 0x01, 0);

    bus.write_io(0xB2, 0x01);
    assert_eq!(bus.read_io(0xB4) & 0x01, 0x01);
}

#[test]
fn serial_tx_irq_ack_reasserts_only_while_enabled() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB3, 0x84);
    bus.write_io(0xB2, 0x01);

    bus.write_io(0xB6, 0x01);
    assert_eq!(
        bus.read_io(0xB4) & 0x01,
        0x01,
        "enabled UART TX is a level IRQ while INT_ENABLE bit 0 is set"
    );

    bus.write_io(0xB2, 0x00);
    bus.write_io(0xB6, 0x01);
    assert_eq!(
        bus.read_io(0xB4) & 0x01,
        0,
        "ACK can clear UART TX once its INT_ENABLE bit is cleared"
    );
}

#[test]
fn serial_tx_irq_level_readback_is_mono_only() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.set_model(HardwareModel::Color);
    bus.write_io(0xB3, 0x84);
    bus.write_io(0xB2, 0x01);

    assert_eq!(bus.read_io(0xB4) & 0x01, 0);
}

#[test]
fn int_vector_read_includes_highest_cause_bit() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB0, 0x87);
    assert_eq!(bus.read_io(0xB0), 0x80);

    bus.write_io(0xB2, 1 << IrqSource::VBlank as u8);
    bus.request_irq(IrqSource::VBlank);
    assert_eq!(bus.read_io(0xB0), 0x86);

    bus.write_io(0xB2, 0);
    assert_eq!(
        bus.read_io(0xB0),
        0x86,
        "changing INT_ENABLE does not clear an already-latched cause"
    );

    bus.write_io(0xB6, 0xFF);
    assert_eq!(bus.read_io(0xB0), 0x80);
}

#[test]
fn int_vector_status_low_bits_are_mono_only() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.set_model(HardwareModel::Color);
    bus.write_io(0xB0, 0x87);
    bus.write_io(0xB2, 1 << IrqSource::VBlank as u8);
    bus.request_irq(IrqSource::VBlank);

    assert_eq!(bus.read_io(0xB0), 0x80);
}

fn setup_gdma_ctrl_read() -> (u8, Bus) {
    let mut bus = color_video_bus(vec![0u8; 0x10000]);
    // 0xC0 = start (bit 7) + decrement direction (bit 6). With length 0 the
    // burst auto-completes instantly, so the busy bit (7) is already clear by
    // the time the CPU reads the register back; only the direction bit remains.
    bus.write_io(0x48, 0xC0);
    let first = bus.read_io(0x48);
    (first, bus)
}

#[test]
fn gdma_ctrl_first_read_returns_direction_bit_after_autocompletion() {
    let (first, _) = setup_gdma_ctrl_read();
    assert_eq!(first, 0x40);
}

#[test]
fn gdma_ctrl_second_read_returns_zero_after_self_clear() {
    let (_, mut bus) = setup_gdma_ctrl_read();
    assert_eq!(bus.read_io(0x48), 0x00);
}

// ── Interrupt controller ─────────────────────────────────────────────────────

#[test]
fn pending_irq_is_none_at_startup() {
    let bus = Bus::new(vec![0u8; 0x10000]);
    assert!(bus.pending_irq().is_none());
}

#[test]
fn vblank_irq_not_pending_before_on_vblank() {
    let bus = Bus::new(vec![0u8; 0x10000]);
    assert!(bus.pending_irq().is_none());
}

fn bus_after_vblank() -> Bus {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB2, 1 << IrqSource::VBlank as u8);
    bus.on_vblank();
    bus
}

#[test]
fn vblank_irq_is_pending_after_on_vblank() {
    let bus = bus_after_vblank();
    assert!(bus.pending_irq().is_some());
}

#[test]
fn vblank_irq_vector_matches_irq_source_index() {
    let bus = bus_after_vblank();
    // With INT_BASE = 0, vector = 0 + 6 = 6
    assert_eq!(bus.pending_irq().unwrap(), IrqSource::VBlank as u8);
}

fn bus_with_gdma_enabled_and_key_requested() -> Bus {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Enable only GDMA (bit 3)
    bus.write_io(0xB2, 1 << IrqSource::GdmaComplete as u8);
    bus.request_irq(IrqSource::KeyPress); // bit 1, not enabled
    bus
}

#[test]
fn pending_irq_is_none_when_only_disabled_source_is_requested() {
    let bus = bus_with_gdma_enabled_and_key_requested();
    assert!(bus.pending_irq().is_none());
}

#[test]
fn pending_irq_is_some_when_enabled_source_is_requested() {
    let mut bus = bus_with_gdma_enabled_and_key_requested();
    bus.request_irq(IrqSource::GdmaComplete);
    assert!(bus.pending_irq().is_some());
}

#[test]
fn int_base_offsets_vector_number() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Set INT_BASE to 8 via raw port write (port 0xB0)
    bus.write_io(0xB0, 8);
    bus.write_io(0xB2, 1 << IrqSource::VBlank as u8);
    bus.on_vblank();
    // VBlank is bit 6 → vector = 8 + 6 = 14
    let vector = bus.pending_irq().unwrap();
    assert_eq!(vector, 14);
}

#[test]
fn highest_priority_bit_wins_when_multiple_pending() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Enable all interrupts
    bus.write_io(0xB2, 0xFF);
    // Request both VBLANK (6) and HBLANK_TIMER (7) simultaneously
    bus.request_irq(IrqSource::VBlank);
    bus.request_irq(IrqSource::HBlankTimer);
    // HBlankTimer (bit 7) has higher priority
    let vector = bus.pending_irq().unwrap();
    assert_eq!(vector, IrqSource::HBlankTimer as u8); // INT_BASE=0, priority=7 → vector 7
}

// ── Timer ────────────────────────────────────────────────────────────────────

fn bus_with_hblank_timer_period_3() -> Bus {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB2, 0xFF);
    // Enable HBlank timer (bit 0) without auto-reload (bit 1 = 0)
    bus.write_io(0xA2, 0x01);
    // Set period to 3 HBlanks (writing resets counter too)
    bus.write_io(0xA4, 3);
    bus.write_io(0xA5, 0);
    bus
}

fn bus_after_hblank_1_of_period_3() -> Bus {
    let mut bus = bus_with_hblank_timer_period_3();
    bus.on_hblank(); // counter: 3 → 2
    bus
}

fn bus_after_hblank_2_of_period_3() -> Bus {
    let mut bus = bus_with_hblank_timer_period_3();
    bus.on_hblank(); // counter: 3 → 2
    bus.on_hblank(); // counter: 2 → 1
    bus
}

fn bus_after_hblank_3_of_period_3() -> Bus {
    let mut bus = bus_with_hblank_timer_period_3();
    bus.on_hblank(); // counter: 3 → 2
    bus.on_hblank(); // counter: 2 → 1
    bus.on_hblank(); // counter: 1 → 0 → fires HBlankTimer IRQ
    bus
}

#[test]
fn hblank_timer_is_not_pending_after_first_hblank() {
    let bus = bus_after_hblank_1_of_period_3();
    assert!(bus.pending_irq().is_none());
}

#[test]
fn hblank_timer_is_not_pending_after_second_hblank() {
    let bus = bus_after_hblank_2_of_period_3();
    assert!(bus.pending_irq().is_none());
}

#[test]
fn hblank_timer_fires_after_period_hblanks() {
    let bus = bus_after_hblank_3_of_period_3();
    assert!(bus.pending_irq().is_some());
}

#[test]
fn hblank_timer_counter_one_latches_even_when_timer_disabled() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB2, 1 << IrqSource::HBlankTimer as u8);
    bus.write_io(0xA2, 0x00);
    bus.write_io(0xA4, 1);
    bus.write_io(0xA5, 0);
    bus.on_hblank();
    assert_eq!(bus.pending_irq(), Some(IrqSource::HBlankTimer as u8));
    assert_eq!(bus.read_io(0xA8), 0);
    assert_eq!(bus.read_io(0xA9), 0);
}

#[test]
fn hblank_timer_counter_one_waits_when_timer_and_irq_are_disabled() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB2, 0);
    bus.write_io(0xA2, 0x00);
    bus.write_io(0xA4, 1);
    bus.write_io(0xA5, 0);
    bus.on_hblank();
    assert!(bus.pending_irq().is_none());
    assert_eq!(bus.read_io(0xA8), 1);
    assert_eq!(bus.read_io(0xA9), 0);
}

#[test]
fn hblank_timer_above_one_does_not_run_when_disabled() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB2, 1 << IrqSource::HBlankTimer as u8);
    bus.write_io(0xA2, 0x00);
    bus.write_io(0xA4, 2);
    bus.write_io(0xA5, 0);
    bus.on_hblank();
    assert!(bus.pending_irq().is_none());
    assert_eq!(bus.read_io(0xA8), 2);
    assert_eq!(bus.read_io(0xA9), 0);
}

#[test]
fn hblank_timer_irq_source_matches_hblank_timer() {
    let bus = bus_after_hblank_3_of_period_3();
    assert_eq!(bus.pending_irq().unwrap(), IrqSource::HBlankTimer as u8);
}

fn bus_with_auto_reload_hblank_timer() -> Bus {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB2, 0xFF);
    // HBlank timer with auto-reload (bits 0 and 1)
    bus.write_io(0xA2, 0x03);
    bus.write_io(0xA4, 2);
    bus.write_io(0xA5, 0);
    bus
}

fn bus_after_auto_reload_first_fire() -> Bus {
    let mut bus = bus_with_auto_reload_hblank_timer();
    bus.on_hblank(); // 2 → 1
    bus.on_hblank(); // 1 → 0 → fires; reloads to 2
    bus
}

fn bus_after_auto_reload_first_fire_cleared() -> Bus {
    let mut bus = bus_after_auto_reload_first_fire();
    bus.write_io(0xB6, 1 << IrqSource::HBlankTimer as u8);
    bus
}

#[test]
fn hblank_timer_fires_on_first_period_with_auto_reload() {
    let bus = bus_after_auto_reload_first_fire();
    assert!(bus.pending_irq().is_some());
}

#[test]
fn hblank_timer_irq_clears_after_cause_clear_write() {
    let bus = bus_after_auto_reload_first_fire_cleared();
    assert!(bus.pending_irq().is_none());
}

#[test]
fn hblank_timer_fires_again_after_auto_reload() {
    let mut bus = bus_after_auto_reload_first_fire_cleared();
    bus.on_hblank(); // 2 → 1
    bus.on_hblank(); // 1 → 0 → fires again
    assert!(bus.pending_irq().is_some());
}

fn bus_with_vblank_timer_period_2() -> Bus {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB2, 0xFF);
    // Enable VBlank timer (bit 2) without auto-reload
    bus.write_io(0xA2, 0x04);
    bus.write_io(0xA6, 2);
    bus.write_io(0xA7, 0);
    bus
}

fn bus_after_first_vblank_timer_cleared() -> Bus {
    let mut bus = bus_with_vblank_timer_period_2();
    bus.on_vblank(); // counter: 2 → 1; also fires VBlank IRQ
    bus.write_io(0xB6, 1 << IrqSource::VBlank as u8); // clear VBlank to isolate timer test
    bus
}

#[test]
fn vblank_timer_is_not_pending_before_counter_reaches_zero() {
    let bus = bus_after_first_vblank_timer_cleared();
    assert!(bus.pending_irq().is_none());
}

#[test]
fn vblank_timer_appears_in_cause_register_when_counter_reaches_zero() {
    let mut bus = bus_after_first_vblank_timer_cleared();
    bus.on_vblank(); // counter: 1 → 0 → fires VBlankTimer
    let cause = bus.read_io(0xB4);
    assert_ne!(cause & (1 << IrqSource::VBlankTimer as u8), 0);
}

// ── GDMA ─────────────────────────────────────────────────────────────────────

fn setup_gdma_rom_to_wram() -> Bus {
    let mut rom = vec![0u8; 0x10000];
    rom[0xFFF0] = 0xAA;
    rom[0xFFF1] = 0xBB;
    rom[0xFFF2] = 0xCC;
    rom[0xFFF3] = 0xDD;
    let mut bus = color_video_bus(rom);
    bus.write_io(0xB2, 0xFF);
    bus.write_io(0x40, 0xF0); // src offset low (bit 0 forced 0 → 0xF0)
    bus.write_io(0x41, 0xFF); // src offset high
    bus.write_io(0x42, 0x0F); // src segment
    bus.write_io(0x44, 0x10); // dst offset low
    bus.write_io(0x45, 0x00); // dst offset high
    bus.write_io(0x46, 4); // length low
    bus.write_io(0x47, 0); // length high
    bus.write_io(0x48, 0x80); // arm GDMA via public I/O write
    bus.tick_gdma();
    bus
}

#[test]
fn gdma_copies_byte_0_from_rom_to_wram() {
    let bus = setup_gdma_rom_to_wram();
    assert_eq!(bus.read_u8(0x0010), 0xAA);
}

#[test]
fn gdma_copies_byte_1_from_rom_to_wram() {
    let bus = setup_gdma_rom_to_wram();
    assert_eq!(bus.read_u8(0x0011), 0xBB);
}

#[test]
fn gdma_copies_byte_2_from_rom_to_wram() {
    let bus = setup_gdma_rom_to_wram();
    assert_eq!(bus.read_u8(0x0012), 0xCC);
}

#[test]
fn gdma_copies_byte_3_from_rom_to_wram() {
    let bus = setup_gdma_rom_to_wram();
    assert_eq!(bus.read_u8(0x0013), 0xDD);
}

#[test]
fn gdma_sets_complete_irq_after_transfer() {
    let mut bus = setup_gdma_rom_to_wram();
    let cause = bus.read_io(0xB4);
    assert_ne!(cause & (1 << IrqSource::GdmaComplete as u8), 0);
}

#[test]
fn gdma_returns_zero_cycles_without_enable_bit() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.wram[0x10] = 0xAB;
    // Do NOT set the enable bit in port 0x48
    let cycles = bus.tick_gdma();
    assert_eq!(cycles, 0);
}

#[test]
fn gdma_leaves_destination_unchanged_without_enable_bit() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.wram[0x10] = 0xAB;
    bus.tick_gdma();
    assert_eq!(bus.wram[0x10], 0xAB);
}

#[test]
fn gdma_from_slow_rom_aborts_without_consuming_length() {
    let mut rom = vec![0u8; 0x10000];
    rom[0] = 0x5A;
    let mut bus = color_video_bus(rom);
    bus.write_io(0xA0, 0x08); // SYSTEM_CTRL1_ROM_WAIT
    bus.write_io(0x42, 0x08); // ROM source at 0x80000
    bus.write_io(0x44, 0x00);
    bus.write_io(0x46, 2);

    bus.write_io(0x48, 0x80);

    assert_eq!(bus.read_io(0x46), 2);
    assert_eq!(bus.read_u8(0x0000), 0x00);
}

#[test]
fn gdma_from_upper_linear_rom_transfers_even_when_rom_wait_is_set() {
    let mut rom = vec![0u8; 0x10000];
    rom[0] = 0x5A;
    let mut bus = color_video_bus(rom);
    bus.write_io(0xA0, 0x08); // SYSTEM_CTRL1_ROM_WAIT
    bus.write_io(0x42, 0x0F); // upper linear ROM source at 0xF0000
    bus.write_io(0x44, 0x00);
    bus.write_io(0x46, 2);

    bus.write_io(0x48, 0x80);

    assert_eq!(bus.read_io(0x46), 0);
    assert_eq!(bus.read_u8(0x0000), 0x5A);
}

#[test]
fn gdma_from_fast_rom_still_transfers_when_rom_wait_is_clear() {
    let mut rom = vec![0u8; 0x10000];
    rom[0] = 0x5A;
    let mut bus = color_video_bus(rom);
    bus.write_io(0x42, 0x08); // ROM source at 0x80000
    bus.write_io(0x44, 0x00);
    bus.write_io(0x46, 2);

    bus.write_io(0x48, 0x80);

    assert_eq!(bus.read_io(0x46), 0);
    assert_eq!(bus.read_u8(0x0000), 0x5A);
}

// ── SDMA ─────────────────────────────────────────────────────────────────────

fn color_bus_with_sdma_voice() -> Bus {
    let mut bus = color_video_bus(rom_with_color_flag(true));
    bus.write_io(0x91, 0x80); // headphone path: keep voice tests independent of speaker volume
    bus.write_io(0x90, 0x20); // channel 2 voice mode
    bus.write_io(0x94, 0x05); // full left + full right voice routing
    bus
}

fn arm_sdma(bus: &mut Bus, source: u32, counter: u32, ctrl: u8) {
    let [src_lo, src_hi] = (source as u16).to_le_bytes();
    bus.write_io(0x4A, src_lo);
    bus.write_io(0x4B, src_hi);
    bus.write_io(0x4C, ((source >> 16) & 0x0F) as u8);
    let [count_lo, count_hi] = (counter as u16).to_le_bytes();
    bus.write_io(0x4E, count_lo);
    bus.write_io(0x4F, count_hi);
    bus.write_io(0x50, ((counter >> 16) & 0x0F) as u8);
    bus.write_io(0x52, ctrl);
}

#[test]
fn sdma_segment_registers_keep_low_nibble_only() {
    let mut bus = color_bus_with_sdma_voice();
    bus.write_io(0x4C, 0xF7);
    bus.write_io(0x50, 0xE3);
    assert_eq!(bus.read_io(0x4C), 0x07);
    assert_eq!(bus.read_io(0x50), 0x03);
}

#[test]
fn sdma_ignored_when_disabled() {
    let mut bus = color_bus_with_sdma_voice();
    bus.write_u8(0x0010, 0xC8);
    arm_sdma(&mut bus, 0x0010, 1, 0x03);
    bus.tick_apu(128);
    assert_eq!(bus.read_io(0x89), 0x00);
    assert_eq!(bus.read_io(0x4E), 0x01);
}

#[test]
fn sdma_ignored_on_mono_hardware() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x90, 0x20);
    bus.write_u8(0x0010, 0xC8);
    arm_sdma(&mut bus, 0x0010, 1, 0x83);
    bus.tick_apu(128);
    assert_eq!(bus.read_io(0x89), 0x00);
    assert_eq!(bus.read_io(0x52) & 0x80, 0x00);
}

#[test]
fn sdma_copies_memory_byte_to_voice_latch() {
    let mut bus = color_bus_with_sdma_voice();
    bus.write_u8(0x0010, 0xC8);
    arm_sdma(&mut bus, 0x0010, 1, 0x83);
    bus.tick_apu(128);
    assert_eq!(bus.read_io(0x89), 0xC8);
}

#[test]
fn sdma_rate_bits_select_apu_sample_divider() {
    for (rate, divider) in [(0u8, 6u32), (1, 4), (2, 2), (3, 1)] {
        let mut bus = color_bus_with_sdma_voice();
        bus.write_u8(0x0010, 0x80 | rate);
        arm_sdma(&mut bus, 0x0010, 1, 0x80 | rate);

        bus.tick_apu(128 * divider - 1);
        assert_eq!(bus.read_io(0x89), 0x00, "rate {rate} transferred early");

        bus.tick_apu(1);
        assert_eq!(
            bus.read_io(0x89),
            0x80 | rate,
            "rate {rate} did not transfer after {divider} APU sample ticks"
        );
    }
}

#[test]
fn sdma_voice_transfer_produces_non_silent_samples() {
    let mut bus = color_bus_with_sdma_voice();
    bus.write_u8(0x0010, 0xC8);
    arm_sdma(&mut bus, 0x0010, 1, 0x83);
    bus.tick_apu(128);
    assert!(bus.audio_samples().iter().any(|&sample| sample != 0));
}

#[test]
fn sdma_terminal_count_clears_enable_and_writes_back_registers() {
    let mut bus = color_bus_with_sdma_voice();
    bus.write_u8(0x0010, 0xC8);
    arm_sdma(&mut bus, 0x0010, 1, 0x83);
    bus.tick_apu(128);
    assert_eq!(bus.read_io(0x52) & 0x80, 0x00);
    assert_eq!(bus.read_io(0x4A), 0x11);
    assert_eq!(bus.read_io(0x4E), 0x00);
}

#[test]
fn sdma_decrement_direction_updates_source_backwards() {
    let mut bus = color_bus_with_sdma_voice();
    bus.write_u8(0x0010, 0xC8);
    arm_sdma(&mut bus, 0x0010, 1, 0xC3);
    bus.tick_apu(128);
    assert_eq!(bus.read_io(0x4A), 0x0F);
}

#[test]
fn sdma_repeat_reloads_source_and_counter_without_clearing_enable() {
    let mut bus = color_bus_with_sdma_voice();
    bus.write_u8(0x0010, 0xC8);
    arm_sdma(&mut bus, 0x0010, 1, 0x8B);
    bus.tick_apu(128);
    assert_eq!(bus.read_io(0x52) & 0x80, 0x80);
    assert_eq!(bus.read_io(0x4A), 0x10);
    assert_eq!(bus.read_io(0x4E), 0x01);
}

#[test]
fn sdma_hold_outputs_zero_without_advancing_counter() {
    let mut bus = color_bus_with_sdma_voice();
    bus.write_io(0x89, 0xC8);
    arm_sdma(&mut bus, 0x0010, 1, 0x87);
    bus.tick_apu(128);
    assert_eq!(bus.read_io(0x89), 0x00);
    assert_eq!(bus.read_io(0x4E), 0x01);
    assert_eq!(bus.read_io(0x52) & 0x80, 0x80);
}

#[test]
fn gdma_clears_enable_bit_after_transfer() {
    let rom = vec![0xFFu8; 0x10000];
    let mut bus = color_video_bus(rom);
    bus.write_io(0x44, 0x00);
    bus.write_io(0x45, 0x00);
    bus.write_io(0x46, 2);
    bus.write_io(0x47, 0);
    bus.write_io(0x48, 0x80); // arm via public I/O write
    bus.tick_gdma();
    // ctrl should be cleared after completion
    assert_eq!(bus.read_io(0x48), 0x00);
}

/// Regression: arming GDMA via port 0x48 must transfer synchronously, so two
/// arms in quick succession (as a game updating several tilemap regions per
/// frame does) both land. Before GDMA ran on the port write, execution was
/// deferred to a single per-scanline tick and every transfer but the last was
/// silently dropped — the root cause of the Lode Runner tilemap corruption.
#[test]
fn back_to_back_gdma_arms_both_complete() {
    let mut rom = vec![0u8; 0x10000];
    rom[0xFFF0] = 0x11;
    rom[0xFFF1] = 0x22;
    let mut bus = color_video_bus(rom);
    bus.write_io(0x42, 0x0F); // src segment 0xF (→ ROM at 0xFxxxx)

    // First transfer: 2 bytes from 0xFFFF0 → WRAM 0x0010, armed with no
    // intervening tick_gdma() call.
    bus.write_io(0x40, 0xF0);
    bus.write_io(0x41, 0xFF);
    bus.write_io(0x44, 0x10);
    bus.write_io(0x45, 0x00);
    bus.write_io(0x46, 2);
    bus.write_io(0x48, 0x80);

    // Second transfer: same source to a different destination 0x0030.
    bus.write_io(0x40, 0xF0);
    bus.write_io(0x41, 0xFF);
    bus.write_io(0x44, 0x30);
    bus.write_io(0x45, 0x00);
    bus.write_io(0x46, 2);
    bus.write_io(0x48, 0x80);

    // Both destinations received the bytes; the first was not clobbered.
    assert_eq!([bus.read_u8(0x0010), bus.read_u8(0x0011)], [0x11, 0x22]);
    assert_eq!([bus.read_u8(0x0030), bus.read_u8(0x0031)], [0x11, 0x22]);
}

// ── PPU integration ──────────────────────────────────────────────────────────

/// A bus with SCR1 enabled, identity palette, and tile 0 drawing pixel 1 at
/// its top-left corner (map entry (0,0) defaults to tile 0).
fn setup_bus_scr1_pixel() -> Bus {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x00, 0x01); // SCR1 enable
    bus.write_io(0x07, 0x00); // SCR1 map base 0
    bus.write_io(0x20, 0x10); // palette 0: pixel1 → pool1
    bus.write_io(0x1C, 0x10); // pool1 → shade 1
    bus.wram[0x2000] = 0b1000_0000; // tile 0 row 0 plane 0: x0 set
    bus
}

/// The RGB444 framebuffer value a monochrome `shade` (0–15) resolves to (the
/// mono resolver inverts brightness: shade 0 = white = 0x0FFF).
fn grey(shade: u8) -> u16 {
    let n = (15 - (shade & 0x0F)) as u16;
    (n << 8) | (n << 4) | n
}

#[test]
fn render_scanline_draws_scr1_pixel_to_framebuffer() {
    let mut bus = setup_bus_scr1_pixel();
    bus.render_scanline(0);
    assert_eq!(bus.framebuffer()[0], grey(1));
}

#[test]
fn render_scanline_leaves_unset_pixel_clear() {
    let mut bus = setup_bus_scr1_pixel();
    bus.render_scanline(0);
    assert_eq!(bus.framebuffer()[1], grey(0));
}

#[test]
fn render_scanline_updates_lcd_line_register() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.render_scanline(5);
    assert_eq!(bus.read_io(0x02), 5);
}

#[test]
fn render_scanline_at_compare_line_raises_scanline_match_irq() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB2, 0xFF); // enable all interrupts
    bus.write_io(0x03, 10); // LCD line compare = 10
    bus.render_scanline(10);
    let cause = bus.read_io(0xB4);
    assert_ne!(cause & (1 << IrqSource::ScanlineMatch as u8), 0);
}

#[test]
fn framebuffer_has_full_screen_length() {
    let bus = Bus::new(vec![0u8; 0x10000]);
    assert_eq!(bus.framebuffer().len(), 224 * 144);
}

// ── CPU INT / IRET / IN / OUT (integration tests via Bus) ───────────────────

fn setup_int_instruction() -> (Cpu, Bus) {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // IVT: INT 0x10 → CS:IP = 0x0000:0x0200
    bus.wram[0x40] = 0x00; // IP low
    bus.wram[0x41] = 0x02; // IP high  → IP = 0x0200
    bus.wram[0x42] = 0x00; // CS low
    bus.wram[0x43] = 0x00; // CS high  → CS = 0x0000
    bus.wram[0x0100] = 0xCD; // INT 0x10
    bus.wram[0x0101] = 0x10;
    let mut cpu = Cpu::new();
    cpu.reset(0x0000, 0x0100);
    cpu.step(&mut bus);
    (cpu, bus)
}

#[test]
fn int_instruction_sets_ip_from_ivt() {
    let (cpu, _) = setup_int_instruction();
    assert_eq!(cpu.regs.ip, 0x0200);
}

#[test]
fn int_instruction_sets_cs_from_ivt() {
    let (cpu, _) = setup_int_instruction();
    assert_eq!(cpu.regs.cs, 0x0000);
}

#[test]
fn int_instruction_clears_if_flag() {
    let (cpu, _) = setup_int_instruction();
    assert!(!cpu.flags.interrupt);
}

fn setup_iret() -> (Cpu, Bus) {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // IVT: INT 5 → 0x0000:0x0300
    let ivt_addr = 5 * 4;
    bus.wram[ivt_addr] = 0x00;
    bus.wram[ivt_addr + 1] = 0x03; // IP = 0x0300
    bus.wram[ivt_addr + 2] = 0x00;
    bus.wram[ivt_addr + 3] = 0x00; // CS = 0x0000
    bus.wram[0x0300] = 0xCF; // IRET handler
    bus.wram[0x0100] = 0xCD; // INT 5
    bus.wram[0x0101] = 0x05;
    let mut cpu = Cpu::new();
    cpu.reset(0x0000, 0x0100);
    // Stack lives in WRAM; prime SP near top of first 16 KiB.
    cpu.regs.sp = 0x3FFE;
    cpu.flags.interrupt = true;
    cpu.step(&mut bus); // INT 5: jumps to 0x0300, saves IP=0x0102
    cpu.step(&mut bus); // IRET: pops IP, CS, FLAGS
    (cpu, bus)
}

#[test]
fn iret_restores_ip_to_next_instruction() {
    let (cpu, _) = setup_iret();
    assert_eq!(cpu.regs.ip, 0x0102);
}

#[test]
fn iret_restores_cs() {
    let (cpu, _) = setup_iret();
    assert_eq!(cpu.regs.cs, 0x0000);
}

#[test]
fn iret_restores_interrupt_flag() {
    let (cpu, _) = setup_iret();
    assert!(cpu.flags.interrupt);
}

#[test]
fn into_does_not_trigger_when_overflow_clear() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.wram[0x0100] = 0xCE; // INTO
    let mut cpu = Cpu::new();
    cpu.reset(0x0000, 0x0100);
    cpu.flags.overflow = false;
    let ip_before = cpu.regs.ip;
    cpu.step(&mut bus);
    // No interrupt taken: IP advances by 1
    assert_eq!(cpu.regs.ip, ip_before + 1);
}

#[test]
fn in_out_imm_port_reads_and_writes_io() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Port 0x10 is a plain read/write display window coordinate.
    // Code: OUT 0x10, AL  (0xE6 0x10); IN AL, 0x10  (0xE4 0x10)
    bus.wram[0x0100] = 0xE6;
    bus.wram[0x0101] = 0x10;
    bus.wram[0x0102] = 0xE4;
    bus.wram[0x0103] = 0x10;

    let mut cpu = Cpu::new();
    cpu.reset(0x0000, 0x0100);
    cpu.regs.set_reg8(0, 0x55); // AL = 0x55
    cpu.step(&mut bus); // OUT 0x10, AL
    cpu.regs.set_reg8(0, 0x00); // clear AL
    cpu.step(&mut bus); // IN AL, 0x10
    assert_eq!(cpu.regs.get_reg8(0), 0x55);
}

#[test]
fn in_out_dx_uses_dx_as_port_number() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Code: OUT DX, AL  (0xEE); IN AL, DX  (0xEC)
    bus.wram[0x0100] = 0xEE;
    bus.wram[0x0101] = 0xEC;

    let mut cpu = Cpu::new();
    cpu.reset(0x0000, 0x0100);
    cpu.regs.dx = 0x0010; // port 0x10
    cpu.regs.set_reg8(0, 0x77); // AL = 0x77
    cpu.step(&mut bus); // OUT DX, AL
    cpu.regs.set_reg8(0, 0x00);
    cpu.step(&mut bus); // IN AL, DX
    assert_eq!(cpu.regs.get_reg8(0), 0x77);
}

fn setup_handle_irq() -> (Cpu, Bus) {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // IVT: vector 6 (VBlank) → 0x0000:0x0400
    bus.wram[6 * 4] = 0x00;
    bus.wram[6 * 4 + 1] = 0x04; // IP = 0x0400
    bus.wram[6 * 4 + 2] = 0x00;
    bus.wram[6 * 4 + 3] = 0x00; // CS = 0x0000
    let mut cpu = Cpu::new();
    cpu.reset(0x0000, 0x0200);
    cpu.flags.interrupt = true;
    bus.write_io(0xB2, 1 << IrqSource::VBlank as u8);
    bus.on_vblank();
    let vector = bus.pending_irq().unwrap();
    cpu.handle_irq(&mut bus, vector);
    (cpu, bus)
}

#[test]
fn handle_irq_sets_ip_from_ivt() {
    let (cpu, _) = setup_handle_irq();
    assert_eq!(cpu.regs.ip, 0x0400);
}

#[test]
fn handle_irq_clears_interrupt_flag() {
    let (cpu, _) = setup_handle_irq();
    assert!(!cpu.flags.interrupt);
}

#[test]
fn handle_irq_clears_halted_state() {
    let (cpu, _) = setup_handle_irq();
    assert!(!cpu.halted);
}

// ── Color-mode resolver selection (8b) ───────────────────────────────────────

/// A bus with SCR1 enabled and tile 0 drawing pixel 1 (palette 0) at its
/// top-left corner, an identity monochrome palette, and a color palette-RAM
/// entry (palette 0, color 1) set to `color_ram`. The caller sets the model and
/// port 0x60 to select the render path.
fn setup_bus_scr1_pixel_dual(color_ram: u16) -> Bus {
    let mut bus = setup_bus_scr1_pixel();
    let addr = 0xFE00 + 2; // palette 0, color 1 → entry 1 → byte offset 2
    let [lo, hi] = (color_ram & 0x0FFF).to_le_bytes();
    bus.wram[addr] = lo;
    bus.wram[addr + 1] = hi;
    bus
}

#[test]
fn color_model_with_color_bit_renders_from_palette_ram() {
    let mut bus = setup_bus_scr1_pixel_dual(0x0F0F);
    bus.set_model(HardwareModel::Color);
    bus.write_io(0x60, 0x80); // color-mode bit set
    bus.render_scanline(0);
    assert_eq!(bus.framebuffer()[0], 0x0F0F);
}

#[test]
fn color_model_without_color_bit_uses_mono_path() {
    let mut bus = setup_bus_scr1_pixel_dual(0x0F0F);
    bus.set_model(HardwareModel::Color);
    bus.write_io(0x60, 0x00); // color-mode bit clear → mono-compat
    bus.render_scanline(0);
    assert_eq!(bus.framebuffer()[0], grey(1));
}

#[test]
fn mono_model_ignores_color_bit() {
    let mut bus = setup_bus_scr1_pixel_dual(0x0F0F);
    bus.write_io(0x60, 0x80); // bit set, but model is Mono
    bus.render_scanline(0);
    assert_eq!(bus.framebuffer()[0], grey(1));
}

// ── Cartridge RTC (ports 0xCA/0xCB) ──────────────────────────────────────────

/// 16-byte ROM whose footer flags byte (offset 0xC) sets bit 2 = RTC present.
fn rom_with_rtc() -> Vec<u8> {
    let mut rom = vec![0u8; 16];
    rom[0xC] = 0x04;
    rom
}

#[test]
fn cart_without_rtc_reads_open_bus_on_command_port() {
    let mut bus = Bus::from_rom(rom_with_color_flag(true));
    assert!(!bus.has_rtc());
    bus.write_io(0xCA, 0x14);
    assert_eq!(bus.read_io(0xCA), 0x90);
}

#[test]
fn cart_without_rtc_reads_open_bus_on_data_port() {
    let mut bus = Bus::from_rom(rom_with_color_flag(true));
    bus.write_io(0xCB, 0x56);
    assert_eq!(bus.read_io(0xCB), 0x90);
}

#[test]
fn rtc_footer_bit_creates_clock() {
    let bus = Bus::from_rom(rom_with_rtc());
    assert!(bus.has_rtc());
}

#[test]
fn rtc_datetime_read_over_ports_returns_injected_time() {
    let mut bus = Bus::from_rom(rom_with_rtc());
    bus.set_rtc_datetime(26, 7, 3, 5, 12, 34, 56);
    // Command port: select "read date/time"; status reports ready and busy
    // until the seven-byte payload has been consumed.
    bus.write_io(0xCA, 0x14);
    assert_eq!(bus.read_io(0xCA), 0x90);
    let got: Vec<u8> = (0..7).map(|_| bus.read_io(0xCB)).collect();
    assert_eq!(got, vec![0x26, 0x07, 0x03, 5, 0x12, 0x34, 0x56]);
}

#[test]
fn rtc_datetime_write_over_ports_round_trips() {
    let mut bus = Bus::from_rom(rom_with_rtc());
    bus.write_io(0xCA, 0x15); // write date/time
    for b in [0x26, 0x07, 0x03, 0x05, 0x12, 0x34, 0x56] {
        bus.write_io(0xCB, b);
    }
    bus.write_io(0xCA, 0x14); // read date/time
    let got: Vec<u8> = (0..7).map(|_| bus.read_io(0xCB)).collect();
    assert_eq!(got, vec![0x26, 0x07, 0x03, 0x05, 0x12, 0x34, 0x56]);
}

#[test]
fn tick_rtc_advances_seconds() {
    let mut bus = Bus::from_rom(rom_with_rtc());
    bus.set_rtc_datetime(26, 7, 3, 5, 12, 0, 0);
    bus.tick_rtc(crate::system::MASTER_CLOCK_HZ); // one wall-second
    bus.write_io(0xCA, 0x14);
    let got: Vec<u8> = (0..7).map(|_| bus.read_io(0xCB)).collect();
    assert_eq!(got[6], 0x01); // seconds register advanced by one
}

#[test]
fn tick_rtc_without_clock_is_noop() {
    let mut bus = Bus::from_rom(rom_with_color_flag(true));
    bus.tick_rtc(crate::system::MASTER_CLOCK_HZ);
    assert!(!bus.has_rtc());
}
