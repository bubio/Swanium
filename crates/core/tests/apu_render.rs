//! Integration tests for the APU: drive the full Bus + APU stack and check the
//! generated stereo sample buffer.
//!
//! Waveform data is written into WRAM and sound registers through the public
//! API (`MemoryBus::write_u8`, `Bus::write_io`). One test drives the
//! channel-enable register from a V30MZ `OUT` instruction to exercise the
//! CPU → I/O → APU path.

use swanium_core::bus::Bus;
use swanium_core::cpu::{Cpu, MemoryBus};

const STEREO: usize = 2;

// ── Harness ──────────────────────────────────────────────────────────────────

/// Configure channel 1 as a steady tone: waveform byte 0 high nibble = 5
/// (sampled at index 1, held because pitch 0 → period 2048), left volume 3.
fn setup_channel1_tone(bus: &mut Bus) {
    bus.write_io(0x90, 0x01); // SND_CTRL: channel 1 enable
    bus.write_io(0x88, 0x30); // channel 1 volume: L = 3, R = 0
    bus.write_io(0x80, 0x00); // pitch low
    bus.write_io(0x81, 0x00); // pitch high → period 2048 (sample held)
    bus.write_u8(0x0000, 0x50); // waveform byte 0: index 1 high nibble = 5
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

// ── Sample generation through the full stack ──────────────────────────────────

#[test]
fn no_samples_before_first_output_period() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    setup_channel1_tone(&mut bus);
    bus.tick_apu(127);
    assert!(bus.audio_samples().is_empty());
}

#[test]
fn one_stereo_sample_after_output_period() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    setup_channel1_tone(&mut bus);
    bus.tick_apu(128);
    assert_eq!(bus.audio_samples().len(), STEREO);
}

#[test]
fn channel1_left_sample_reflects_volume() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    setup_channel1_tone(&mut bus);
    bus.tick_apu(128);
    assert_eq!(bus.audio_samples()[0], 480); // raw 5 × vol 3 = 15, × MIX_SCALE 32 = 480
}

#[test]
fn channel1_right_sample_is_muted() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    setup_channel1_tone(&mut bus);
    bus.tick_apu(128);
    assert_eq!(bus.audio_samples()[1], 0); // right volume 0
}

#[test]
fn disabled_sound_chip_is_silent() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    setup_channel1_tone(&mut bus);
    bus.write_io(0x90, 0x00); // disable all channels
    bus.tick_apu(128);
    assert_eq!(bus.audio_samples()[0], 0);
}

#[test]
fn clear_audio_samples_drains_buffer() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    setup_channel1_tone(&mut bus);
    bus.tick_apu(256);
    bus.clear_audio_samples();
    assert!(bus.audio_samples().is_empty());
}

// ── CPU → I/O → APU path ──────────────────────────────────────────────────────

#[test]
fn cpu_out_enabling_channel1_makes_tone_audible() {
    // ROM bank 0 program: MOV AL,1 ; OUT 0x90,AL ; HLT — enables channel 1.
    let mut rom = vec![0u8; 0x10000];
    #[rustfmt::skip]
    let code = [
        0xB0, 0x01, // MOV AL, 1
        0xE6, 0x90, // OUT 0x90, AL  (SND_CTRL = channel 1 enable)
        0xF4,       // HLT
    ];
    rom[..code.len()].copy_from_slice(&code);

    let mut bus = Bus::new(rom);
    bus.write_io(0xC2, 0x00); // ROM bank 0 → ROM[0] at physical 0x20000
    bus.write_io(0x88, 0x30); // channel 1 volume L = 3
    bus.write_io(0x80, 0x00);
    bus.write_io(0x81, 0x00);
    bus.write_u8(0x0000, 0x50); // waveform index 1 = 5

    run_cpu_until_halt(&mut bus, 1_000);
    bus.tick_apu(128);
    assert_eq!(bus.audio_samples()[0], 480); // raw 15 × MIX_SCALE 32 = 480
}

#[test]
fn channel1_stays_silent_without_cpu_enabling_it() {
    // Same setup but the CPU never runs: SND_CTRL stays 0, nothing is generated.
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x88, 0x30);
    bus.write_u8(0x0000, 0x50);
    bus.tick_apu(128);
    assert_eq!(bus.audio_samples()[0], 0);
}
