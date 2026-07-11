//! Throughput benchmarks for the emulator core's frame pipeline.
//!
//! Run with `cargo bench -p swanium-core`. By default a tiny self-contained
//! synthetic ROM is used so the bench runs in CI with no external files; set
//! `SWANIUM_BENCH_ROM=path/to/rom.ws` to benchmark against a real title's CPU
//! workload instead.
//!
//! `bench_frame` measures a whole `run_frame`; the micro-benchmarks isolate the
//! PPU scanline renderer and the APU tick so a change can be attributed. For a
//! per-subsystem split of a *single* run, build the core with `--features
//! profiling` and use the `profile` example instead.

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use swanium_core::cpu::MemoryBus;
use swanium_core::keypad::KeyState;
use swanium_core::system::System;

/// A minimal 64 KiB ROM whose reset vector runs an infinite `JMP $` (`EB FE`).
/// Deterministic CPU work plus the full PPU/APU/DMA pipeline; no external file
/// or copyrighted content required.
fn synthetic_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x10000];
    // Reset maps physical 0xFFFF0 → 0xFFF0 in a 64 KiB image; put the loop there.
    rom[0xFFF0] = 0xEB; // JMP rel8
    rom[0xFFF1] = 0xFE; // -2 → jump to self
    rom
}

/// The ROM to benchmark: a real title via `SWANIUM_BENCH_ROM`, else synthetic.
fn bench_rom() -> Vec<u8> {
    match std::env::var("SWANIUM_BENCH_ROM") {
        Ok(path) => {
            std::fs::read(&path).unwrap_or_else(|e| panic!("SWANIUM_BENCH_ROM={path}: {e}"))
        }
        Err(_) => synthetic_rom(),
    }
}

/// A system advanced a few frames so the benched state is representative
/// (past reset/boot rather than a cold machine).
fn warm_system() -> System {
    let mut system = System::from_rom(bench_rom());
    for _ in 0..30 {
        system.run_frame(KeyState::default());
    }
    system
}

/// A warmed system with a single plain wave channel enabled: this exercises the
/// APU wave-only fast path rather than the complete-silence path.
fn warm_wave_system() -> System {
    let mut system = warm_system();
    let bus = system.bus_mut();
    for i in 0..16 {
        bus.write_u8(i, 0x55);
    }
    bus.write_io(0x80, 0x00); // ch1 pitch low
    bus.write_io(0x81, 0x00); // ch1 pitch high
    bus.write_io(0x88, 0x11); // ch1 left/right volume
    bus.write_io(0x8F, 0x00); // waveform base
    bus.write_io(0x90, 0x01); // enable ch1, no voice/sweep/noise
    bus.write_io(0x91, 0x80); // headphone path
    system.clear_audio_samples();
    system
}

fn bench_frame(c: &mut Criterion) {
    c.bench_function("run_frame", |b| {
        let mut system = warm_system();
        b.iter(|| system.run_frame(black_box(KeyState::default())));
    });
}

fn bench_render_scanline(c: &mut Criterion) {
    c.bench_function("render_scanline", |b| {
        let mut system = warm_system();
        b.iter(|| {
            for line in 0u8..144 {
                system.bus_mut().render_scanline(black_box(line));
            }
        });
    });
}

fn bench_tick_apu(c: &mut Criterion) {
    c.bench_function("tick_apu_frame", |b| {
        let mut system = warm_system();
        // One frame's worth of sound-clock ticks (159 scanlines × 256 cycles).
        b.iter(|| {
            for _ in 0..159 {
                system.bus_mut().tick_apu(black_box(256));
            }
        });
    });
}

fn bench_tick_apu_wave(c: &mut Criterion) {
    c.bench_function("tick_apu_wave_frame", |b| {
        let mut system = warm_wave_system();
        b.iter(|| {
            for _ in 0..159 {
                system.bus_mut().tick_apu(black_box(256));
            }
        });
    });
}

criterion_group!(
    benches,
    bench_frame,
    bench_render_scanline,
    bench_tick_apu,
    bench_tick_apu_wave
);
criterion_main!(benches);
