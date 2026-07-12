//! Integration smoke tests for the frame-boundary driver.
//!
//! These drive [`System`] through the public API exactly as the frontend will:
//! load a ROM, run whole frames, and observe the framebuffer / save data.
//! The intent (per `docs/dev/DevelopmentPlan.md` Phase 7 テスト方法) is a
//! "headlessly runs N frames without crashing" guarantee, plus a check that a
//! tiny program drawn into WRAM actually reaches the framebuffer across frames.

use swanium_core::bus::IrqSource;
use swanium_core::cpu::MemoryBus;
use swanium_core::keypad::KeyState;
use swanium_core::system::{
    System, CYCLES_PER_FRAME, MASTER_CLOCK_HZ, SCANLINES_PER_FRAME, VISIBLE_SCANLINES,
};
use swanium_core::HardwareModel;

/// A 64 KiB ROM whose reset entry (`0xFFFF0`, the last 16 bytes) is `HLT`.
fn halting_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x10000];
    let len = rom.len();
    rom[len - 16] = 0xF4; // HLT
    rom
}

fn rom_with_reset_code(code: &[u8]) -> Vec<u8> {
    assert!(code.len() <= 16);
    let mut rom = vec![0u8; 0x10000];
    let start = rom.len() - 16;
    rom[start..start + code.len()].copy_from_slice(code);
    rom
}

#[test]
fn runs_many_frames_without_panicking() {
    let mut system = System::new(halting_rom());
    for _ in 0..120 {
        system.run_frame(KeyState::NONE);
    }
    assert_eq!(system.framebuffer().len(), 224 * 144);
}

#[test]
fn key_press_sets_keypad_scan_result() {
    let mut system = System::new(halting_rom());
    system.run_frame(KeyState::X1 | KeyState::A);
    // Select the X-pad group (0x20) on the keypad port, then read it back.
    system.bus_mut().write_io(0xB5, 0x20);
    assert_eq!(system.bus_mut().read_io(0xB5), 0x20 | 0x01); // X1 -> bit 0
}

#[test]
fn background_tile_reaches_framebuffer_after_a_frame() {
    let mut system = System::new(halting_rom());
    let bus = system.bus_mut();
    // Identity monochrome palette (tile pixel i -> shade i).
    bus.write_io(0x20, 0x10);
    bus.write_io(0x21, 0x32);
    bus.write_io(0x1C, 0x10);
    bus.write_io(0x1D, 0x32);
    bus.write_io(0x00, 0x01); // enable SCR1
    bus.write_io(0x07, 0x00); // SCR1 map base 0
                              // Tile 0, row 0, leftmost pixel = colour 1.
    bus.write_u8(0x2000, 0b1000_0000);
    bus.write_u8(0x2001, 0b0000_0000);

    system.run_frame(KeyState::NONE);
    // Mono shade 1 as an RGB444 grey (the resolver inverts brightness).
    let grey1 = {
        let n = (15 - 1u16) & 0x0F;
        (n << 8) | (n << 4) | n
    };
    assert_eq!(system.framebuffer()[0], grey1);
}

#[test]
fn color_background_tile_reaches_framebuffer_after_a_frame() {
    // The Phase 8 colour path end-to-end through `System::run_frame`: a Color
    // machine with the colour bit set resolves the tile pixel through palette
    // RAM (RGB444) rather than the mono shade pool.
    let mut system = System::new(halting_rom());
    system.set_model(HardwareModel::Color);
    let bus = system.bus_mut();
    bus.write_io(0x60, 0x80); // colour-mode bit
    bus.write_io(0x00, 0x01); // enable SCR1
    bus.write_io(0x07, 0x00); // SCR1 map base 0 (tilemap entry 0 → tile 0, palette 0)
                              // Tile 0, row 0, leftmost pixel = colour 1.
    bus.write_u8(0x2000, 0b1000_0000);
    bus.write_u8(0x2001, 0b0000_0000);
    // Palette RAM: palette 0, color 1 → 0xFE00 + (0*16 + 1)*2 = 0xFE02.
    bus.write_u8(0xFE02, 0x0F);
    bus.write_u8(0xFE03, 0x0F);

    system.run_frame(KeyState::NONE);
    assert_eq!(system.framebuffer()[0], 0x0F0F);
}

#[test]
fn sprite_oam_write_during_frame_is_visible_next_frame() {
    // MOV byte [0x0203],8 ; HLT. The program moves sprite 0's X coordinate
    // from 0 to 8 during the first frame. Sprite attributes are latched before
    // visible rendering, so the old position remains stable for frame 1 and the
    // new position appears on frame 2.
    let mut system = System::new(rom_with_reset_code(&[
        0xC6, 0x06, 0x03, 0x02, 0x08, // MOV byte [0x0203], 8
        0xF4, // HLT
    ]));
    let grey1 = {
        let n = (15 - 1u16) & 0x0F;
        (n << 8) | (n << 4) | n
    };
    {
        let bus = system.bus_mut();
        bus.write_io(0x00, 0x04); // enable sprites
        bus.write_io(0x04, 0x01); // OAM base 0x200
        bus.write_io(0x05, 0);
        bus.write_io(0x06, 1);
        bus.write_io(0x30, 0x10); // sprite palette 8: pixel1 -> pool1
        bus.write_io(0x1C, 0x10); // pool1 -> shade 1
        bus.write_u8(0x0200, 1); // tile 1
        bus.write_u8(0x0201, 0);
        bus.write_u8(0x0202, 0); // y
        bus.write_u8(0x0203, 0); // x before CPU moves it
        bus.write_u8(0x2010, 0b1000_0000); // tile 1 row 0, x0 = pixel 1
    }

    system.run_frame(KeyState::NONE);
    assert_eq!(system.framebuffer()[0], grey1);

    system.run_frame(KeyState::NONE);
    assert_eq!(system.framebuffer()[8], grey1);
}

