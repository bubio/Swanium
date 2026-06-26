use super::{Bus, IrqSource};
use crate::cpu::{Cpu, MemoryBus};

// ── Memory map ───────────────────────────────────────────────────────────────

#[test]
fn test_wram_read_write() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_u8(0x0000, 0xAB);
    bus.write_u8(0x03FF, 0xCD);
    bus.write_u8(0x3FFF, 0xEF);
    assert_eq!(bus.read_u8(0x0000), 0xAB);
    assert_eq!(bus.read_u8(0x03FF), 0xCD);
    assert_eq!(bus.read_u8(0x3FFF), 0xEF);
}

#[test]
fn test_wram_word_roundtrip() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_u16(0x0100, 0xBEEF);
    assert_eq!(bus.read_u16(0x0100), 0xBEEF);
    // little-endian layout
    assert_eq!(bus.read_u8(0x0100), 0xEF);
    assert_eq!(bus.read_u8(0x0101), 0xBE);
}

#[test]
fn test_open_bus_mono() {
    let bus = Bus::new(vec![0u8; 0x10000]);
    // 0x04000-0x0FFFF is open bus on mono
    assert_eq!(bus.read_u8(0x04000), 0x90);
    assert_eq!(bus.read_u8(0x0FFFF), 0x90);
}

#[test]
fn test_rom_ex_boot_vector() {
    // Last 16 bytes of a 64 KiB ROM should be visible at 0xFFFF0-0xFFFFF
    // when linear_off = 0xFF (power-on default).
    let mut rom = vec![0u8; 0x10000];
    rom[0xFFF0] = 0x55; // marker at last 16 bytes
    rom[0xFFF1] = 0xAA;
    let bus = Bus::new(rom);
    assert_eq!(bus.read_u8(0xFFFF0), 0x55);
    assert_eq!(bus.read_u8(0xFFFF1), 0xAA);
}

#[test]
fn test_sram_read_write() {
    let rom = vec![0u8; 0x10000];
    let sram = vec![0u8; 0x10000];
    let mut bus = Bus::with_sram(rom, sram);
    // ram_bank defaults to 0xFF; effective SRAM index = (0xFF << 16 | offset) % len
    bus.write_u8(0x10000, 0x77);
    assert_eq!(bus.read_u8(0x10000), 0x77);
}

#[test]
fn test_rom_bank_switch() {
    // ROM bank 0 (I/O port 0xC2) controls what appears at 0x20000-0x2FFFF.
    let mut rom = vec![0u8; 0x30000]; // 192 KiB ROM
                                      // Bank 0 (bank_reg=0) → offset 0x00000-0x0FFFF
    rom[0x0000] = 0x11;
    // Bank 1 (bank_reg=1) → offset 0x10000-0x1FFFF
    rom[0x10000] = 0x22;
    // Bank 2 (bank_reg=2) → offset 0x20000-0x2FFFF
    rom[0x20000] = 0x33;

    let mut bus = Bus::new(rom);

    // Point ROM bank 0 register at bank 0
    bus.write_io(0xC2, 0x00);
    assert_eq!(bus.read_u8(0x20000), 0x11);

    // Point ROM bank 0 register at bank 1
    bus.write_io(0xC2, 0x01);
    assert_eq!(bus.read_u8(0x20000), 0x22);

    // Point ROM bank 0 register at bank 2
    bus.write_io(0xC2, 0x02);
    assert_eq!(bus.read_u8(0x20000), 0x33);
}

#[test]
fn test_rom_write_ignored() {
    // Writes to ROM areas should be silently ignored.
    let mut bus = Bus::new(vec![0xAA; 0x10000]);
    bus.write_u8(0xFFFF0, 0x00);
    assert_eq!(bus.read_u8(0xFFFF0), 0xAA);
}

// ── I/O port basics ──────────────────────────────────────────────────────────

#[test]
fn test_io_port_raw_write_read() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Port 0x00 (DISP_CTRL) – no special handling, raw R/W
    bus.write_io(0x00, 0x42);
    assert_eq!(bus.read_io(0x00), 0x42);
}

#[test]
fn test_io_int_enable_vblank_always_set() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Writing with bit 6 clear should still return bit 6 set on read
    bus.write_io(0xB2, 0b0000_0001);
    assert_eq!(bus.read_io(0xB2) & (1 << 6), 1 << 6);
}

#[test]
fn test_io_int_cause_clear() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.request_irq(IrqSource::VBlank);
    bus.request_irq(IrqSource::HBlankTimer);
    // INT_CAUSE_CLEAR: writing 1 clears the corresponding bits
    bus.write_io(0xB6, 1 << IrqSource::VBlank as u8);
    // VBlank bit cleared; HBlankTimer still set
    let cause = bus.read_io(0xB4);
    assert_eq!(cause & (1 << IrqSource::VBlank as u8), 0);
    assert_eq!(cause & (1 << IrqSource::HBlankTimer as u8), 1 << 7);
}

