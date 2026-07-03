//! Headless per-subsystem profiler for the emulator core.
//!
//! Runs the frame pipeline for a fixed number of frames and prints where the
//! time goes (CPU / PPU / APU / DMA). Requires the `profiling` feature:
//!
//! ```sh
//! cargo run -p swanium-core --features profiling --example profile --release
//! cargo run -p swanium-core --features profiling --example profile --release -- path/to/rom.ws
//! ```
//!
//! A ROM may be given as the first CLI argument or via the `SWANIUM_BENCH_ROM`
//! environment variable; with neither, a tiny self-contained synthetic ROM is
//! used so the example always runs standalone.

use swanium_core::keypad::KeyState;
use swanium_core::system::System;

/// Frames to execute before reporting (≈ several seconds of emulated time).
/// Override with `SWANIUM_PROFILE_FRAMES` to run long enough for an external
/// sampling profiler (e.g. macOS `sample`) to attach.
const DEFAULT_FRAMES: u32 = 600;

fn frame_count() -> u32 {
    std::env::var("SWANIUM_PROFILE_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_FRAMES)
}

fn main() {
    let rom = match load_rom() {
        Some((path, bytes)) => {
            eprintln!("profiling ROM: {path} ({} bytes)", bytes.len());
            bytes
        }
        None => {
            eprintln!("profiling synthetic ROM (no ROM given)");
            synthetic_rom()
        }
    };

    let mut system = System::from_rom(rom);
    // Warm up briefly so first-frame allocation/caching noise is excluded.
    for _ in 0..30 {
        system.run_frame(KeyState::default());
    }
    system.reset_profile();

    for _ in 0..frame_count() {
        system.run_frame(KeyState::default());
    }

    let s = system.profile_snapshot();
    println!("{s}");
    println!(
        "  CPU {:>8.3} ms/frame ({:>4.1}%)",
        per_frame_ms(s.cpu_ns, s.frames),
        s.cpu_pct
    );
    println!(
        "  PPU {:>8.3} ms/frame ({:>4.1}%)",
        per_frame_ms(s.ppu_ns, s.frames),
        s.ppu_pct
    );
    println!(
        "  APU {:>8.3} ms/frame ({:>4.1}%)",
        per_frame_ms(s.apu_ns, s.frames),
        s.apu_pct
    );
    println!(
        "  DMA {:>8.3} ms/frame ({:>4.1}%)",
        per_frame_ms(s.dma_ns, s.frames),
        s.dma_pct
    );
    let fps = if s.avg_frame_ns > 0 {
        1.0e9 / s.avg_frame_ns as f64
    } else {
        0.0
    };
    println!("  → {fps:.0} frames/s headroom (target 75)");
}

fn per_frame_ms(ns: u64, frames: u64) -> f64 {
    if frames == 0 {
        0.0
    } else {
        ns as f64 / frames as f64 / 1.0e6
    }
}

fn load_rom() -> Option<(String, Vec<u8>)> {
    let path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("SWANIUM_BENCH_ROM").ok())?;
    match std::fs::read(&path) {
        Ok(bytes) => Some((path, bytes)),
        Err(e) => {
            eprintln!("failed to read {path}: {e}; falling back to synthetic ROM");
            None
        }
    }
}

/// A minimal 64 KiB ROM whose reset vector runs an infinite `JMP $` (`EB FE`).
/// Deterministic CPU work every frame plus the full PPU/APU/DMA pipeline; no
/// external file or copyrighted content required.
fn synthetic_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 0x10000];
    // Reset maps physical 0xFFFF0 → 0xFFF0 in a 64 KiB image; put the loop there.
    rom[0xFFF0] = 0xEB; // JMP rel8
    rom[0xFFF1] = 0xFE; // -2 → jump to self
    rom
}
