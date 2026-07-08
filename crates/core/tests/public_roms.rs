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

const DEFAULT_WS_CPU_TEST_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/WSCpuTest/WSCpuTest.wsc";
const WSC_CPU_TEST_MAX_FRAMES: usize = 75 * 180;
const WSC_CPU_TEST_BACKGROUND_MAP: u32 = 0x1000;
const WSC_CPU_TEST_TILEMAP_WIDTH: usize = 32;
const WSC_CPU_TEST_TILEMAP_HEIGHT: usize = 32;
const WSC_CPU_TEST_TILEMAP_STRIDE_BYTES: u32 = 64;
const WSC_CPU_TEST_IS_TESTING_ADDR: u32 = 0x0136;
const DEFAULT_WS_TEST_SUITE_80186_QUIRKS_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/mono/cpu/80186_quirks.ws";
const WS_TEST_SUITE_MAX_FRAMES: usize = 120;
const WS_TEST_SUITE_SCREEN_1: u32 = 0x1800;
const WS_TEST_SUITE_TILEMAP_STRIDE_BYTES: u32 = 64;
const WS_TEST_SUITE_PASS_TILE: u8 = 5;
const WS_TEST_SUITE_FAIL_TILE: u8 = 6;
const DEFAULT_WS_TIMING_TEST_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/WSTimingTest/timingtest.ws";
const WS_TIMING_TEST_BACKGROUND_MAP: u32 = 0x1800;
const WS_TIMING_TEST_TILEMAP_STRIDE_BYTES: u32 = 64;
const WS_TIMING_TEST_PASS_X: u32 = 24;
const WS_TIMING_TEST_PASS_TILE: u8 = b'o';
const WS_TIMING_TEST_FAIL_TILE: u8 = b'x';
const WS_TIMING_TEST_MAX_FRAMES_PER_PAGE: usize = 180;
const DEFAULT_WS_HW_TEST_ROM: &str = "/Volumes/CrucialX6/roms/WonderSwan/Tests/WSHWTest.wsc";
const WS_HW_TEST_MAX_FRAMES: usize = 75 * 60;
const WS_HW_TEST_BACKGROUND_MAP: u32 = 0x1000;
const WS_HW_TEST_ROM_LOAD_OFFSET: usize = 0x40000;
const WS_HW_TEST_MAPPED_ROM_SIZE: usize = 0x100000;

const WS_TIMING_TEST_PAGE_ROWS: &[(usize, &[usize])] = &[(
    0,
    &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17],
)];

fn rom_path_from_env_or_default(env_var: &str, default_path: &str) -> PathBuf {
    std::env::var_os(env_var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default_path))
}

fn read_rom(path: &Path, env_var: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}; set {env_var}=<rom path>",
            path.display(),
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

fn timing_test_pass_marker(system: &System, row: usize) -> u8 {
    let addr = WS_TIMING_TEST_BACKGROUND_MAP
        + row as u32 * WS_TIMING_TEST_TILEMAP_STRIDE_BYTES
        + WS_TIMING_TEST_PASS_X * 2;
    system.read_memory_at(addr)
}

fn run_wstimingtest_page(system: &mut System, page: usize, rows: &[usize]) -> Vec<u8> {
    for _ in 0..page {
        system.run_frame(KeyState::X2);
        system.run_frame(KeyState::NONE);
    }

    let mut markers = vec![0; rows.len()];
    for _ in 0..WS_TIMING_TEST_MAX_FRAMES_PER_PAGE {
        system.run_frame(KeyState::NONE);
        for (marker, &row) in markers.iter_mut().zip(rows) {
            *marker = timing_test_pass_marker(system, row);
        }
        if markers
            .iter()
            .all(|&tile| tile == WS_TIMING_TEST_PASS_TILE || tile == WS_TIMING_TEST_FAIL_TILE)
        {
            break;
        }
    }
    markers
}

fn run_wshwtest_all_until_result(rom: Vec<u8>) -> (System, String) {
    let mut system = System::from_rom(rom);
    system.set_model(HardwareModel::Color);

    for _ in 0..8 {
        system.run_frame(KeyState::NONE);
    }
    // WSHWTest starts with "ShowStartup Registers" selected; move once to
    // "Test All", release, then press A to run it.
    system.run_frame(KeyState::X3);
    system.run_frame(KeyState::NONE);
    system.run_frame(KeyState::A);
    system.run_frame(KeyState::NONE);

    let mut latest_text = String::new();
    for _ in 0..WS_HW_TEST_MAX_FRAMES {
        system.run_frame(KeyState::NONE);
        latest_text = tilemap_text(
            &system,
            WS_HW_TEST_BACKGROUND_MAP,
            WSC_CPU_TEST_TILEMAP_HEIGHT,
        );
        if latest_text.contains("Failed!") {
            break;
        }
        if latest_text.contains("Sound Noise Values")
            && (latest_text.contains("Ok!") || latest_text.contains("Done."))
        {
            break;
        }
    }

    (system, latest_text)
}

