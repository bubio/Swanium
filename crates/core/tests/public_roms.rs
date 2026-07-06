//! Opt-in integration tests for public WonderSwan CPU test ROMs.
//!
//! These tests are marked `#[ignore]` by default because the ROM files are not
//! committed to the repository (licensing constraints).  To run them:
//!
//! 1. Download the ROM(s) from the sources listed in `tests/README.md`.
//! 2. Set the relevant environment variable to the path of the ROM file.
//! 3. Run with the `--include-ignored` flag:
//!
//! ```sh
//! WS_CPU_TEST_ROM=/path/to/WSCpuTest.wsc \
//!     cargo test -p swanium-core --test public_roms -- --include-ignored
//! ```

use std::path::{Path, PathBuf};

use swanium_core::bus::Bus;
use swanium_core::cpu::{Cpu, MemoryBus};
use swanium_core::keypad::KeyState;
use swanium_core::model::HardwareModel;
use swanium_core::system::System;

// ── Harness ──────────────────────────────────────────────────────────────────

const DEFAULT_WS_CPU_TEST_ROM: &str = "tests/fixtures/cpu/public/WSCpuTest.wsc";
const WSC_CPU_TEST_MAX_FRAMES: usize = 75 * 180;
const WSC_CPU_TEST_BACKGROUND_MAP: u32 = 0x1000;
const WSC_CPU_TEST_TILEMAP_WIDTH: usize = 32;
const WSC_CPU_TEST_TILEMAP_HEIGHT: usize = 32;
const WSC_CPU_TEST_TILEMAP_STRIDE_BYTES: u32 = 64;
const WSC_CPU_TEST_IS_TESTING_ADDR: u32 = 0x0136;

/// Minimal legacy harness for ROMs whose pass/fail protocol is still unknown.
/// This is not suitable for WSCpuTest because that ROM uses HLT as a normal
/// VBlank wait and needs the full [`System`] frame driver.
fn boot_rom_until_hlt(rom: Vec<u8>, max_cycles: u64) -> (Cpu, Bus) {
    let mut bus = Bus::new(rom);
    let mut cpu = Cpu::new();
    cpu.reset(0xFFFF, 0x0000);
    cpu.regs.sp = 0x3FFE;
    let mut cycles = 0u64;
    while !cpu.halted && cycles < max_cycles {
        cycles += cpu.step(&mut bus) as u64;
    }
    (cpu, bus)
}

fn rom_path_from_env_or_default(env_var: &str, default_path: &str) -> PathBuf {
    std::env::var_os(env_var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_path))
}

fn read_rom(path: &Path, env_var: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}; set {env_var}=<rom path> or place the ROM at {}",
            path.display(),
            DEFAULT_WS_CPU_TEST_ROM
        )
    })
}

fn background_map_text(system: &System) -> String {
    let mut text =
        String::with_capacity((WSC_CPU_TEST_TILEMAP_WIDTH + 1) * WSC_CPU_TEST_TILEMAP_HEIGHT);
    for y in 0..WSC_CPU_TEST_TILEMAP_HEIGHT {
        for x in 0..WSC_CPU_TEST_TILEMAP_WIDTH {
            let addr = WSC_CPU_TEST_BACKGROUND_MAP
                + y as u32 * WSC_CPU_TEST_TILEMAP_STRIDE_BYTES
                + x as u32 * 2;
            let byte = system.read_memory_at(addr);
            let ch = match byte {
                0 => ' ',
                0x20..=0x7E => byte as char,
                _ => '.',
            };
            text.push(ch);
        }
        text.push('\n');
    }
    text
}

