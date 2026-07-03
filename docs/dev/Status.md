# Implementation status

Last updated: 2026-07-03. Update this file (not AGENTS.md) when implementation progress changes.

Phases 1–7 of `docs/dev/DevelopmentPlan.md` are substantially complete; **Phase 8
(WonderSwan Color)** is in progress (subphases 8a–8e done, plus a HW_FLAGS 0xA0
boot-state fix that makes real WSC ROMs render in colour). The workspace has 515 passing
tests (+2 opt-in, env-gated public-ROM tests marked `ignored`).

## Core (`crates/core`, package `swanium-core`) — platform-independent

### CPU — Phase 1 (`cpu/`)
V30MZ register file, flags, ModRM decoding, and a near-complete 8086/80186-class
instruction set against the `MemoryBus` trait.

- Data movement: MOV (all forms incl. segment registers and memory-direct 0xA0–0xA3),
  XCHG, PUSH/POP (incl. segment forms), LAHF/SAHF/PUSHF/POPF, XLAT, CBW/CWD, LEA, LES, LDS.
- Arithmetic/logic: ADD/OR/ADC/SBB/AND/SUB/XOR/CMP/TEST (+ immediate/group forms),
  INC/DEC, NOT/NEG, MUL/IMUL/DIV/IDIV (group F6/F7), shift/rotate group (D0–D3),
  BCD instructions DAA/DAS/AAA/AAS/AAM/AAD.
- Control flow: JMP/Jcc/CALL/RET (near/far), LOOP/LOOPE/LOOPNE/JCXZ, flag instructions,
  NOP/HLT, ENTER/LEAVE, indirect CALL/JMP/PUSH (Group FF), INT/INTO/IRET.
- String: MOVS/CMPS/SCAS/LODS/STOS (byte and word, 0xA4–0xAF) with REP/REPE/REPNE
  (0xF2/0xF3); INS/OUTS (0x6C–0x6F).
- Port I/O: IN/OUT (fixed and variable port, byte and word) wired to the Phase 2 I/O bus.
- 80186/V30 additions: PUSHA/POPA (0x60/0x61), BOUND (0x62), PUSH imm16/imm8 (0x68/0x6A),
  IMUL r16,r/m16,imm (0x69/0x6B), immediate-count shift/rotate (0xC0/0xC1), POP r/m16 (0x8F).
  See `cpu/tests/v30_extensions.rs`.
- Prefixes: segment override (0x26/0x2E/0x36/0x3E) in `Cpu::seg_override`; REP in `Cpu::rep_prefix`.
- Still deferred (`unimplemented!`): LES/LDS with a register operand (illegal encoding),
  ENTER with nesting level > 0, a few undefined Group FE/FF opcode sub-cases.

### Memory map / I/O bus, interrupts, timers, DMA — Phase 2 (`bus/`)
20-bit address space with WRAM / I/O port / cartridge-ROM dispatch; interrupt controller
(IVT, IRQ priority, INT/IRET linkage, VBlank line); HBlank/VBlank + general hardware timers;
GDMA (memory-to-memory) and SDMA (sound) transfer engines. I/O ports dispatch to handlers.

### CPU test ROMs — Phase 3 (`tests/cpu_test_roms.rs`)
Self-built machine-code integration harness (`run_code`) covering arithmetic, control flow,
stack, string instructions, HLT. Public WSCPUTest / ws-test-suite ROMs are opt-in and
env-gated in `tests/public_roms.rs` (2 `ignored`; output-format verification is a Phase 3
residual task, see DevelopmentPlan §10.4).

### PPU — Phase 4 (`ppu/`)
Mono 224×144, 4-shade grayscale, scanline-driven. SCR1/SCR2 backgrounds (scroll, tile flip),
sprite layer (OAM 4-byte entries, priority, X/Y flip), window mask (SCR2 inside/outside +
sprite window). Palette resolution abstracted behind the `PaletteResolver` trait with
`MonoPaletteResolver` (2bpp → palette-pool → shade-pool chain). The mono palette-zero
transparency rule was fixed in commit 10a8146. Rendering is optimised per scanline rather than
per pixel with output unchanged (verified by framebuffer hash): sprites are decoded and
Y-filtered once per line (`collect_line_sprites`), and each background layer is resolved once per
line (`fill_background_line`), decoding the tile-map entry and tile row bytes once per 8-pixel
span. Together ~7× faster PPU / ~5× faster frame on a real WSC ROM; see `docs/dev/Profiling.md`.

