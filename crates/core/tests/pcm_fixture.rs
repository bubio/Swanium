//! License-clean PCM fixture tests.
//!
//! These tests use the public `Bus` API to emit deterministic audio write
//! streams without committing ROM binaries. They are the sample-sequence oracle
//! for a future public/self-built PCM ROM that drives the same `0x89`, SDMA, and
//! HyperVoice patterns from guest code.

use swanium_core::bus::Bus;
use swanium_core::cpu::MemoryBus;
use swanium_core::HardwareModel;

fn mono_voice_bus() -> Bus {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.write_io(0x91, 0x80); // headphone path: preserve stereo
    bus.write_io(0x90, 0x20); // channel 2 voice mode
    bus.write_io(0x94, 0x05); // full left + full right voice routing
    bus
}

fn color_voice_bus() -> Bus {
    let mut bus = mono_voice_bus();
    bus.set_model(HardwareModel::Color);
    bus.write_io(0x60, 0x80); // WSC color mode gate, matching normal Color execution
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

fn next_stereo_sample(bus: &mut Bus) -> (i16, i16) {
    bus.clear_audio_samples();
    bus.tick_apu(128);
    let samples = bus.audio_samples();
    (samples[0], samples[1])
}

#[test]
fn pcm_fixture_cpu_voice_writes_emit_expected_sequence() {
    let mut bus = mono_voice_bus();

    bus.write_io(0x89, 0xC0); // +64, averaged with reset 0 => +32
    assert_eq!(next_stereo_sample(&mut bus), (2048, 2048));

    bus.write_io(0x89, 0x80); // 0, averaged with previous +64 => +32
    assert_eq!(next_stereo_sample(&mut bus), (2048, 2048));

    bus.write_io(0x89, 0x40); // -64, averaged with previous 0 => -32
    assert_eq!(next_stereo_sample(&mut bus), (-2048, -2048));
}

#[test]
fn pcm_fixture_sdma_voice_writes_emit_expected_sequence() {
    let mut bus = color_voice_bus();
    bus.write_u8(0x0010, 0xC0);
    bus.write_u8(0x0011, 0x40);
    arm_sdma(&mut bus, 0x0010, 2, 0x83); // enable, fastest 24 kHz cadence

    assert_eq!(next_stereo_sample(&mut bus), (2048, 2048));
    assert_eq!(next_stereo_sample(&mut bus), (0, 0));
    assert_eq!(bus.read_io(0x52) & 0x80, 0x00);
}

#[test]
fn pcm_fixture_hypervoice_latch_writes_emit_expected_sequence() {
    let mut bus = Bus::new(vec![0u8; 0x10000]);
    bus.set_model(HardwareModel::Color);
    bus.write_io(0x60, 0x80); // WSC color mode gate
    bus.write_io(0x91, 0x80); // headphone path: preserve stereo
    bus.write_io(0x6A, 0x80); // enable, unsigned mode, shift 0
    bus.write_io(0x6B, 0x60); // route left + right

    bus.write_io(0x69, 0x10); // (0x10 << 8) >> 5 = 128; *32 = 4096
    assert_eq!(next_stereo_sample(&mut bus), (4096, 4096));

    bus.write_io(0x69, 0x20); // (0x20 << 8) >> 5 = 256; *32 = 8192
    assert_eq!(next_stereo_sample(&mut bus), (8192, 8192));
}
