//! Integration tests for the PPU: drive the full Bus + PPU stack one frame at
//! a time and check the resulting framebuffer.
//!
//! Tile data, tile maps, and display registers are written through the public
//! API (`MemoryBus::write_u8` for WRAM, `Bus::write_io` for display
//! registers), standing in for a real ROM→WRAM setup. One test additionally
//! drives the display-enable register from a V30MZ `OUT` instruction to
//! exercise the CPU → I/O → PPU path.

use swanium_core::bus::{Bus, IrqSource};
use swanium_core::cpu::{Cpu, MemoryBus};

const SCREEN_WIDTH: usize = 224;
const VISIBLE_LINES: u8 = 144;

/// The RGB444 framebuffer value a monochrome `shade` (0–15) resolves to (the
/// mono resolver inverts brightness: shade 0 = white = 0x0FFF).
fn grey(shade: u8) -> u16 {
    let n = (15 - (shade & 0x0F)) as u16;
    (n << 8) | (n << 4) | n
}

// ── Harness ──────────────────────────────────────────────────────────────────

/// Write an identity monochrome palette (tile pixel `i` → shade `i`) for
/// palette 0 via the public I/O port API.
fn write_identity_palette(bus: &mut Bus) {
    bus.write_io(0x20, 0x10); // palette 0: pixel1 → pool1
    bus.write_io(0x21, 0x32); //            pixel2 → pool2, pixel3 → pool3
    bus.write_io(0x1C, 0x10); // pool0 → 0, pool1 → 1
    bus.write_io(0x1D, 0x32); // pool2 → 2, pool3 → 3
}

/// Write one planar tile row (2 bits/pixel) into WRAM tile data at 0x2000.
fn write_tile_row(bus: &mut Bus, tile_idx: u32, row: u32, plane0: u8, plane1: u8) {
    let addr = 0x2000 + tile_idx * 16 + row * 2;
    bus.write_u8(addr, plane0);
    bus.write_u8(addr + 1, plane1);
}

/// Render a full visible frame (144 scanlines) then signal VBlank.
fn render_frame(bus: &mut Bus) {
    for line in 0..VISIBLE_LINES {
        bus.render_scanline(line);
    }
    bus.on_vblank();
}

/// Run the CPU from ROM bank 0 (CS=0x2000, IP=0) until HLT or `max_cycles`.
fn run_cpu_until_halt(bus: &mut Bus, max_cycles: u32) {
    let mut cpu = Cpu::new();
    cpu.reset(0x2000, 0x0000);
    cpu.regs.sp = 0x3FFE;
    let mut cycles = 0u32;
    while !cpu.halted && cycles < max_cycles {
        cycles += cpu.step(bus);
    }
}

// ── Background rendering through the full stack ───────────────────────────────

#[test]
fn frame_renders_scr1_tile_to_top_left() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x00, 0x01); // SCR1 enable
    bus.write_io(0x07, 0x00); // SCR1 map base 0
    write_identity_palette(&mut bus);
    write_tile_row(&mut bus, 0, 0, 0b1000_0000, 0b0000_0000); // tile 0 row 0: x0 = 1
    render_frame(&mut bus);
    assert_eq!(bus.framebuffer()[0], grey(1));
}

#[test]
fn frame_renders_pixel_on_later_scanline() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x00, 0x01);
    bus.write_io(0x07, 0x00);
    write_identity_palette(&mut bus);
    write_tile_row(&mut bus, 0, 3, 0b1000_0000, 0b0000_0000); // tile 0 row 3: x0 = 1
    render_frame(&mut bus);
    assert_eq!(bus.framebuffer()[3 * SCREEN_WIDTH], grey(1));
}

#[test]
fn frame_leaves_background_clear_where_no_tile_pixel() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x00, 0x01);
    bus.write_io(0x07, 0x00);
    write_identity_palette(&mut bus);
    write_tile_row(&mut bus, 0, 0, 0b1000_0000, 0b0000_0000);
    render_frame(&mut bus);
    assert_eq!(bus.framebuffer()[1], grey(0)); // x=1 has no pixel set
}

#[test]
fn frame_signals_vblank_interrupt() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0xB2, 1 << IrqSource::VBlank as u8);
    render_frame(&mut bus);
    let cause = bus.read_io(0xB4);
    assert_ne!(cause & (1 << IrqSource::VBlank as u8), 0);
}

// ── Sprite rendering through the full stack ───────────────────────────────────

#[test]
fn frame_renders_sprite_pixel() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x00, 0x04); // SPR enable
    bus.write_io(0x04, 0x01); // OAM base = 1 << 9 = 0x200
    bus.write_io(0x05, 0); // first sprite 0
    bus.write_io(0x06, 1); // process 1 sprite
    bus.write_io(0x30, 0x10); // sprite palette (idx 8): pixel1 → pool1
    bus.write_io(0x1C, 0x10); // pool1 → shade 1

    // sprite 0: tile 1, x=20, y=0
    bus.write_u8(0x200, 0x01); // attr word low = tile 1
    bus.write_u8(0x201, 0x00); // attr word high
    bus.write_u8(0x202, 0); // Y = 0
    bus.write_u8(0x203, 20); // X = 20
    write_tile_row(&mut bus, 1, 0, 0b1000_0000, 0b0000_0000); // tile 1 row 0: x0 = 1
    render_frame(&mut bus);
    assert_eq!(bus.framebuffer()[20], grey(1));
}

// ── CPU → I/O → PPU path ──────────────────────────────────────────────────────

#[test]
fn cpu_out_enabling_scr1_makes_tile_visible() {
    // ROM bank 0 program: MOV AL,1 ; OUT 0x00,AL ; HLT — enables SCR1 via I/O.
    let mut rom = vec![0u8; 0x10000];
    #[rustfmt::skip]
    let code = [
        0xB0, 0x01, // MOV AL, 1
        0xE6, 0x00, // OUT 0x00, AL  (DISP_CTRL = SCR1 enable)
        0xF4,       // HLT
    ];
    rom[..code.len()].copy_from_slice(&code);

    let mut bus = Bus::new(rom);
    bus.write_io(0xC2, 0x00); // ROM bank 0 → ROM[0] at physical 0x20000
    bus.write_io(0x07, 0x00); // SCR1 map base 0
    write_identity_palette(&mut bus);
    write_tile_row(&mut bus, 0, 0, 0b1000_0000, 0b0000_0000); // tile 0 row 0: x0 = 1

    run_cpu_until_halt(&mut bus, 1_000);
    render_frame(&mut bus);
    assert_eq!(bus.framebuffer()[0], grey(1));
}

#[test]
fn scr1_stays_disabled_without_cpu_enabling_it() {
    // Same setup as above but the CPU never runs: SCR1 remains disabled, so
    // nothing is drawn.
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x07, 0x00);
    write_identity_palette(&mut bus);
    write_tile_row(&mut bus, 0, 0, 0b1000_0000, 0b0000_0000);
    render_frame(&mut bus);
    assert_eq!(bus.framebuffer()[0], grey(0));
}