#[test]
fn test_io_gdma_ctrl_self_clears_on_read() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Write 0xC0 (enable + direction bits) directly via shadow
    bus.write_io(0x48, 0xC0);
    // First read: returns C0, then clears
    let first = bus.read_io(0x48);
    let second = bus.read_io(0x48);
    assert_eq!(first, 0xC0);
    assert_eq!(second, 0x00);
}

// ── Interrupt controller ─────────────────────────────────────────────────────

#[test]
fn test_pending_irq_none_when_disabled() {
    let bus = Bus::new(vec![0u8; 0x10000]);
    // No pending IRQs at startup
    assert!(bus.pending_irq().is_none());
}

#[test]
fn test_pending_irq_vblank_not_pending_until_event() {
    let bus = Bus::new(vec![0u8; 0x10000]);
    // VBlank is enabled but not yet pending
    assert!(bus.pending_irq().is_none());
}

#[test]
fn test_pending_irq_fires_after_vblank_event() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.on_vblank();
    // VBlank (bit 6) should now be pending
    assert!(bus.pending_irq().is_some());
    let vector = bus.pending_irq().unwrap();
    // With INT_BASE = 0, vector = 0 + 6 = 6
    assert_eq!(vector, IrqSource::VBlank as u8);
}

#[test]
fn test_pending_irq_respects_int_enable() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Enable only GDMA (bit 3)
    bus.write_io(0xB2, 1 << IrqSource::GdmaComplete as u8);
    bus.request_irq(IrqSource::KeyPress); // bit 1, not enabled
                                          // GDMA not yet pending; KEY not enabled → no IRQ
    assert!(bus.pending_irq().is_none());
    bus.request_irq(IrqSource::GdmaComplete);
    assert!(bus.pending_irq().is_some());
}

#[test]
fn test_int_base_register() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Set INT_BASE to 8 via raw port write (port 0xB0)
    bus.write_io(0xB0, 8);
    bus.on_vblank();
    // VBlank is bit 6 → vector = 8 + 6 = 14
    let vector = bus.pending_irq().unwrap();
    assert_eq!(vector, 14);
}

#[test]
fn test_priority_highest_bit_wins() {
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

#[test]
fn test_hblank_timer_fires_at_period() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Enable all interrupts
    bus.write_io(0xB2, 0xFF);
    // Enable HBlank timer (bit 0) without auto-reload (bit 1 = 0)
    bus.write_io(0xA2, 0x01);
    // Set period to 3 HBlanks (writing resets counter too)
    bus.write_io(0xA4, 3);
    bus.write_io(0xA5, 0);

    bus.on_hblank(); // counter: 3 → 2
    assert!(bus.pending_irq().is_none());
    bus.on_hblank(); // counter: 2 → 1
    assert!(bus.pending_irq().is_none());
    bus.on_hblank(); // counter: 1 → 0 → fires HBlankTimer IRQ
    assert!(bus.pending_irq().is_some());
    let vector = bus.pending_irq().unwrap();
    assert_eq!(vector, IrqSource::HBlankTimer as u8);
}

#[test]
fn test_hblank_timer_auto_reload() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB2, 0xFF);
    // HBlank timer with auto-reload (bits 0 and 1)
    bus.write_io(0xA2, 0x03);
    bus.write_io(0xA4, 2);
    bus.write_io(0xA5, 0);

    bus.on_hblank(); // 2 → 1
    bus.on_hblank(); // 1 → 0 → fires; reloads to 2
    assert!(bus.pending_irq().is_some());
    // Clear the IRQ via INT_CAUSE_CLEAR
    bus.write_io(0xB6, 1 << IrqSource::HBlankTimer as u8);
    assert!(bus.pending_irq().is_none());

    // Should fire again after another 2 HBlanks
    bus.on_hblank(); // 2 → 1
    bus.on_hblank(); // 1 → 0 → fires again
    assert!(bus.pending_irq().is_some());
}

#[test]
fn test_vblank_timer_fires_at_period() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB2, 0xFF);
    // Enable VBlank timer (bit 2) without auto-reload
    bus.write_io(0xA2, 0x04);
    bus.write_io(0xA6, 2);
    bus.write_io(0xA7, 0);

    bus.on_vblank(); // counter: 2 → 1; also fires VBlank IRQ
                     // Clear VBlank IRQ to isolate the timer test
    bus.write_io(0xB6, 1 << IrqSource::VBlank as u8);
    assert!(bus.pending_irq().is_none());

    bus.on_vblank(); // counter: 1 → 0 → fires VBlankTimer
    let cause = bus.read_io(0xB4);
    assert_ne!(cause & (1 << IrqSource::VBlankTimer as u8), 0);
}

// ── GDMA ─────────────────────────────────────────────────────────────────────

