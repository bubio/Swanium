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
//! WS_CPU_TEST_ROM=/path/to/WSCPUTest.ws \
//!     cargo test -p swanium-core --test public_roms -- --include-ignored
//! ```
//!
//! # Verification approach
//!
//! WonderSwan test ROMs typically signal pass/fail by writing a result byte to
//! a known WRAM address or by entering an infinite loop vs. a HLT.  The exact
//! protocol varies per ROM.  Each test below documents the expected output
//! format in its comment block; update it once the ROM's output convention is
//! confirmed against the real hardware or the ROM's source code.

use swanium_core::bus::Bus;
use swanium_core::cpu::{Cpu, MemoryBus};

// ── Harness ──────────────────────────────────────────────────────────────────

/// Boots a `.ws` ROM image: places ROM bytes into the Bus, resets the CPU to
/// the V30MZ power-on state (CS=0xFFFF, IP=0x0000, physical 0xFFFF0), and
/// runs for up to `max_cycles` cycles.
///
/// The caller is responsible for interpreting the resulting Bus state.
fn boot_rom(rom: Vec<u8>, max_cycles: u64) -> (Cpu, Bus) {
    let mut bus = Bus::new(rom);
    let mut cpu = Cpu::new();
    cpu.reset(0xFFFF, 0x0000); // V30MZ power-on reset vector
    cpu.regs.sp = 0x3FFE;
    let mut cycles = 0u64;
    while !cpu.halted && cycles < max_cycles {
        cycles += cpu.step(&mut bus) as u64;
    }
    (cpu, bus)
}

// ── WSCPUTest (FluBBaOfWard) ─────────────────────────────────────────────────

/// Runs the WSCPUTest ROM (FluBBaOfWard/WSCPUTest) and checks for a passing
/// result.
///
/// # ROM output format (to be confirmed)
///
/// Set `WS_CPU_TEST_ROM` to the path of the `.ws` ROM file.
///
/// Expected result convention (placeholder — update once confirmed):
/// - The ROM runs self-tests and writes 0x00 to WRAM address 0x0000 on full
///   pass, or a non-zero error code on failure.
/// - Execution ends with a HLT instruction.
///
/// Run with: `WS_CPU_TEST_ROM=/path/to/WSCPUTest.ws cargo test -p swanium-core
///   --test public_roms -- --include-ignored wscputest`
#[test]
#[ignore = "requires WSCPUTest.ws; set WS_CPU_TEST_ROM=/path/to/WSCPUTest.ws"]
fn wscputest_all_tests_pass() {
    let path =
        std::env::var("WS_CPU_TEST_ROM").expect("WS_CPU_TEST_ROM must point to WSCPUTest.ws");
    let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));

    // Run for up to ~60 frames worth of cycles at 3.072 MHz (≈ 3_000_000 / 75).
    let max_cycles = 3_072_000u64 * 10; // 10 seconds of simulated time
    let (cpu, bus) = boot_rom(rom, max_cycles);

    // TODO: update this check once the WSCPUTest output convention is confirmed.
    // Current placeholder: CPU must have halted (test finished) and WRAM[0] = 0 (pass).
    assert!(
        cpu.halted,
        "WSCPUTest did not reach HLT within the cycle budget — \
         test may not have completed or requires more cycles"
    );
    assert_eq!(
        bus.read_u8(0x0000),
        0x00,
        "WSCPUTest result at WRAM[0x0000] is non-zero (failure code); \
         confirm the actual output address/format with the ROM source"
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
    let (cpu, bus) = boot_rom(rom, max_cycles);

    // TODO: confirm the ws-test-suite output convention and update accordingly.
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
