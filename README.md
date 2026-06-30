# Swanium

> **Work in progress — a ROM boots, renders, and plays sound, but accuracy work remains.**

A cycle-accurate WonderSwan / WonderSwan Color emulator written in Rust.

## Status

The emulator core (CPU, memory, interrupts, timers, DMA, PPU, APU, cartridge)
is implemented and a minimal Slint frontend can load a `.ws` ROM, display the
picture, play audio through cpal, and accept keyboard and gamepad input.
**443 tests pass** across the workspace.

| Phase | Description | Status |
|-------|-------------|--------|
| 0 | Cargo workspace bootstrap | ✅ Complete |
| 1 | V30MZ CPU core (full 8086/80186-class instruction set) | ✅ Complete |
| 2 | Memory map, interrupts, timers, DMA (GDMA/SDMA), port I/O | ✅ Complete |
| 3 | CPU test-ROM harness + public-ROM opt-in policy | ✅ Complete |
| 4 | PPU (monochrome tile/sprite scanline renderer) | ✅ Complete |
| 5 | APU (4 wave-table channels + voice/sweep/noise) | ✅ Complete |
| 6 | Cartridge header, mapper banking, EEPROM, save data | ✅ Complete |
| 7 | Frontend: Slint window, framebuffer, keyboard, cpal audio | 🚧 In progress |
| 8 | WonderSwan Color extensions (color palettes, RTC) | 🔲 Not started |

### Phase 7 remaining follow-ups

- ✅ **Config persistence** — TOML load/save of `Config` (serde + toml); the
  frontend loads `~/.config/swanium/config.toml` (platform-dependent) at startup.
- ✅ **gilrs gamepad input** — `input::gamepad::Gamepad` polls gilrs and folds
  the controller state into the key matrix each frame (D-pad/left stick → X-pad,
  right stick → Y-pad, face buttons → A/B, menu → Start). OR-combined with the
  keyboard; a missing controller is non-fatal.
- **UI**: in-app ROM file picker, start/pause, settings & key-binding screens.
- High-quality scaling / shader post-processing (deferred to Phase 9).

### Known issues

- **Background tilemap update timing** (e.g. Lode Runner story screens): SCR1
  text bleeds through SCR2 on a 48 px vertical period. Layer compositing itself
  is correct; the cause is traced to GDMA / tilemap-update / frame-driver timing
  and is deferred to the cycle-accuracy hardening phase. Debug aids: the `P` key
  in the frontend dumps display registers (`System::run_frame_traced`,
  `Bus::peek_io`, `Bus::debug_bg_sample`).

See [`docs/dev/DevelopmentPlan.md`](docs/dev/DevelopmentPlan.md) for the full roadmap.

## Building

Requires Rust stable (see `rust-toolchain.toml`).

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Running

```sh
cargo run -p frontend -- path/to/game.ws
```

Default keyboard controls: arrow keys = X-pad, `WASD` = Y-pad, `Z` = B, `X` = A,
`Enter` = Start. Press `P` to dump PPU display registers to stderr.

A connected gamepad works too (auto-detected via gilrs): D-pad / left stick =
X-pad, right stick = Y-pad, bottom face button = B, right face button = A, menu
button = Start.

Public test ROMs are never committed to this repository (licensing); see
[`tests/README.md`](tests/README.md).

## Architecture

```
Frontend (Slint) → App → { Audio (cpal), Input (gilrs/keyboard) }
                               ↓
         Emulator Core: CPU · Memory · PPU · APU · DMA · Cartridge
```

The emulator core (`crates/core`, package `swanium-core`) has no dependency on
GUI, audio, or input libraries and can be used headlessly — a requirement for
the planned RetroAchievements (rcheevos) integration.

## License

MIT — see [LICENSE](LICENSE).