### APU — Phase 5 (`apu/`)
Four 32-sample × 4-bit wave-table channels, per-channel L/R nibble volume, stereo mix;
ch4 noise (15-bit LFSR, variable tap), ch3 sweep, ch2 voice PCM. Output is interleaved
i16 @ 24 kHz via `Bus::audio_samples()` / `clear_audio_samples()`.

### Cartridge / save RAM — Phase 6 (`bus/cart/`)
16-byte footer header parse, Bandai 2001/2003 banking via a `Mapper` enum, SRAM and
93Cxx (Microwire) EEPROM save media, zero-copy save-data API (`Bus::save_data` /
`load_save_data`). The `Cartridge.rtc: Option<Rtc>` device is realized in Phase 8e
(BCD registers, 0xCA/0xCB command protocol, deterministic injected-time timekeeping).

### System / keypad — Phase 7 core (`system.rs`, `keypad.rs`)
`System { cpu, bus }` owns the machine and exposes frame-boundary `run_frame(keys)`
(159 scanlines × 256 cycles, sequential CPU→APU/GDMA→scanline driving) plus the
RA-friendly, side-effect-free `read_memory_at(addr)`. 11 physical keys are modeled by
`KeyState` (`u16` bitset); `Bus::set_keys` raises a frame-granular `KeyPress` interrupt.

## Frontend & adapter crates — Phase 7

- `crates/video`: shade-index (0–15) → RGBA8 conversion (`shade_to_rgba` / `framebuffer_to_rgba`).
- `crates/audio`: cpal output stream + fixed-capacity `RingBuffer`; audio–video sync via
  buffer-level frame pacing.
- `crates/input`: backend-agnostic `Button` enum (11 keys) + `keys_from`; gilrs gamepad
  (`gamepad::Gamepad`, event-driven digital + dead-zoned analog); keyboard bindings.
- `crates/common`: `tracing` logging (`logging::init`); typed `Config` with serde/TOML
  persistence at the platform config dir (`swanium/config.toml`), range-clamped on load.
- `crates/frontend`: Slint `MainWindow` (Image + FocusScope), ~13.25 ms (~75.47 Hz) timer
  driving `System::run_frame` → `video::write_rgba` → Slint image (integer scaling,
  `image-rendering: pixelated`). In-app ROM picker (rfd via xdg-portal, `O` key), menu bar
  (File ▸ Open ROM… / Quit), status bar (ROM name + FPS). Headless frame smoke test in
  `crates/core/tests/system_frame.rs`.

Remaining Phase 7 UI polish (deferred, non-blocking): startup-pause, settings UI, key-binding screen.

## Phase 8 — WonderSwan Color (in progress)

Realizes the Color abstraction points from DevelopmentPlan §6. Subphase breakdown and the
framebuffer-format / RTC-determinism decisions are recorded in DevelopmentPlan Phase 8.

- **8a (done)**: `HardwareModel::{Mono, Color, Crystal}` (`crates/core/src/model.rs`), threaded
  through `Bus`/`System` (`model()`/`set_model`, defaulted from the header color flag in
  `from_rom`). The framebuffer was unified to **RGB444 `u16` (`0x0RGB`)**: `PaletteResolver`
  now returns `Rgb444`; `MonoPaletteResolver` maps a shade to an (inverted-brightness) grey via
  `grey_rgb444`; `crates/video` expands RGB444 → RGBA8888 (`rgb444_to_rgba`). Mono output is
  byte-for-byte identical on screen.
- **8b (done)**: `ColorPaletteResolver` (`crates/core/src/ppu/palette.rs`) reads the 12-bit
  palette RAM at WRAM 0xFE00 (16 palettes × 16 colors); `Bus::render_scanline` selects it via
  `color_rendering_enabled()` = model is color **and** the 0x60 video-mode color bit (bit 7) is
  set, else the mono shade path. Color transparency, backdrop indexing, and the 0x60 bit meaning
  are documented assumptions (DevelopmentPlan Phase 8 実装メモ 8b) pending hardware/test-ROM checks.