#[test]
fn test_gdma_basic_transfer() {
    // Transfer 4 bytes from ROM linear range to WRAM.
    // ROM: linear_off=0xFF → last 16 bytes of 64 KiB ROM are at 0xFFFF0-0xFFFFF.
    // For simplicity, place source data at ROM offset 0xFFF0 (last 16 bytes).
    let mut rom = vec![0u8; 0x10000];
    rom[0xFFF0] = 0xAA;
    rom[0xFFF1] = 0xBB;
    rom[0xFFF2] = 0xCC;
    rom[0xFFF3] = 0xDD;
    let mut bus = Bus::new(rom);

    // Enable all IRQs so GDMA completion fires
    bus.write_io(0xB2, 0xFF);

    // Source: physical 0xFFFF0 → seg=0xF, offset=0xFFF0
    bus.write_io(0x40, 0xF0); // src offset low (bit 0 forced 0 → 0xF0)
    bus.write_io(0x41, 0xFF); // src offset high
    bus.write_io(0x42, 0x0F); // src segment

    // Destination: WRAM offset 0x0010
    bus.write_io(0x44, 0x10);
    bus.write_io(0x45, 0x00);

    // Length: 4 bytes
    bus.write_io(0x46, 4);
    bus.write_io(0x47, 0);

    // Arm GDMA
    bus.ports[0x48] = 0x80;

    bus.tick_gdma();

    assert_eq!(bus.read_u8(0x0010), 0xAA);
    assert_eq!(bus.read_u8(0x0011), 0xBB);
    assert_eq!(bus.read_u8(0x0012), 0xCC);
    assert_eq!(bus.read_u8(0x0013), 0xDD);
    // GDMA complete IRQ should be set
    let cause = bus.read_io(0xB4);
    assert_ne!(cause & (1 << IrqSource::GdmaComplete as u8), 0);
}

#[test]
fn test_gdma_not_active_without_enable_bit() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.wram[0x10] = 0xAB;
    // Do NOT set the enable bit in port 0x48
    let cycles = bus.tick_gdma();
    assert_eq!(cycles, 0);
    assert_eq!(bus.wram[0x10], 0xAB); // unchanged
}

#[test]
fn test_gdma_clears_enable_after_transfer() {
    let rom = vec![0xFFu8; 0x10000];
    let mut bus = Bus::new(rom);
    bus.write_io(0x44, 0x00);
    bus.write_io(0x45, 0x00);
    bus.write_io(0x46, 2);
    bus.write_io(0x47, 0);
    bus.ports[0x48] = 0x80;
    bus.tick_gdma();
    // ctrl should be cleared after completion
    assert_eq!(bus.read_io(0x48), 0x00);
}

// ── CPU INT / IRET / IN / OUT (integration tests via Bus) ───────────────────

#[test]
fn test_cpu_int_instruction_jumps_to_vector() {
    // Set up IVT: INT 0x10 → CS:IP = 0x0000:0x0200
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Vector 0x10 is at physical 0x40 (= 0x10 * 4)
    bus.wram[0x40] = 0x00; // IP low
    bus.wram[0x41] = 0x02; // IP high  → IP = 0x0200
    bus.wram[0x42] = 0x00; // CS low
    bus.wram[0x43] = 0x00; // CS high  → CS = 0x0000
                           // Code at 0x0100: INT 0x10
    bus.wram[0x0100] = 0xCD;
    bus.wram[0x0101] = 0x10;

    let mut cpu = Cpu::new();
    cpu.reset(0x0000, 0x0100);
    cpu.step(&mut bus);

    assert_eq!(cpu.regs.ip, 0x0200);
    assert_eq!(cpu.regs.cs, 0x0000);
    assert!(!cpu.flags.interrupt); // IF cleared by INT
}

#[test]
fn test_cpu_iret_restores_state() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    // Set up IVT: INT 5 → 0x0000:0x0300
    let ivt_addr = 5 * 4;
    bus.wram[ivt_addr] = 0x00;
    bus.wram[ivt_addr + 1] = 0x03; // IP = 0x0300
    bus.wram[ivt_addr + 2] = 0x00;
    bus.wram[ivt_addr + 3] = 0x00; // CS = 0x0000
                                   // Handler at 0x0300: IRET
    bus.wram[0x0300] = 0xCF;
    // Code at 0x0100: INT 5
    bus.wram[0x0100] = 0xCD;
    bus.wram[0x0101] = 0x05;

    let mut cpu = Cpu::new();
    cpu.reset(0x0000, 0x0100);
    // Stack must live inside WRAM (0x0000–0x03FFF). SP=0 wraps to 0xFFFE
    // which falls in the open-bus region (0x4000–0xFFFF); prime SP at the
    // top of the first 16 KiB instead.
    cpu.regs.sp = 0x3FFE;
    cpu.flags.interrupt = true;
    cpu.step(&mut bus); // INT 5: jumps to 0x0300, saves old IP=0x0102
    assert_eq!(cpu.regs.ip, 0x0300);
    cpu.step(&mut bus); // IRET: pops IP=0x0102, CS=0x0000, FLAGS
    assert_eq!(cpu.regs.ip, 0x0102); // returned past the INT instruction
    assert_eq!(cpu.regs.cs, 0x0000);
    assert!(cpu.flags.interrupt); // IF restored
}

#[test]
fn test_cpu_into_no_overflow() {
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
fn test_cpu_in_out_port() {
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
fn test_cpu_in_out_dx() {
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

#[test]
fn test_cpu_handle_irq_via_bus() {
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

    assert_eq!(cpu.regs.ip, 0x0400);
    assert!(!cpu.flags.interrupt);
    assert!(!cpu.halted);
}
