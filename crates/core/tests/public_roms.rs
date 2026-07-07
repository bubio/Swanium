//! Opt-in integration tests for public WonderSwan CPU test ROMs.
//!
//! These tests are marked `#[ignore]` by default because the ROM files are not
//! committed to the repository (licensing constraints).  To run them:
//!
//! 1. Build/download the ROM(s) from the sources listed in `tests/README.md`.
//! 2. Place them at the shared local paths below, or set the relevant
//!    environment variable to override the path.
//! 3. Run with the `--include-ignored` flag:
//!
//! ```sh
//! WS_CPU_TEST_ROM=/path/to/WSCpuTest.wsc \
//!     cargo test -p swanium-core --test public_roms -- --include-ignored
//! ```

use std::path::{Path, PathBuf};

use swanium_core::keypad::KeyState;
use swanium_core::model::HardwareModel;
use swanium_core::system::System;

// ── Harness ──────────────────────────────────────────────────────────────────

const DEFAULT_WS_CPU_TEST_ROM: &str = "/Volumes/CrucialX6/roms/WonderSwan/WSCpuTest/WSCpuTest.wsc";
const WSC_CPU_TEST_MAX_FRAMES: usize = 75 * 180;
const WSC_CPU_TEST_BACKGROUND_MAP: u32 = 0x1000;
const WSC_CPU_TEST_TILEMAP_WIDTH: usize = 32;
const WSC_CPU_TEST_TILEMAP_HEIGHT: usize = 32;
const WSC_CPU_TEST_TILEMAP_STRIDE_BYTES: u32 = 64;
const WSC_CPU_TEST_IS_TESTING_ADDR: u32 = 0x0136;
const DEFAULT_WS_TEST_SUITE_80186_QUIRKS_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/ws-test-suite/mono/cpu/80186_quirks.ws";
const WS_TEST_SUITE_MAX_FRAMES: usize = 120;
const WS_TEST_SUITE_SCREEN_1: u32 = 0x1800;
const WS_TEST_SUITE_TILEMAP_STRIDE_BYTES: u32 = 64;
const WS_TEST_SUITE_PASS_TILE: u8 = 5;
const WS_TEST_SUITE_FAIL_TILE: u8 = 6;

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

fn tilemap_text(system: &System, base: u32, rows: usize) -> String {
    let mut text = String::with_capacity((WSC_CPU_TEST_TILEMAP_WIDTH + 1) * rows);
    for y in 0..rows {
        for x in 0..WSC_CPU_TEST_TILEMAP_WIDTH {
            let addr = base + y as u32 * WS_TEST_SUITE_TILEMAP_STRIDE_BYTES + x as u32 * 2;
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
/// v0.7.1 source, then place it at
/// `/Volumes/CrucialX6/roms/WonderSwan/WSCpuTest/WSCpuTest.wsc` or set
/// `WS_CPU_TEST_ROM` to the `.wsc` path.
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
#[ignore = "requires WSCpuTest.wsc; default path is /Volumes/CrucialX6/roms/WonderSwan/WSCpuTest/WSCpuTest.wsc"]
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
/// The first decoded oracle is `mono/cpu/80186_quirks.ws`, built from
/// asiekierka/ws-test-suite `src/mono/cpu/80186_quirks`. That source defines
/// `screen_1` in WRAM section `.iramcx_1800`; `draw_pass_fail(y, offset, result)`
/// writes tile 5 for pass and tile 6 for fail at `(x=27-offset, y)`. The test
/// has three checks, all at offset 0, so rows 0–2 at tile-map x=27 must be tile
/// 5 and must not be tile 6.
///
/// Place it at
/// `/Volumes/CrucialX6/roms/WonderSwan/ws-test-suite/mono/cpu/80186_quirks.ws`
/// or set `WS_TEST_SUITE_ROM` to that ROM path.
///
/// Run with: `WS_TEST_SUITE_ROM=/path/to/test.ws cargo test -p swanium-core
///   --test public_roms -- --include-ignored ws_test_suite`
#[test]
#[ignore = "requires ws-test-suite mono/cpu/80186_quirks.ws; default path is /Volumes/CrucialX6/roms/WonderSwan/ws-test-suite/mono/cpu/80186_quirks.ws"]
fn ws_test_suite_rom_passes() {
    let path =
        rom_path_from_env_or_default("WS_TEST_SUITE_ROM", DEFAULT_WS_TEST_SUITE_80186_QUIRKS_ROM);
    let path_text = path.to_string_lossy();
    assert!(
        path_text.ends_with("80186_quirks.ws"),
        "only ws-test-suite mono/cpu/80186_quirks.ws has a decoded oracle; got {}",
        path.display()
    );
    let rom = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}; set WS_TEST_SUITE_ROM=<rom path> or place the ROM at {}",
            path.display(),
            DEFAULT_WS_TEST_SUITE_80186_QUIRKS_ROM
        )
    });

    let mut system = System::from_rom(rom);
    system.set_model(HardwareModel::Mono);

    let mut markers = [0u8; 3];
    for _ in 0..WS_TEST_SUITE_MAX_FRAMES {
        system.run_frame(KeyState::NONE);
        for (row, marker) in markers.iter_mut().enumerate() {
            let addr =
                WS_TEST_SUITE_SCREEN_1 + row as u32 * WS_TEST_SUITE_TILEMAP_STRIDE_BYTES + 27 * 2;
            *marker = system.read_memory_at(addr);
        }
        if markers
            .iter()
            .all(|&tile| tile == WS_TEST_SUITE_PASS_TILE || tile == WS_TEST_SUITE_FAIL_TILE)
        {
            break;
        }
    }

    let visible_text = tilemap_text(&system, WS_TEST_SUITE_SCREEN_1, 4);
    assert!(
        !markers.contains(&WS_TEST_SUITE_FAIL_TILE),
        "ws-test-suite 80186_quirks reported failure markers {markers:?}; visible text:\n{visible_text}"
    );
    assert_eq!(
        markers, [WS_TEST_SUITE_PASS_TILE; 3],
        "ws-test-suite 80186_quirks did not produce all pass markers within \
         {WS_TEST_SUITE_MAX_FRAMES} frames; markers={markers:?}; visible text:\n{visible_text}"
    );
}
