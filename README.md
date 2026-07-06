# Swanium

> **Work in progress — mono and color ROMs boot, render, and play sound, but accuracy work remains.**

A cycle-accurate WonderSwan / WonderSwan Color emulator written in Rust.

## Status

The emulator core (CPU, memory, interrupts, timers, DMA, PPU, APU, cartridge,
RTC, and WonderSwan Color extensions) is implemented. The Slint frontend can
load `.ws` / `.wsc` ROMs, display the picture, play audio through cpal, and
accept keyboard and gamepad input. **570 tests pass** across the workspace,
with 2 public-ROM compatibility tests kept opt-in because ROM binaries are not
committed.

| Phase | Description | Status |
|-------|-------------|--------|
| 0 | Cargo workspace bootstrap | ✅ Complete |
| 1 | V30MZ CPU core (full 8086/80186-class instruction set) | ✅ Complete |
| 2 | Memory map, interrupts, timers, DMA (GDMA/SDMA), port I/O | ✅ Complete |
| 3 | CPU test-ROM harness + public-ROM opt-in policy | ✅ Complete |
| 4 | PPU (monochrome tile/sprite scanline renderer) | ✅ Complete |
| 5 | APU (4 wave-table channels + voice/sweep/noise) | ✅ Complete |
| 6 | Cartridge header, mapper banking, EEPROM, save data | ✅ Complete |
| 7 | Frontend: Slint window, framebuffer, keyboard/gamepad, cpal audio | 🚧 In progress |
| 8 | WonderSwan Color extensions (color palettes, RTC, HyperVoice) | ✅ Complete |

### Frontend status

- ✅ **Config persistence** — TOML load/save of `Config` (serde + toml); the
  frontend loads `~/.config/swanium/config.toml` (platform-dependent) at startup.
- ✅ **gilrs gamepad input** — `input::gamepad::Gamepad` polls gilrs and folds
  the controller state into the key matrix each frame (D-pad/left stick → X-pad,
  right stick → Y-pad, face buttons → A/B, menu → Start). OR-combined with the
  keyboard; a missing controller is non-fatal.
- ✅ **In-app ROM file picker** — the ROM path is now optional; with no
  argument the window opens empty and the `O` key pops a native open dialog
  (via `rfd`, XDG-portal backend on Linux so no GTK is needed). The picker
  remembers the last directory and swapping ROMs flushes the audio buffer.
- ✅ **Menu bar & status bar** — a Slint `MenuBar` (native macOS menu bar,
  in-window elsewhere) with ROM history, settings, view controls, pause/reset,
  plus a bottom status bar showing the current ROM name, FPS, and volume.
- ✅ **Input settings** — keyboard and controller bindings can be remapped and
  persisted.
- ✅ **About** — macOS uses the OS-standard application menu About item;
  Windows/Linux use the Slint Help ▸ About dialog.
- High-quality scaling / shader post-processing remains deferred.

### Known issues

- Accuracy hardening is ongoing. Current focus areas are validating CPU/PPU/APU
  edge cases against public test ROMs and tightening cycle-level timing where
  real games expose differences.

See [`docs/dev/DevelopmentPlan.md`](docs/dev/DevelopmentPlan.md) for the full roadmap.

## Building

Requires Rust stable (see `rust-toolchain.toml`).

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

### macOS App Bundle

Build the unsigned universal App Bundle locally on macOS:

```sh
scripts/build-macos-app.sh
```

The script builds `frontend` for both `aarch64-apple-darwin` and
`x86_64-apple-darwin`, combines them with `lipo`, generates the app icon from
`assets/icons/AppIcon.png` into `Contents/Resources/Assets.car`, fills the
bundle metadata used by macOS's standard About panel, and writes:

- `target/release/Swanium.app`
- `target/release/Swanium-macos-universal.zip`

The bundle targets macOS 13.5 or newer (`MACOSX_DEPLOYMENT_TARGET=13.5`) and is
not code-signed.

## CI/CD

GitHub Actions are split by platform:

- `.github/workflows/ci-linux.yml`
- `.github/workflows/ci-macos.yml`
- `.github/workflows/ci-windows.yml`

Each workflow uses path filters so documentation-only changes do not trigger
build/test jobs. The macOS workflow runs on `macos-26`, calls
`scripts/build-macos-app.sh`, and uploads the `.app` directory as the normal
workflow artifact to avoid zip-in-zip packaging. On GitHub Releases, the
generated `Swanium-macos-universal.zip` is published as a release asset via
`.github/workflows/publish-release-assets.yml`.

## Running

```sh
cargo run -p frontend -- path/to/game.ws   # ROM path is optional
cargo run -p frontend                       # opens empty; press O to pick a ROM
```

Default keyboard controls: arrow keys = X-pad, `WASD` = Y-pad, `Z` = B, `X` = A,
`Enter` = Start. Press `O` to open a ROM file picker and `P` to dump PPU display
registers to stderr.

A connected gamepad works too (auto-detected via gilrs): D-pad / left stick =
X-pad, right stick = Y-pad, bottom face button = B, right face button = A, menu
button = Start.

Public test ROMs are never committed to this repository (licensing); see
[`tests/README.md`](tests/README.md).

## Accuracy References

Hardware behavior is checked first against primary WonderSwan references:

- [WSDev Wiki](https://ws.nesdev.org/wiki/WSdev_Wiki)
- [WonderSwan - Sacred Tech Scroll](http://perfectkiosk.net/stsws.html)
- [WonderSwan CPU test](https://github.com/FluBBaOfWard/WSCPUTest)
- [WonderSwan test suite](https://github.com/asiekierka/ws-test-suite)

Public test ROMs are run as opt-in compatibility tests because ROM binaries are
not committed. See [`tests/README.md`](tests/README.md) and
[`docs/dev/DevelopmentPlan.md`](docs/dev/DevelopmentPlan.md) for the testing
policy and reference priority.

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
