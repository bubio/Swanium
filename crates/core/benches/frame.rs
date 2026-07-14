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

/// A warmed synthetic system with 32 visible sprites on scanline 0. This
/// isolates sprite compositing cost without needing an external ROM.
fn warm_sprite_system() -> System {
    let mut system = warm_system();
    let bus = system.bus_mut();

    bus.write_io(0x00, 0x04); // sprites enabled only
    bus.write_io(0x04, 0x01); // OAM base = 0x200
    bus.write_io(0x05, 0x00); // first sprite
    bus.write_io(0x06, 32); // process 32 sprites
    bus.write_io(0x30, 0x10); // sprite palette 8: pixel 1 -> shade 1
    bus.write_io(0x31, 0x32);
    bus.write_io(0x1C, 0x10); // shade pool
    bus.write_io(0x1D, 0x32);

    for row in 0..8u32 {
        let addr = 0x2000 + 16 + row * 2; // tile 1, 2bpp row
        bus.write_u8(addr, 0xFF);
        bus.write_u8(addr + 1, 0x00);
    }

    for idx in 0..32u32 {
        let attr = 1u16; // tile 1, palette 0, back priority
        let [lo, hi] = attr.to_le_bytes();
        let addr = 0x200 + idx * 4;
        bus.write_u8(addr, lo);
        bus.write_u8(addr + 1, hi);
        bus.write_u8(addr + 2, 0); // y
        bus.write_u8(addr + 3, ((idx * 7) % 224) as u8); // x
    }

    system
}

/// A warmed synthetic system with only SCR1 enabled. This isolates the common
/// one-background-layer path and makes disabled-layer buffer work visible.
fn warm_background_system() -> System {
    let mut system = warm_system();
    let bus = system.bus_mut();

    bus.write_io(0x00, 0x01); // SCR1 enabled only
    bus.write_io(0x07, 0x00); // SCR1 map base = 0
    bus.write_io(0x20, 0x10); // palette 0: pixel 1 -> shade-pool entry 1
    bus.write_io(0x21, 0x32);
    bus.write_io(0x1C, 0x10); // identity shade pool
    bus.write_io(0x1D, 0x32);

    let [lo, hi] = 1u16.to_le_bytes(); // map (0, 0) -> tile 1
    bus.write_u8(0, lo);
    bus.write_u8(1, hi);
    for row in 0..8u32 {
        let addr = 0x2000 + 16 + row * 2;
        bus.write_u8(addr, 0xAA);
        bus.write_u8(addr + 1, 0x55);
    }

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

fn bench_render_sprite_scanline(c: &mut Criterion) {
    c.bench_function("render_sprite_scanline", |b| {
        let mut system = warm_sprite_system();
        b.iter(|| system.bus_mut().render_scanline(black_box(0)));
    });
}

fn bench_render_background_scanline(c: &mut Criterion) {
    c.bench_function("render_background_scanline", |b| {
        let mut system = warm_background_system();
        b.iter(|| system.bus_mut().render_scanline(black_box(0)));
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
    bench_render_sprite_scanline,
    bench_render_background_scanline,
    bench_tick_apu,
    bench_tick_apu_wave
);
criterion_main!(benches);