fn run_wscputest_until_result(rom: Vec<u8>) -> (System, String) {
    let mut system = System::from_rom(rom);
    system.set_model(HardwareModel::Color);

    // Let the ROM reach its menu, then press A for one frame to choose the
    // default first menu item: "Test All".
    for _ in 0..8 {
        system.run_frame(KeyState::NONE);
    }
    system.run_frame(KeyState::A);
    system.run_frame(KeyState::NONE);

    let mut latest_text = String::new();
    for _ in 0..WSC_CPU_TEST_MAX_FRAMES {
        system.run_frame(KeyState::NONE);
        latest_text = background_map_text(&system);
        if latest_text.contains("Failed!") {
            break;
        }
        if latest_text.contains("Ok!")
            && system.read_memory_at(WSC_CPU_TEST_IS_TESTING_ADDR) == 0
            && system.cpu().halted
        {
            break;
        }
    }

    (system, latest_text)
}

// ── WSCPUTest (FluBBaOfWard) ─────────────────────────────────────────────────

/// Runs the WSCpuTest ROM (FluBBaOfWard/WSCpuTest) and checks for a passing
/// result.
///
/// # ROM output format
///
/// Build with `nasm -f bin -o WSCpuTest.wsc WSCpuTest.asm` from the upstream
/// v0.7.1 source, then set `WS_CPU_TEST_ROM` to the `.wsc` path or place it at
/// `tests/fixtures/cpu/public/WSCpuTest.wsc`.
///
/// The upstream README documents the externally visible protocol: the ROM
/// writes `Ok!` after successful tests and prints `Failed!` plus expected/tested
/// values at the first failure.  The text is emitted through INT 0x10 into the
/// background tile map, whose low bytes contain ASCII tile indices.  This test
/// starts the default `Test All` menu item with the A button and scans that
/// tile map for `Ok!` / `Failed!`.
///
/// Run with: `WS_CPU_TEST_ROM=/path/to/WSCpuTest.wsc cargo test -p swanium-core
///   --test public_roms -- --include-ignored wscputest`
#[test]
#[ignore = "requires WSCpuTest.wsc; set WS_CPU_TEST_ROM or place it under tests/fixtures/cpu/public"]
fn wscputest_all_tests_pass() {
    let path = rom_path_from_env_or_default("WS_CPU_TEST_ROM", DEFAULT_WS_CPU_TEST_ROM);
    let rom = read_rom(&path, "WS_CPU_TEST_ROM");
    let (system, text) = run_wscputest_until_result(rom);

    assert!(
        !text.contains("Failed!"),
        "WSCpuTest reported failure; visible background text:\n{text}"
    );
    assert!(
        text.contains("Ok!"),
        "WSCpuTest did not produce Ok! within {WSC_CPU_TEST_MAX_FRAMES} frames; \
         cpu_halted={}, is_testing={}, visible background text:\n{text}",
        system.cpu().halted,
        system.read_memory_at(WSC_CPU_TEST_IS_TESTING_ADDR)
    );
}

// ── ws-test-suite (asiekierka) ────────────────────────────────────────────────

/// Runs a single ROM from the ws-test-suite (asiekierka/ws-test-suite).
///
/// Set `WS_TEST_SUITE_ROM` to the path of a specific test ROM from the suite.
///
/// Run with: `WS_TEST_SUITE_ROM=/path/to/test.ws cargo test -p swanium-core
///   --test public_roms -- --include-ignored ws_test_suite`
#[test]
#[ignore = "requires a ws-test-suite ROM; set WS_TEST_SUITE_ROM=/path/to/test.ws"]
fn ws_test_suite_rom_passes() {
    let path = std::env::var("WS_TEST_SUITE_ROM")
        .expect("WS_TEST_SUITE_ROM must point to a ws-test-suite .ws ROM");
    let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));

    let max_cycles = 3_072_000u64 * 10;
    let (cpu, bus) = boot_rom_until_hlt(rom, max_cycles);

    // TODO(issue): confirm ws-test-suite output convention and update accordingly.
    assert!(
        cpu.halted,
        "ws-test-suite ROM did not reach HLT within the cycle budget"
    );
    assert_eq!(
        bus.read_u8(0x0000),
        0x00,
        "ws-test-suite result byte at WRAM[0x0000] is non-zero"
    );
}
