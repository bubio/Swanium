//! Integration tests for WonderSwan Color (Phase 8) features driven through the
//! full public `Bus` stack: colour PPU rendering (RGB444 palette RAM), the
//! HyperVoice APU extension, and the cartridge RTC. Each test also pins the
//! monochrome-regression side of the same seam — the Color additions gate on
//! `HardwareModel`/port `0x60` bit 7, so a machine modelled as `Mono` (or Color
//! with the colour bit clear) must still produce the pre-Phase-8 behaviour.
//!
//! Unit-level coverage of these paths lives in `src/ppu/tests.rs`,
//! `src/bus/tests.rs`, and `src/apu/mod.rs`; these tests exercise the same
//! behaviour end-to-end through the public API the frontend uses.

use swanium_core::bus::Bus;
use swanium_core::cpu::{Cpu, MemoryBus};
use swanium_core::HardwareModel;

const STEREO: usize = 2;

/// The RGB444 framebuffer value a monochrome `shade` (0–15) resolves to (the
/// mono resolver inverts brightness: shade 0 = white = 0x0FFF).
fn grey(shade: u8) -> u16 {
    let n = (15 - (shade & 0x0F)) as u16;
    (n << 8) | (n << 4) | n
}

// ── PPU colour rendering ──────────────────────────────────────────────────────

/// Write an identity monochrome palette (tile pixel `i` → shade `i`) for
/// palette 0, so the mono-compat path is well-defined when the colour bit is
/// clear.
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

/// Bus with SCR1 enabled and tile 0 drawing pixel 1 (palette 0) at the top-left
/// corner, an identity mono palette, and colour palette-RAM entry (palette 0,
/// color 1) set to `color_ram`. The caller picks the model and port 0x60 to
/// select the render path. Palette RAM at 0xFE00 is only writable on Color
/// hardware, so the model is promoted before the write.
fn configure_scr1_color_pixel(bus: &mut Bus, color_ram: u16) {
    bus.write_io(0x00, 0x01); // SCR1 enable
    bus.write_io(0x07, 0x00); // SCR1 map base 0 (tilemap entry 0 → tile 0, palette 0)
    write_identity_palette(bus);
    write_tile_row(bus, 0, 0, 0b1000_0000, 0b0000_0000); // tile 0 row 0: x0 = pixel 1
                                                         // Palette RAM: palette 0, color 1 → 0xFE00 + (0*16 + 1)*2 = 0xFE02.
    let [lo, hi] = (color_ram & 0x0FFF).to_le_bytes();
    bus.write_u8(0xFE02, lo);
    bus.write_u8(0xFE03, hi);
}

fn setup_scr1_color_pixel(model: HardwareModel, color_ram: u16) -> Bus {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.set_model(model);
    configure_scr1_color_pixel(&mut bus, color_ram);
    bus
}

#[test]
fn color_model_renders_pixel_from_palette_ram() {
    let mut bus = setup_scr1_color_pixel(HardwareModel::Color, 0x0F0F);
    bus.write_io(0x60, 0x80); // colour-mode bit set
    bus.render_scanline(0);
    assert_eq!(bus.framebuffer()[0], 0x0F0F);
}

#[test]
fn color_model_without_color_bit_falls_back_to_mono() {
    // Colour hardware, but the game never sets port 0x60 bit 7: the mono shade
    // path renders, ignoring palette RAM entirely (pre-Phase-8 behaviour).
    let mut bus = setup_scr1_color_pixel(HardwareModel::Color, 0x0F0F);
    bus.write_io(0x60, 0x00);
    bus.render_scanline(0);
    assert_eq!(bus.framebuffer()[0], grey(1));
}

#[test]
fn mono_model_ignores_color_bit() {
    // Mono regression: even with the colour bit set, mono hardware never takes
    // the colour path.
    let mut bus = setup_scr1_color_pixel(HardwareModel::Mono, 0x0F0F);
    bus.write_io(0x60, 0x80);
    bus.render_scanline(0);
    assert_eq!(bus.framebuffer()[0], grey(1));
}

#[test]
fn mono_model_drops_palette_ram_writes() {
    // The 0xFE00 palette-RAM window is gated on Color hardware for writes too:
    // a mono machine drops the write, so promoting to Color afterwards reads 0.
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_u8(0xFE02, 0xFF); // dropped on mono
    bus.write_u8(0xFE03, 0x0F);
    bus.set_model(HardwareModel::Color);
    assert_eq!(bus.read_u8(0xFE02), 0x00);
    assert_eq!(bus.read_u8(0xFE03), 0x00);
}

// ── HyperVoice APU extension ──────────────────────────────────────────────────

