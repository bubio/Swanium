# Compatibility matrix

Last updated: 2026-07-07.

This matrix tracks license-clean compatibility evidence. Public test ROMs are
referenced by source and local path convention only; binaries are not committed.
Commercial ROM checks are manual notes and must not include ROM assets.

## Automated opt-in tests

| Area | ROM / test | Source | Local path / env var | Coverage | Expected result | Current result | Notes |
|---|---|---|---|---|---|---|---|
| CPU | WSCpuTest v0.7.1 `Test All` | FluBBaOfWard/WSCpuTest | `WS_CPU_TEST_ROM` or `tests/fixtures/cpu/public/WSCpuTest.wsc` | V30MZ instruction behavior | Background tile map prints `Ok!`; no `Failed!` | Pass when the opt-in ROM is provided | Test injects A to start the default menu item and decodes the BG tile map. |
| CPU | `mono/cpu/80186_quirks.ws` | asiekierka/ws-test-suite | `WS_TEST_SUITE_ROM` or `tests/fixtures/cpu/public/ws-test-suite/mono/cpu/80186_quirks.ws` | AAM/AAD non-10 immediates, SALC | WRAM `screen_1` pass/fail markers are tile `5,5,5` | Decoded oracle implemented; run is opt-in | Source defines `screen_1` at WRAM `0x1800`; markers are at x=27, rows 0-2. |
| SDMA audio | Internal SDMA unit/integration tests | Swanium self-built tests | `cargo test -p swanium-core sdma` | SDMA register masks, Color gate, voice-latch delivery, terminal count, repeat, hold | All focused SDMA tests pass | Covered by normal test suite | No external ROM fixture selected yet. |
| Mono PPU | Synthetic PPU render tests | Swanium self-built tests | `cargo test -p swanium-core ppu` | Mono tile/sprite/window/palette path | Framebuffer assertions pass | Covered by normal test suite | Public PPU ROM coverage still needs fixture selection. |
| Color PPU | Synthetic Color render tests | Swanium self-built tests | `cargo test -p swanium-core color` | Color palette RAM, 2bpp/4bpp paths, color enable gate | Framebuffer assertions pass | Covered by normal test suite | Low-level color assumptions remain targeted by Milestone 10c. |
| RTC | Cartridge RTC unit tests | Swanium self-built tests | `cargo test -p swanium-core rtc` | BCD clock, command/data protocol, deterministic ticking | Unit tests pass | Covered by normal test suite | Public `rtctest` fixture not selected yet. |
| Mapper/save | Cartridge mapper tests | Swanium self-built tests | `cargo test -p swanium-core cartridge_mapper_tests` | Bandai 2001/2003 banking, SRAM/EEPROM save media | Unit/integration tests pass | Covered by normal test suite | Needs one public or local fixture row per save class in Milestone 12. |

## Manual commercial-ROM smoke checks

| Area | Title / check | Source | Local path / env var | Coverage | Expected result | Current result | Notes |
|---|---|---|---|---|---|---|---|
| SDMA audio | Needs fixture | User-provided commercial ROM | Local only, untracked | Games that stream voice PCM through SDMA | Voice clips audible, no stuck DMA | Not selected | Prefer a public SDMA audio ROM before relying on this row. |
| Mono PPU | Needs fixture | User-provided commercial ROM | Local only, untracked | Mono background/sprite/window rendering | Boots and renders stable gameplay | Not selected | Record screenshots/hashes separately only if license-clean. |
| Color PPU | Final Fantasy / similar WSC title | User-provided commercial ROM | Local only, untracked | Color hardware detection and palette RAM | Boots into color path | Previously smoke-checked | Status.md records FF/Dark Eyes/Dragonball/Digimon Tamers color boot checks. |
| RTC | Needs fixture | User-provided RTC-bearing ROM | Local only, untracked | RTC footer detection and time injection | Time reads are deterministic and plausible | Not selected | Public `rtctest` is preferred for protocol validation. |
| Mapper/save | Needs fixture | User-provided save-bearing ROM | Local only, untracked | SRAM, cart EEPROM, RTC-bearing, no-save classes | Save medium round-trips without mapper regressions | Not selected | Keep save files outside the repository. |