#[test]
fn hblank_timer_counts_visible_and_vblank_scanlines_per_frame() {
    let mut system = System::new(halting_rom());
    let period = SCANLINES_PER_FRAME;
    {
        let bus = system.bus_mut();
        bus.write_io(0xB2, 0xFF); // enable all IRQ sources
        bus.write_io(0xA2, 0x01); // HBlank timer enabled, no auto-reload
        bus.write_io(0xA4, period as u8);
        bus.write_io(0xA5, (period >> 8) as u8);
    }

    system.run_frame(KeyState::NONE);

    assert_eq!(
        system.bus_mut().pending_irq(),
        Some(IrqSource::HBlankTimer as u8)
    );
}

#[test]
fn traced_frame_records_each_visible_scanline() {
    let mut system = System::new(halting_rom());
    let trace = system.run_frame_traced(KeyState::NONE);

    assert_eq!(trace.len(), VISIBLE_SCANLINES as usize);
    assert_eq!(trace.first().map(|t| t.line), Some(0));
    assert_eq!(
        trace.last().map(|t| t.line),
        Some((VISIBLE_SCANLINES - 1) as u8)
    );
}

#[test]
fn traced_frame_observes_scroll_written_before_scanline_render() {
    // MOV AL,7 ; OUT 0x11,AL ; HLT. The CPU runs before line 0 is traced and
    // rendered, so this fixes the current scanline-boundary ordering.
    let mut system = System::new(rom_with_reset_code(&[0xB0, 0x07, 0xE6, 0x11, 0xF4]));
    let trace = system.run_frame_traced(KeyState::NONE);

    assert_eq!(trace[0].scr1_scroll_y, 7);
}

#[test]
fn line_compare_irq_is_raised_during_frame_drive() {
    let mut system = System::new(halting_rom());
    {
        let bus = system.bus_mut();
        bus.write_io(0xB2, 0xFF); // enable all IRQ sources
        bus.write_io(0x03, 12); // LCD line compare
    }

    system.run_frame(KeyState::NONE);

    let cause = system.bus_mut().read_io(0xB4);
    assert_ne!(cause & (1 << IrqSource::ScanlineMatch as u8), 0);
}

#[test]
fn vblank_irq_is_raised_after_visible_scanlines() {
    let mut system = System::new(halting_rom());
    system.bus_mut().write_io(0xB2, 0xFF); // enable all IRQ sources

    system.run_frame(KeyState::NONE);

    let cause = system.bus_mut().read_io(0xB4);
    assert_ne!(cause & (1 << IrqSource::VBlank as u8), 0);
}

#[test]
fn pending_irq_wakes_halted_cpu_even_when_interrupt_flag_is_clear() {
    let mut system = System::new(rom_with_reset_code(&[
        0xFA, // CLI
        0xB0, 0x40, // MOV AL, VBlank IRQ bit
        0xE6, 0xB2, // OUT INT_ENABLE, AL
        0xF4, // HLT
        0xC6, 0x06, 0x00, 0x00, 0x5A, // MOV byte [0], 0x5A
        0xEB, 0xFE, // JMP $
    ]));

    system.run_frame(KeyState::NONE);

    assert_eq!(system.read_memory_at(0), 0x5A);
}

#[test]
fn rtc_free_runs_one_second_across_frames() {
    // The RTC advances from `System::run_frame` alone (no CPU involvement): each
    // frame ticks `CYCLES_PER_FRAME` master-clock cycles, carrying a second every
    // `MASTER_CLOCK_HZ`. 76 frames (> 1 s) yields exactly one carried second.
    let frames = MASTER_CLOCK_HZ.div_ceil(CYCLES_PER_FRAME);
    let mut rom = halting_rom();
    let len = rom.len();
    rom[len - 16 + 0x0C] = 0x04; // footer flags bit 2 = on-cartridge RTC

    // `from_rom` parses the footer (and thus the RTC bit); `new` does not.
    let mut system = System::from_rom(rom);
    assert!(
        system.bus_mut().has_rtc(),
        "RTC footer bit must create a clock"
    );
    system.set_rtc_datetime(26, 7, 3, 5, 12, 0, 0); // seconds = 0
    for _ in 0..frames {
        system.run_frame(KeyState::NONE);
    }
    let bus = system.bus_mut();
    bus.write_io(0xCA, 0x14); // read date/time
    let secs = (0..7).map(|_| bus.read_io(0xCB)).last().unwrap();
    assert_eq!(secs, 0x01); // seconds register carried once
}

#[test]
fn save_data_round_trips_through_system() {
    let sram = vec![0u8; 0x8000];
    let mut system = System::with_sram(halting_rom(), sram);
    let mut snapshot = vec![0u8; 0x8000];
    snapshot[10] = 0xAB;
    system.load_save_data(&snapshot);
    assert_eq!(system.save_data()[10], 0xAB);
}
