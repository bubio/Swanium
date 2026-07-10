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
const DEFAULT_WS_TEST_SUITE_PREFIXES_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/mono/cpu/prefixes.ws";
const DEFAULT_WS_TEST_SUITE_SOC_INTERRUPTS_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/mono/soc/interrupts.ws";
const DEFAULT_WS_TEST_SUITE_INTERRUPT_TIMING_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/mono/cpu/interrupt_timing.ws";
const DEFAULT_WS_TEST_SUITE_MONO_PALETTES_WRITEMASK_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/mono/display/mono_palettes_writemask.ws";
const DEFAULT_WS_TEST_SUITE_GDMA_ALIGNMENT_ACCESS_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/color/dma/alignment_access.wsc";
const DEFAULT_WS_TEST_SUITE_LIBC_STRLEN_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/wonderful/libc/strlen.ws";
const DEFAULT_WS_TEST_SUITE_LIBC_STRCHR_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/wonderful/libc/strchr.ws";
const DEFAULT_WS_TEST_SUITE_LIBC_MEMSET_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/wonderful/libc/memset.ws";
const DEFAULT_WS_TEST_SUITE_LIBC_MEMCMP_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/wonderful/libc/memcmp.ws";
const DEFAULT_WS_TEST_SUITE_LIBC_MEMCPY_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/wonderful/libc/memcpy.ws";
const DEFAULT_WS_TEST_SUITE_LIBC_MEMCCPY_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/wonderful/libc/memccpy.ws";
const DEFAULT_WS_TEST_SUITE_LIBC_SETJMP_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/wonderful/libc/setjmp.ws";
const DEFAULT_WS_TEST_SUITE_LIBC_INITFINI_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/wonderful/libc/initfini.ws";
const DEFAULT_WS_TEST_SUITE_LIBC_MALLOC_ROM: &str =
    "/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/wonderful/libc/malloc.ws";
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

