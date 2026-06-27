use super::{Bus, IrqSource};
use crate::cpu::{Cpu, MemoryBus};

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
    // Port 0x00 (DISP_CTRL) – no special handling, raw R/W
    bus.write_io(0x00, 0x42);
    assert_eq!(bus.read_io(0x00), 0x42);
}

#[test]
fn int_enable_vblank_bit_is_always_set() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Writing with bit 6 clear should still return bit 6 set on read
    bus.write_io(0xB2, 0b0000_0001);
    assert_eq!(bus.read_io(0xB2) & (1 << 6), 1 << 6);
}

fn bus_with_vblank_and_hblank_requested_then_vblank_cleared() -> Bus {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
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

fn setup_gdma_ctrl_read() -> (u8, Bus) {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x48, 0xC0);
    let first = bus.read_io(0x48);
    (first, bus)
}

#[test]
fn gdma_ctrl_first_read_returns_written_value() {
    let (first, _) = setup_gdma_ctrl_read();
    assert_eq!(first, 0xC0);
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
    // VBlank is enabled but not yet pending
    assert!(bus.pending_irq().is_none());
}

fn bus_after_vblank() -> Bus {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
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
    let mut bus = Bus::new(rom);
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
fn gdma_clears_enable_bit_after_transfer() {
    let rom = vec![0xFFu8; 0x10000];
    let mut bus = Bus::new(rom);
    bus.write_io(0x44, 0x00);
    bus.write_io(0x45, 0x00);
    bus.write_io(0x46, 2);
    bus.write_io(0x47, 0);
    bus.write_io(0x48, 0x80); // arm via public I/O write
    bus.tick_gdma();
    // ctrl should be cleared after completion
    assert_eq!(bus.read_io(0x48), 0x00);
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
    // Code: OUT 0xA0, AL  (0xE6 0xA0); IN AL, 0xA0  (0xE4 0xA0)
    bus.wram[0x0100] = 0xE6;
    bus.wram[0x0101] = 0xA0;
    bus.wram[0x0102] = 0xE4;
    bus.wram[0x0103] = 0xA0;

    let mut cpu = Cpu::new();
    cpu.reset(0x0000, 0x0100);
    cpu.regs.set_reg8(0, 0x55); // AL = 0x55
    cpu.step(&mut bus); // OUT 0xA0, AL
    cpu.regs.set_reg8(0, 0x00); // clear AL
    cpu.step(&mut bus); // IN AL, 0xA0
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
    cpu.regs.dx = 0x00A1; // port 0xA1
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