fn map_wshwtest_rom_for_direct_boot(rom: Vec<u8>) -> Vec<u8> {
    if rom.len() >= WS_HW_TEST_MAPPED_ROM_SIZE {
        return rom;
    }
    let mut mapped = vec![0x00; WS_HW_TEST_MAPPED_ROM_SIZE];
    let end = WS_HW_TEST_ROM_LOAD_OFFSET + rom.len();
    assert!(
        end <= mapped.len(),
        "WSHWTest ROM is too large to map at 0x{WS_HW_TEST_ROM_LOAD_OFFSET:05X}: {} bytes",
        rom.len()
    );
    mapped[WS_HW_TEST_ROM_LOAD_OFFSET..end].copy_from_slice(&rom);
    mapped
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
/// `/Volumes/CrucialX6/roms/WonderSwan/Tests/WSCpuTest/WSCpuTest.wsc` or set
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
#[ignore = "requires WSCpuTest.wsc; default path is /Volumes/CrucialX6/roms/WonderSwan/Tests/WSCpuTest/WSCpuTest.wsc"]
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
/// `/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/mono/cpu/80186_quirks.ws`
/// or set `WS_TEST_SUITE_ROM` to that ROM path.
///
/// Run with: `WS_TEST_SUITE_ROM=/path/to/test.ws cargo test -p swanium-core
///   --test public_roms -- --include-ignored ws_test_suite`
#[test]
#[ignore = "requires ws-test-suite mono/cpu/80186_quirks.ws; default path is /Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/mono/cpu/80186_quirks.ws"]
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

// ── WSTimingTest (FluBBaOfWard) ──────────────────────────────────────────────

/// Runs selected pages from FluBBaOfWard/WSTimingTest.
///
/// WSTimingTest measures V30MZ instruction timing by running each test loop
/// 1000 times and displaying the expected scanline count, actual scanline
/// count, and a pass marker. Its README notes that measured values can differ
/// by one scanline on hardware in some circumstances; this test follows the
/// ROM's own `okfail` result instead of reimplementing tolerance logic.
///
/// Source-confirmed output protocol:
///
/// - `timingtest.asm` defines `backgroundMap = WS_TILE_BANK - MAP_SIZE`, i.e.
///   WRAM `0x1800`.
/// - `runtest` receives a row number, prints the test at that row, and calls
///   `okfail`.
/// - `okfail` writes ASCII `o` for pass or `x` for fail at byte offset
///   `row * 64 + 48`, i.e. tile-map column 24.
/// - The program starts on page 0; X2 increments the page counter.
///
/// Build with `nasm -f bin -o timingtest.ws timingtest.asm` from
/// FluBBaOfWard/WSTimingTest, then place it at the default path or set
/// `WS_TIMING_TEST_ROM`.
#[test]
#[ignore = "requires WSTimingTest timingtest.ws; default path is /Volumes/CrucialX6/roms/WonderSwan/Tests/WSTimingTest/timingtest.ws"]
fn wstimingtest_selected_pages_pass() {
    let path = rom_path_from_env_or_default("WS_TIMING_TEST_ROM", DEFAULT_WS_TIMING_TEST_ROM);
    let rom = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}; set WS_TIMING_TEST_ROM=<rom path> or place the ROM at {}",
            path.display(),
            DEFAULT_WS_TIMING_TEST_ROM
        )
    });

    for &(page, rows) in WS_TIMING_TEST_PAGE_ROWS {
        let mut system = System::from_rom(rom.clone());
        system.set_model(HardwareModel::Color);
        let markers = run_wstimingtest_page(&mut system, page, rows);
        let visible_text = tilemap_text(&system, WS_TIMING_TEST_BACKGROUND_MAP, 18);

        assert!(
            !markers.contains(&WS_TIMING_TEST_FAIL_TILE),
            "WSTimingTest page {page} reported failure markers {markers:?}; visible text:\n{visible_text}"
        );
        assert!(
            markers.iter().all(|&tile| tile == WS_TIMING_TEST_PASS_TILE),
            "WSTimingTest page {page} did not finish within {WS_TIMING_TEST_MAX_FRAMES_PER_PAGE} frames; \
             markers={markers:?}; visible text:\n{visible_text}"
        );
    }
}

// ── WSHWTest (FluBBaOfWard) ──────────────────────────────────────────────────

/// Runs FluBBaOfWard/WSHWTest's menu item "Test All".
///
/// The ROM is menu-driven: it starts on "ShowStartup Registers", X3 moves the
/// selection down to "Test All", and A starts the selected item. The text is
/// emitted through INT 0x10 into the background tile map at WRAM `0x1000`
/// (`backgroundMap = WS_TILE_BANK - MAP_SIZE - MAP_SIZE`). This test treats
/// `Failed!` as a hard failure and waits until the run reaches the final
/// "Sound Noise Values" section with an `Ok!`/`Done.` marker.
///
/// Build with `nasm -f bin -o WSHWTest.wsc WSHWTest.asm` from
/// FluBBaOfWard/WSHWTest, then place it at the default path or set
/// `WS_HW_TEST_ROM`.
#[test]
#[ignore = "requires WSHWTest.wsc; default path is /Volumes/CrucialX6/roms/WonderSwan/Tests/WSHWTest.wsc"]
fn wshwtest_all_tests_pass() {
    let path = rom_path_from_env_or_default("WS_HW_TEST_ROM", DEFAULT_WS_HW_TEST_ROM);
    let rom = map_wshwtest_rom_for_direct_boot(read_rom(&path, "WS_HW_TEST_ROM"));
    let (system, text) = run_wshwtest_all_until_result(rom);

    assert!(
        !text.contains("Failed!"),
        "WSHWTest reported failure; visible background text:\n{text}"
    );
    assert!(
        text.contains("Sound Noise Values") && (text.contains("Ok!") || text.contains("Done.")),
        "WSHWTest did not reach the expected completion marker within {WS_HW_TEST_MAX_FRAMES} frames; \
         cpu_halted={}, visible background text:\n{text}",
        system.cpu().halted
    );
}