const WS_TIMING_TEST_PAGE_ROWS: &[(usize, &[usize])] = &[
    (
        0,
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17],
    ),
    (1, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (2, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (3, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (4, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (5, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (6, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (7, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (
        8,
        &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17],
    ),
    (9, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (10, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (11, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (12, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (13, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (14, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (15, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (16, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]),
    (17, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (18, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
    (19, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]),
    (20, &[1, 2, 3, 4, 5, 6, 7, 8]),
    (21, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]),
    (22, &[1, 2, 3, 4, 5, 6, 7, 8, 9]),
    (23, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]),
    (24, &[1, 2, 3, 4, 5, 6]),
    (25, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]),
    (26, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]),
    (27, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]),
    (28, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
];

struct WsTestSuitePassFailCase {
    name: &'static str,
    env_var: &'static str,
    default_path: &'static str,
    model: HardwareModel,
    marker_ranges: &'static [(usize, usize)],
    source_protocol: &'static str,
}

const WS_TEST_SUITE_80186_QUIRKS_MARKER_RANGES: &[(usize, usize)] = &[(0, 0), (1, 0), (2, 0)];
const WS_TEST_SUITE_PREFIXES_MARKER_RANGES: &[(usize, usize)] =
    &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0), (6, 0)];
const WS_TEST_SUITE_SOC_INTERRUPTS_MARKER_RANGES: &[(usize, usize)] = &[(0, 7), (1, 4)];
const WS_TEST_SUITE_INTERRUPT_TIMING_MARKER_RANGES: &[(usize, usize)] = &[
    (0, 0),
    (1, 0),
    (2, 0),
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 0),
    (12, 0),
    (13, 0),
    (14, 0),
];
const WS_TEST_SUITE_MONO_PALETTES_WRITEMASK_MARKER_RANGES: &[(usize, usize)] = &[
    (0, 1),
    (1, 1),
    (2, 1),
    (3, 1),
    (4, 1),
    (5, 1),
    (6, 1),
    (7, 1),
    (8, 1),
    (9, 1),
    (10, 1),
    (11, 1),
    (12, 1),
    (13, 1),
    (14, 1),
    (15, 1),
];
const WS_TEST_SUITE_GDMA_ALIGNMENT_ACCESS_MARKER_RANGES: &[(usize, usize)] =
    &[(0, 2), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0)];
const WS_TEST_SUITE_LIBC_STRLEN_MARKER_RANGES: &[(usize, usize)] = &[(0, 1)];
const WS_TEST_SUITE_LIBC_STRCHR_MARKER_RANGES: &[(usize, usize)] = &[(0, 4)];
const WS_TEST_SUITE_LIBC_MEMSET_MARKER_RANGES: &[(usize, usize)] = &[(0, 6), (1, 6)];
const WS_TEST_SUITE_LIBC_MEMCMP_MARKER_RANGES: &[(usize, usize)] = &[(0, 4), (1, 4)];
const WS_TEST_SUITE_LIBC_MEMCPY_MARKER_RANGES: &[(usize, usize)] =
    &[(0, 5), (1, 5), (2, 5), (3, 5)];
const WS_TEST_SUITE_LIBC_MEMCCPY_MARKER_RANGES: &[(usize, usize)] = &[(0, 7)];
const WS_TEST_SUITE_LIBC_SETJMP_MARKER_RANGES: &[(usize, usize)] = &[(0, 0), (1, 0), (2, 0)];
const WS_TEST_SUITE_LIBC_INITFINI_MARKER_RANGES: &[(usize, usize)] = &[(0, 0)];
const WS_TEST_SUITE_LIBC_MALLOC_MARKER_RANGES: &[(usize, usize)] =
    &[(0, 1), (1, 1), (2, 1), (3, 0)];

const WS_TEST_SUITE_PASS_FAIL_CASES: &[WsTestSuitePassFailCase] = &[
    WsTestSuitePassFailCase {
        name: "mono/cpu/80186_quirks.ws",
        env_var: "WS_TEST_SUITE_80186_QUIRKS_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_80186_QUIRKS_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_80186_QUIRKS_MARKER_RANGES,
        source_protocol: "`src/mono/cpu/80186_quirks/main.c` calls `draw_pass_fail` \
            three times at rows 0-2 with offset 0.",
    },
    WsTestSuitePassFailCase {
        name: "mono/cpu/prefixes.ws",
        env_var: "WS_TEST_SUITE_PREFIXES_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_PREFIXES_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_PREFIXES_MARKER_RANGES,
        source_protocol: "`src/mono/cpu/prefixes/main.c` writes six direct \
            prefix/string markers at rows 0-5 and calls `draw_pass_fail` for \
            `REP:ESx8:MOVSB(IRQ)` on row 6; all use offset 0.",
    },
    WsTestSuitePassFailCase {
        name: "mono/soc/interrupts.ws",
        env_var: "WS_TEST_SUITE_SOC_INTERRUPTS_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_SOC_INTERRUPTS_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_SOC_INTERRUPTS_MARKER_RANGES,
        source_protocol: "`src/mono/soc/interrupts/main.c` has two rows of \
            `draw_pass_fail`: row 0 uses offsets 7-0 and row 1 uses offsets 4-0.",
    },
    WsTestSuitePassFailCase {
        name: "mono/cpu/interrupt_timing.ws",
        env_var: "WS_TEST_SUITE_INTERRUPT_TIMING_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_INTERRUPT_TIMING_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_INTERRUPT_TIMING_MARKER_RANGES,
        source_protocol: "`src/mono/cpu/interrupt_timing/main.c` expands \
            `IP_STORE_TEST_CALL` fifteen times; each call uses \
            `draw_pass_fail(i++, 0, ...)`.",
    },
    WsTestSuitePassFailCase {
        name: "mono/display/mono_palettes_writemask.ws",
        env_var: "WS_TEST_SUITE_MONO_PALETTES_WRITEMASK_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_MONO_PALETTES_WRITEMASK_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_MONO_PALETTES_WRITEMASK_MARKER_RANGES,
        source_protocol: "`src/mono/display/mono_palettes_writemask/main.c` \
            loops over rows 0-15 and calls `draw_pass_fail` with offsets 1 \
            and 0 for each mono palette register.",
    },
    WsTestSuitePassFailCase {
        name: "color/dma/alignment_access.wsc",
        env_var: "WS_TEST_SUITE_GDMA_ALIGNMENT_ACCESS_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_GDMA_ALIGNMENT_ACCESS_ROM,
        model: HardwareModel::Color,
        marker_ranges: WS_TEST_SUITE_GDMA_ALIGNMENT_ACCESS_MARKER_RANGES,
        source_protocol: "`src/color/dma/alignment_access/main.c` uses \
            `draw_pass_fail` on row 0 with offsets 2-0, then rows 1-5 with \
            offset 0 for GDMA register masks and source-access cases.",
    },
    WsTestSuitePassFailCase {
        name: "wonderful/libc/strlen.ws",
        env_var: "WS_TEST_SUITE_LIBC_STRLEN_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_LIBC_STRLEN_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_LIBC_STRLEN_MARKER_RANGES,
        source_protocol: "`src/wonderful/libc/strlen/main.c` uses \
            `draw_pass_fail` on row 0 with offsets 1-0.",
    },
    WsTestSuitePassFailCase {
        name: "wonderful/libc/strchr.ws",
        env_var: "WS_TEST_SUITE_LIBC_STRCHR_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_LIBC_STRCHR_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_LIBC_STRCHR_MARKER_RANGES,
        source_protocol: "`src/wonderful/libc/strchr/main.c` uses \
            `draw_pass_fail` on row 0 with offsets 4-0.",
    },
    WsTestSuitePassFailCase {
        name: "wonderful/libc/memset.ws",
        env_var: "WS_TEST_SUITE_LIBC_MEMSET_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_LIBC_MEMSET_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_LIBC_MEMSET_MARKER_RANGES,
        source_protocol: "`src/wonderful/libc/memset/main.c` uses \
            `draw_pass_fail` on rows 0-1 with offsets 6-0.",
    },
    WsTestSuitePassFailCase {
        name: "wonderful/libc/memcmp.ws",
        env_var: "WS_TEST_SUITE_LIBC_MEMCMP_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_LIBC_MEMCMP_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_LIBC_MEMCMP_MARKER_RANGES,
        source_protocol: "`src/wonderful/libc/memcmp/main.c` uses \
            `draw_pass_fail` on rows 0-1 with offsets 4-0.",
    },
    WsTestSuitePassFailCase {
        name: "wonderful/libc/memcpy.ws",
        env_var: "WS_TEST_SUITE_LIBC_MEMCPY_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_LIBC_MEMCPY_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_LIBC_MEMCPY_MARKER_RANGES,
        source_protocol: "`src/wonderful/libc/memcpy/main.c` uses \
            `draw_pass_fail` on rows 0-3 with offsets 5-0.",
    },
    WsTestSuitePassFailCase {
        name: "wonderful/libc/memccpy.ws",
        env_var: "WS_TEST_SUITE_LIBC_MEMCCPY_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_LIBC_MEMCCPY_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_LIBC_MEMCCPY_MARKER_RANGES,
        source_protocol: "`src/wonderful/libc/memccpy/main.c` uses \
            `draw_pass_fail` on row 0 with offsets 7-0.",
    },
    WsTestSuitePassFailCase {
        name: "wonderful/libc/setjmp.ws",
        env_var: "WS_TEST_SUITE_LIBC_SETJMP_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_LIBC_SETJMP_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_LIBC_SETJMP_MARKER_RANGES,
        source_protocol: "`src/wonderful/libc/setjmp/main.c` writes pass \
            markers for setjmp return values 0, 1, and 2 on rows 0-2.",
    },
    WsTestSuitePassFailCase {
        name: "wonderful/libc/initfini.ws",
        env_var: "WS_TEST_SUITE_LIBC_INITFINI_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_LIBC_INITFINI_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_LIBC_INITFINI_MARKER_RANGES,
        source_protocol: "`src/wonderful/libc/initfini/main.c` uses \
            `draw_pass_fail` on row 0 with offset 0.",
    },
    WsTestSuitePassFailCase {
        name: "wonderful/libc/malloc.ws",
        env_var: "WS_TEST_SUITE_LIBC_MALLOC_ROM",
        default_path: DEFAULT_WS_TEST_SUITE_LIBC_MALLOC_ROM,
        model: HardwareModel::Mono,
        marker_ranges: WS_TEST_SUITE_LIBC_MALLOC_MARKER_RANGES,
        source_protocol: "`src/wonderful/libc/malloc/main.c` uses \
            `draw_pass_fail` on rows 0-2 with offsets 1-0 and row 3 with \
            offset 0 after the oversized allocation check.",
    },
];

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

fn read_ws_test_suite_markers(system: &System, marker_ranges: &[(usize, usize)]) -> Vec<u8> {
    marker_ranges
        .iter()
        .flat_map(|&(row, max_offset)| {
            (0..=max_offset).rev().map(move |offset| {
                let x = 27 - offset;
                let addr = WS_TEST_SUITE_SCREEN_1
                    + row as u32 * WS_TEST_SUITE_TILEMAP_STRIDE_BYTES
                    + x as u32 * 2;
                system.read_memory_at(addr)
            })
        })
        .collect()
}

fn run_ws_test_suite_pass_fail_case(case: &WsTestSuitePassFailCase) {
    let path = rom_path_from_env_or_default(case.env_var, case.default_path);
    let rom = read_rom(&path, case.env_var);
    let mut system = System::from_rom(rom);
    system.set_model(case.model);

    let marker_count = case
        .marker_ranges
        .iter()
        .map(|&(_, max_offset)| max_offset + 1)
        .sum();
    let mut markers = vec![0u8; marker_count];
    for _ in 0..WS_TEST_SUITE_MAX_FRAMES {
        system.run_frame(KeyState::NONE);
        markers = read_ws_test_suite_markers(&system, case.marker_ranges);
        if markers
            .iter()
            .all(|&tile| tile == WS_TEST_SUITE_PASS_TILE || tile == WS_TEST_SUITE_FAIL_TILE)
        {
            break;
        }
    }

    let rows = case
        .marker_ranges
        .iter()
        .map(|&(row, _)| row + 1)
        .max()
        .unwrap_or(1);
    let visible_text = tilemap_text(&system, WS_TEST_SUITE_SCREEN_1, rows);
    assert!(
        !markers.contains(&WS_TEST_SUITE_FAIL_TILE),
        "ws-test-suite {} reported failure markers {:?}; source protocol: {}; \
         dma/system ports: 40={:02X} 42={:02X} 46={:02X} 47={:02X} 48={:02X} A0={:02X}; \
         visible text:\n{}",
        case.name,
        markers,
        case.source_protocol,
        system.bus().peek_io(0x40),
        system.bus().peek_io(0x42),
        system.bus().peek_io(0x46),
        system.bus().peek_io(0x47),
        system.bus().peek_io(0x48),
        system.bus().peek_io(0xA0),
        visible_text
    );
    assert!(
        markers.iter().all(|&tile| tile == WS_TEST_SUITE_PASS_TILE),
        "ws-test-suite {} did not produce all pass markers within {} frames; \
         markers={:?}; source protocol: {}; visible text:\n{}",
        case.name,
        WS_TEST_SUITE_MAX_FRAMES,
        markers,
        case.source_protocol,
        visible_text
    );
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

/// Runs source-confirmed pass/fail marker ROMs from ws-test-suite
/// (asiekierka/ws-test-suite).
///
/// # ROM output format
///
/// All cases here use the upstream `common/test/pass_fail.h` protocol. Their
/// source defines `screen_1` at WRAM `0x1800`; `draw_pass_fail(y, offset,
/// result)` writes tile 5 for pass and tile 6 for fail at `(x=27-offset, y)`.
/// Unknown ws-test-suite ROM protocols are intentionally not decoded by this
/// test.
#[test]
#[ignore = "requires ws-test-suite ROMs under /Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite"]
fn ws_test_suite_pass_fail_roms_pass() {
    for case in WS_TEST_SUITE_PASS_FAIL_CASES {
        run_ws_test_suite_pass_fail_case(case);
    }
}

// ── WSTimingTest (FluBBaOfWard) ──────────────────────────────────────────────

/// Runs all source-confirmed pages from FluBBaOfWard/WSTimingTest.
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
/// `testcalls.asm` defines pages 0 through 28. Each tuple in
/// `WS_TIMING_TEST_PAGE_ROWS` records the rows that page executes.
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