- **8c (done)**: color tile formats via `TileMode` (derived from port 0x60): 2bpp planar (mono /
  color) at WRAM 0x2000, 4bpp planar and packed at 0x4000, and the color second tile bank
  (tile-map bit 13). `sample_background`/`sample_sprite` decode per the active mode; mono is
  unchanged. Byte-order/base-address details are documented assumptions (DevelopmentPlan 実装メモ 8c).
- **8d (done)**: the Color 64 KiB internal-RAM window. `Bus::read_wram`/`write_wram` gate the upper
  48 KiB (0x04000–0x0FFFF) — which holds the palette RAM at 0xFE00 and the 4bpp tile banks at 0x4000
  — behind `model.is_color()`; on mono it stays open bus and writes are dropped (verified, not just
  read-masked). GDMA destination writes route through the same gate. The port 0x60 video-mode bits
  are already fully consumed (bit 7 by `color_rendering_enabled`, bits 6/5 by `TileMode`), so no
  additional 0x60 wiring was needed.
- **8e (done)**: the cartridge RTC (`crates/core/src/bus/cart/rtc.rs`). BCD date/time (7 registers) +
  status + alarm, driven by the 0xCA (command/status) / 0xCB (data) protocol. Deterministic timekeeping
  with **no wall-clock in core**: the frontend injects an absolute time once via `System::set_rtc_datetime`
  (default epoch 2000-01-01 if never injected), and the clock free-runs off the emulated master clock
  (`System::drive_frame` → `Bus::tick_rtc(CYCLES_PER_FRAME)`) with full BCD carry and leap-year handling.
  `Bus` routes 0xCA/0xCB to the RTC only when `cart.has_rtc()`; presence is decoded from footer flags byte
  0x0C bit 1 (unverified). Command codes / byte order and the alarm-IRQ path are unverified/deferred —
  see DevelopmentPlan 実装メモ（8e）.
- **8 addendum — HW_FLAGS 0xA0 / real WSC colour boot (done)**: real WSC ROMs (Final Fantasy, etc.) run
  as Color hardware and render in colour. The missing piece was the power-on hardware-detect register:
  `Bus::read_io(0xA0)` now returns `0x87` on Color/Crystal and `0x86` on mono (Mednafen `gfx.c`
  `wsc ? 0x87 : 0x86`); games poll it at boot to take their colour path. With this single fix FF/Dark Eyes/
  Dragonball/Digimon Tamers boot into colour mode (set port 0x60 bit 7, populate palette RAM at 0xFE00,
  render full colour) — verified against real ROMs. Confirmed against ares that colour enable = port 0x60
  bit 7 (`color() = mode.bit(2)`), i.e. the 8b assumption was correct. The frontend runs `.wsc` images as
  Color (`set_model`) and shows the model in the status bar. See DevelopmentPlan 実装メモ（8 追補）.
- **8f–8g (pending)**: Color APU extensions (Hyper Voice); test consolidation + final doc pass.

## Tooling — profiling & benchmarks

Performance measurement infrastructure (see `docs/dev/Profiling.md`):

- **In-core frame profiler** — `swanium-core`'s `profiling` feature (off by default, zero overhead,
  fully deterministic) accumulates per-subsystem wall-clock time (CPU / PPU / APU / DMA) inside
  `System::drive_frame`; read it via `System::profile_snapshot()` (`crates/core/src/profile.rs`).
  The `profile` example (`cargo run -p swanium-core --features profiling --example profile --release`)
  prints the split for a synthetic or real (`SWANIUM_BENCH_ROM`) ROM.
- **Criterion benches** — `crates/core/benches/frame.rs` (`cargo bench -p swanium-core`): `run_frame`
  plus `render_scanline` / `tick_apu_frame` micro-benchmarks, on a self-contained synthetic ROM.
- **Release/bench profiles** — root `Cargo.toml` sets `lto = "thin"`, `codegen-units = 1` for
  `[profile.release]` and `[profile.bench]`.