/// Enable HyperVoice with a fixed 8-bit latch routed to both channels. With the
/// four wave channels silent, the emitted stereo frame is pure HyperVoice.
///
/// `data = 0x10` in unsigned mode (ctrl bit pattern 0x80, shift 0) expands to
/// `0x10 << 8 = 0x1000`, `>> 5 = 128`, then `× MIX_SCALE (32) = 4096` per side.
fn setup_hypervoice(bus: &mut Bus) {
    bus.write_io(0x60, 0x80); // WSC color mode: HyperVoice is unavailable without it
    bus.write_io(0x91, 0x80); // headphone path: preserve HyperVoice routing
    bus.write_io(0x6A, 0x80); // HV_CTRL: enable, unsigned mode, shift 0
    bus.write_io(0x6B, 0x60); // HV_CHAN_CTRL: route left (0x40) + right (0x20)
    bus.write_io(0x69, 0x10); // HV_DATA: 8-bit latch
}

#[test]
fn color_hypervoice_emits_stereo_sample() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.set_model(HardwareModel::Color);
    setup_hypervoice(&mut bus);
    bus.tick_apu(128); // one output period
    let samples = bus.audio_samples();
    assert_eq!(samples.len(), STEREO);
    assert_eq!(samples[0], 4096); // left
    assert_eq!(samples[1], 4096); // right
}

#[test]
fn color_hypervoice_routes_left_only() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.set_model(HardwareModel::Color);
    setup_hypervoice(&mut bus);
    bus.write_io(0x6B, 0x40); // left only
    bus.tick_apu(128);
    let samples = bus.audio_samples();
    assert_eq!(samples[0], 4096);
    assert_eq!(samples[1], 0);
}

#[test]
fn color_hypervoice_direct_output_emits_signed_stereo_words() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.set_model(HardwareModel::Color);
    bus.write_io(0x60, 0x80);
    bus.write_io(0x91, 0x80);
    bus.write_io(0x6A, 0x80);
    bus.write_io(0x64, 0x00);
    bus.write_io(0x65, 0x10);
    bus.write_io(0x66, 0x00);
    bus.write_io(0x67, 0xF0);
    bus.tick_apu(128);
    let samples = bus.audio_samples();
    assert_eq!(samples[0], 4096);
    assert_eq!(samples[1], -4096);
}

#[test]
fn color_hypervoice_direct_zero_silences_prior_8_bit_latch() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.set_model(HardwareModel::Color);
    setup_hypervoice(&mut bus);
    bus.write_io(0x64, 0x00);
    bus.write_io(0x65, 0x00);
    bus.write_io(0x66, 0x00);
    bus.write_io(0x67, 0x00);
    bus.tick_apu(128);
    let samples = bus.audio_samples();
    assert_eq!(samples[0], 0);
    assert_eq!(samples[1], 0);
}

#[test]
fn color_mode_disabled_is_silent_with_hypervoice_registers_set() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.set_model(HardwareModel::Color);
    bus.write_io(0x91, 0x80);
    bus.write_io(0x6A, 0x80);
    bus.write_io(0x69, 0x10);
    bus.write_io(0x6B, 0x60);
    bus.tick_apu(128);
    let samples = bus.audio_samples();
    assert_eq!(samples[0], 0);
    assert_eq!(samples[1], 0);
}

#[test]
fn mono_model_is_silent_with_hypervoice_registers_set() {
    // Mono regression: HyperVoice register writes are dropped on mono, and the
    // APU mix skips HyperVoice on non-Color hardware, so the output is silent.
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    setup_hypervoice(&mut bus);
    bus.tick_apu(128);
    let samples = bus.audio_samples();
    assert_eq!(samples[0], 0);
    assert_eq!(samples[1], 0);
}

#[test]
fn demoting_to_mono_stops_hypervoice_mix() {
    // Even if a Color machine armed HyperVoice, a runtime demotion to Mono must
    // not leak the stale enable bit into the mono mix (symmetric with the
    // read/write palette-RAM gate).
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.set_model(HardwareModel::Color);
    setup_hypervoice(&mut bus);
    bus.set_model(HardwareModel::Mono);
    bus.tick_apu(128);
    assert_eq!(bus.audio_samples()[0], 0);
}

// ── CPU → I/O → colour PPU path ───────────────────────────────────────────────

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

#[test]
fn cpu_out_enabling_color_bit_makes_palette_ram_visible() {
    // ROM bank 0 program: MOV AL,0x80 ; OUT 0x60,AL ; HLT — the game itself sets
    // the colour-mode bit (as real WSC titles do after reading HW_FLAGS 0xA0).
    let mut rom = vec![0u8; 0x10000];
    #[rustfmt::skip]
    let code = [
        0xB0, 0x80, // MOV AL, 0x80
        0xE6, 0x60, // OUT 0x60, AL  (display control: colour-mode bit)
        0xF4,       // HLT
    ];
    rom[..code.len()].copy_from_slice(&code);

    let mut bus = Bus::new(rom);
    bus.set_model(HardwareModel::Color);
    configure_scr1_color_pixel(&mut bus, 0x0ABC);
    bus.write_io(0xC2, 0x00); // ROM bank 0 → physical 0x20000

    run_cpu_until_halt(&mut bus, 1_000);
    bus.render_scanline(0);
    assert_eq!(bus.framebuffer()[0], 0x0ABC);
}
