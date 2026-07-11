# Implementation status

Last updated: 2026-07-11. Update this file (not AGENTS.md) when implementation progress changes.

This is the source of truth for current progress. For the broader document map,
start with `docs/dev/README.md`.

## Progress summary

| Track | Current state | Notes |
|---|---|---|
| Architecture / workspace | Complete | Stable Rust workspace with core/frontend/audio/video/input/common crates. |
| Core emulator phases | Phases 1-8 complete/substantially complete | Phase 8 WonderSwan Color is complete, including 8a-8g and the HW_FLAGS 0xA0 boot-state fix. |
| Emulator execution milestones | Milestones 9-12 complete | SDMA, public-ROM oracle seed, compatibility matrix seed, PPU correctness pass, WSC audio pass, RTC/save persistence pass. |
| Current emulator focus | Milestone 13 | Timing/register precision: WSTimingTest pages 0-28 and WSHWTest `Test All` public oracles pass; further precision work is evidence-driven follow-up. |
| Remaining emulator work | Tracked in `docs/dev/RemainingWork.md` | Next priority is broader public ROM oracle coverage; dot-level PPU, timing decomposition, and audio/SDMA validation stay evidence-driven. |
| Compatibility evidence | Tracked in `docs/dev/CompatibilityMatrix.md` | Automated/synthetic rows cover CPU, SDMA, PPU, WSC audio, RTC, and mapper/save classes. |
| Frontend | Phase 7 usable; polish deferred | Remaining listed UI polish is non-blocking. |

Phases 1–7 of `docs/dev/DevelopmentPlan.md` are substantially complete; **Phase 8
(WonderSwan Color) is complete** (subphases 8a–8g done, plus a HW_FLAGS 0xA0
boot-state fix that makes real WSC ROMs render in colour). The workspace has 654 passing
tests (+5 opt-in, env-gated public-ROM test functions marked `ignored`; one ws-test-suite
ignored test covers multiple source-confirmed ROMs).

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
- CPU execution no longer uses `unimplemented!` for opcode dispatch. Every primary opcode byte
  has a non-faulting representative implementation. Undefined-opcode behavior follows
  FluBBaOfWard/WSCpuTest where covered: `0x0F` and `0x63`–`0x67` are 1-byte NOPs,
  `0xD8`–`0xDF` are 2-byte FPO NOPs, `0xD6` is SALC, `0xF6/F7 /1` preserve flags/registers
  while consuming the immediate, `0xFE /2`–`/6` mirror the corresponding `0xFF` group
  operations, `0xFF /7` is a no-op, and register-mode LEA/LES/LDS uses the WSCpuTest extended
  addressing table. `ENTER` nesting levels > 0 are implemented. Unit coverage includes a sweep
  that executes every primary opcode byte once and targeted WSCpuTest compatibility cases.

### Memory map / I/O bus, interrupts, timers, DMA — Phase 2 (`bus/`)
20-bit address space with WRAM / I/O port / cartridge-ROM dispatch; interrupt controller
(IVT, IRQ priority, INT/IRET linkage, VBlank line); HBlank/VBlank + general hardware timers;
GDMA (memory-to-memory) and SDMA (sound) transfer engines. I/O ports dispatch to handlers.
Real BIOS startup is supported beyond direct-cart boot: the bus now models the console internal
EEPROM (`IEEPROM`, ports `0xBA`–`0xBE`) synchronously enough for the BIOS configuration reads/writes.
The internal EEPROM starts zero-filled, matching NewOswan's newly-created `*_ieeprom.bin` files;
an erased `0xFF` image makes the real BIOS skip/avoid the splash display path. The bus also treats
the 0xA0 system-control write with bit 7 set as the internal boot-ROM disable latch, so the BIOS
WRAM trampoline can expose the cartridge reset vector before jumping to `FFFF:0000`.

### CPU test ROMs — Phase 3 (`tests/cpu_test_roms.rs`)
Self-built machine-code integration harness (`run_code`) covering arithmetic, control flow,
stack, string instructions, HLT. Public WSCPUTest / ws-test-suite ROMs are opt-in and
env-gated in `tests/public_roms.rs`. The WSCPUTest path is verified against
FluBBaOfWard/WSCpuTest v0.7.1: the ignored test runs the ROM through `System::run_frame`,
injects A to start the default `Test All` menu item, and decodes the background tile map for
`Ok!` / `Failed!` output. The ws-test-suite path now has source-confirmed decoded oracles for
`mono/cpu/80186_quirks.ws`, `mono/cpu/prefixes.ws`,
`mono/cpu/interrupt_timing.ws`, `mono/soc/interrupts.ws`,
`mono/rtc/mapper.ws`, `mono/display/mono_palettes_writemask.ws`,
`mono/sound/quirks.ws`,
`mono/display/sprite_scanline_limit.ws`,
`mono/eeprom/{cartridge_1kbit,cartridge_16kbit,internal}.ws`,
`color/display/tile_screen_extended_range.wsc`,
`color/dma/alignment_access.wsc`, `color/dma/gdma_timing.wsc`,
`color/dma/sound_dma.wsc`,
and
`wonderful/libc/{strlen,strchr,memset,memcmp,memcpy,memccpy,setjmp,initfini,malloc}.ws`.
Most of these ROMs use upstream `common/test/pass_fail.h`: pass/fail markers are tile 5/6 in
`screen_1` at WRAM `0x1800`, with marker positions mirrored from each ROM's source. Display ROMs
that encode results as rendered text are checked from the framebuffer instead. Unknown ws-test-suite
ROMs are rejected instead of using the former placeholder HLT + `WRAM[0x0000] == 0` convention.
The interrupt-timing oracle pins STI/POPF/IRET IF-enable delay, POP/MOV SS IRQ
delay, and TF/BRK delivery after POPF. The prefixes oracle pins segment-override
precedence across repeated prefixes and the REP MOVSB hardware-IRQ restart IP
observed by the upstream HBlank-timer case.

Milestone 13 added FluBBaOfWard/WSTimingTest v0.4.0 as a public CPU timing oracle
covering pages 0-28. The source-confirmed decoder reads the background tile map at
WRAM `0x1800` and checks the Pass column marker (`o` / `x`) at byte offset
`row * 64 + 48`; page/row lists mirror upstream `testcalls.asm`. The full timing
suite now passes against the local `timingtest.ws`, covering baseline timing,
primary opcode groups, addressing variants, REP/string I/O, exception/interrupt timing,
and loop edges. CPU timing was adjusted accordingly: `NOP` is 1 cycle, fixed-port
`IN`/`OUT` is 7 cycles, DX-port `IN`/`OUT` is 5 cycles, taken `Jcc` is 5 cycles
plus 1 for odd targets, and taken `LOOP` is 6 cycles.
Milestone 13 also added FluBBaOfWard/WSHWTest `Test All` as a public hardware-register
oracle. Passing it required hardware-visible interrupt semantics (interrupt sources latch only
when enabled, but an already-latched cause remains pending after `INT_ENABLE` changes), the
HBlank timer counter-1 latch behavior, color-mode-dependent DMA visibility, and broad I/O
register read/write masks across display, palette, DMA, audio, serial, timer, and interrupt
ports. Further timing work remains evidence-driven rather than a broad per-clock
scheduler rewrite.
The ws-test-suite `mono/soc/interrupts.ws` oracle also pins mono UART TX ready as
a level-style IRQ source, mono `INT_VECTOR` low-bit status readback, and HALT
wake from a pending VBlank cause while IF is clear. The mono-specific readback is
kept gated from Color mode to preserve FluBBaOfWard/WSHWTest behavior.
The ws-test-suite display/DMA additions pin mono palette register write masks
and GDMA source gating/timing: SRAM sources abort, and source `0x80000` in the
slow-ROM window aborts while port `0xA0` bit 3 (`SYSTEM_CTRL1_ROM_WAIT`) is
set; upper linear ROM, fast-ROM, and IRAM sources still complete when allowed.
The GDMA timing oracle also pins the APU fast-sweep test counter used by the
ROM's cycle-count harness and the CPU-visible GDMA stall formula of
`5 + transferred_bytes` for started transfers, with zero extra stall for
zero-length and immediately-aborted SRAM-source cases.
The SDMA oracle pins 20-bit source/length masks, ROM/IRAM/SRAM source access,
hold/repeat behavior, terminal-count zeroing, address overflow wrapping, final
voice-latch contents, and port `0x52` readback preserving the enable bit while
masking unused bit 5.
The mono sound quirks oracle pins CPU-visible APU output readback ports
`0x96`/`0x98`/`0x9A`, LFSR readback through `0x92`/`0x93`, voice readback while
the channel enable bit is clear, channel counter behavior across alternate
voice/sweep/noise modes, and immediate `0x8E` noise-reset self-clear.
The ws-test-suite RTC mapper oracle pins the generated RTC footer flag
(`0x0C` bit 2 / flags value `0x04`), status-port ready/busy bits, command
payload lengths for `0x10`-`0x1B`, unsupported-command timeout for `0x1C`, and
the rule that writing the ready bit to port `0xCA` does not force ready.

### PPU — Phase 4 (`ppu/`) + Milestone 10 correctness pass
Mono 224×144, 4-shade grayscale, scanline-driven. SCR1/SCR2 backgrounds (scroll, tile flip),
sprite layer (OAM 4-byte entries, priority, X/Y flip), window mask (SCR2 inside/outside +
sprite window). Palette resolution abstracted behind the `PaletteResolver` trait with
`MonoPaletteResolver` (2bpp → palette-pool → shade-pool chain). The mono palette-zero
transparency rule was fixed in commit 10a8146. Rendering is optimised per scanline rather than
per pixel with output unchanged (verified by framebuffer hash): sprites are decoded and
Y-filtered once per line (`collect_line_sprites`), and each background layer is resolved once per
line (`fill_background_line`), decoding the tile-map entry and tile row bytes once per 8-pixel
span. Together ~7× faster PPU / ~5× faster frame on a real WSC ROM; see `docs/dev/Profiling.md`.
The scanline renderer now enforces the hardware's 32-sprites-per-scanline limit in OAM order:
the first 32 sprites whose 8-pixel-tall box covers the line are considered, and later sprites on
that line are ignored even if the earlier entries are transparent at the sampled pixel. Regression
tests cover overflow ordering and a 33rd priority-1 sprite that would otherwise draw in front of
SCR2. Sprite X/Y coordinates wrap in 8-bit screen space, so sprites starting at
`0xF8`-`0xFF` can appear clipped at the left/top visible edge; tests cover both
axes.
Color-mode color-zero transparency and backdrop palette-index behavior are now
source-confirmed against ares and Mednafen: color index 0 falls through to the
backdrop, and port `0x01` is an 8-bit palette-RAM backdrop index.

Milestone 10's raster audit keeps the PPU at scanline granularity for now. `System::run_frame_traced`
is covered for one trace row per visible line and CPU-written scroll state before line rendering;
frame-driver tests cover line-compare IRQs, HBlank timer coverage across visible+VBlank scanlines,
and VBlank IRQ timing. No current public fixture or known title justifies a dot-level rewrite yet.

### APU — Phase 5 (`apu/`) + Phase 8f HyperVoice
Four 32-sample × 4-bit wave-table channels, per-channel L/R nibble volume, stereo mix;
ch4 noise (15-bit LFSR, variable tap), ch3 sweep, ch2 voice PCM. Output is interleaved
i16 @ 24 kHz via `Bus::audio_samples()` / `clear_audio_samples()`. **Voice (ch2 PCM)** is
treated as **signed** (silence `0x80` → 0, per Mednafen `wswan/sound.c`) and reconstructed
through a per-write 2-tap moving-average (`VoiceLowPass`, fed by `Apu::write_voice` from
`Bus::write_io` on every `0x89` write while voice mode is on) with a compensating `VOICE_GAIN`.
The frame driver now advances the APU after each CPU instruction (with scanline cycle carry)
instead of batching a whole scanline after the CPU, so HBlank-ISR PCM writes land at the right
point in the audio timeline. It also runs the HBlank timer across all 159 scanlines (144 visible
+ 15 VBlank) rather than only the rendered lines; *Last Alive* uses that timer as the `0x89` PCM
update clock, so a 144-line counter made the stream roughly 144/159 slow. Port `0x91` output
control is applied at the final mix: speaker mode mixes L+R to mono with the documented shift,
while headphone mode preserves stereo.
Games time-multiplex two PCM voices onto the single voice register at ~2× the audio rate
(e.g. *Last Alive* ping-pongs a music voice with a second voice through the HBlank-timer ISR);
the moving average nulls the multiplex component (Nyquist of the write stream) that hardware's
analog output stage averages out, so the interleave no longer reads as a buzz. Filtering the raw
write stream — not a value sampled once per scanline by the mixer — is required to preserve both
halves of the multiplexed stream.
Further reconstruction-quality tuning (residual multiplex ripple from write-per-scanline jitter)
is still open; `docs/dev/AudioAccuracy.md` records the manual comparison plan. **HyperVoice**
(WSC-only, Phase 8f + Milestone 11) adds a fifth independent PCM source. The 8-bit path uses
control at port `0x6A` (enable / sample-extension mode / volume shift), L/R routing at `0x6B`,
and data latch at `0x69`; the sample is expanded and summed into the stereo output at the
wave-mix level (`hypervoice_sample` / `hypervoice_output`, Mednafen `wswan/sound.c`). The
16-bit direct path at `0x64`–`0x67` writes signed little-endian left/right output words, which
take precedence over the 8-bit latch when non-zero. The gate is applied on both read/write and
mixing: `Bus::write_io` drops mono writes to `0x64`–`0x6B`, and `Apu::tick(…, color)` receives
`color=true` only when the machine is Color/Crystal and port `0x60` bit 7 enables color mode.
Port `0x9E` is implemented as the built-in speaker main-volume register on all models for
software-visible low-two-bit readback. It is not applied to the mix: ares applies it as a final
stream amplitude, but Mednafen leaves `0x9E` out of the audio path and labels the port unknown in
its bundled tech note, while MAME comments it as the WSC volume setting. Swanium treats it as a
BIOS/body volume setting: useful on real hardware, but redundant in an emulator where host/frontend
volume is freely adjustable. Sound DMA feeds the voice latch from ports `0x4A`–`0x52`; SDMA sample
cadence is source-confirmed against ares and Mednafen as a 24 kHz APU cadence divided by 6/4/2/1 for
rate bits 0/1/2/3. HyperVoice `0x6A` bits 4-6 are reference-triaged but not applied: ares treats
them as speed divisors, while Mednafen ignores them, so Swanium keeps the Mednafen-like current-latch
behavior pending public-ROM or hardware evidence. Remaining audio risk is SDMA bus-stall validation
or a deterministic PCM fixture exposing a concrete mismatch.

### Cartridge / save RAM — Phase 6 (`bus/cart/`)
16-byte footer header parse, Bandai 2001/2003 banking via a `Mapper` enum, SRAM and
93Cxx (Microwire) EEPROM save media, zero-copy save-data API (`Bus::save_data` /
`load_save_data`). The `Cartridge.rtc: Option<Rtc>` device is realized in Phase 8e
(BCD registers, 0xCA/0xCB command protocol, deterministic injected-time timekeeping).

Milestone 12 tightened the save-related edges without changing the simple cartridge save API.
Cartridge SRAM and cartridge EEPROM are game save media. The core exposes them through
`save_data()` / `load_save_data()`, and the frontend persists those bytes under the platform
config directory as `saves/<ROM file name>.sav`. Bandai 2003 high-byte bank ports are covered on a large
ROM image rather than only modulo-wrapped small fixtures; cartridge EEPROM tests cover common
initialization flows (`WRAL`, `EWDS`) and absent-device open-bus reads; and absent RTC command/data
ports (`0xCA`/`0xCB`) now read open bus instead of a raw zero shadow. RTC-bearing cartridges keep
RTC state outside the raw cartridge save byte slice; `save_data()` remains exactly SRAM-or-EEPROM
bytes, while `Rtc::state` / `load_state` exposes the clock registers separately.

The cartridge EEPROM public oracles pin 128-byte and 2 KiB address widths,
power-on write lock, EWEN/EWDS behavior, erase/write/read flow, invalid-command
status, absent SRAM open bus, and Bandai 2001 control-port DONE-bit behavior.

Console internal EEPROM (`IEEPROM`, ports `0xBA`–`0xBE`) is a different device from cartridge
EEPROM. It is used by the BIOS for console profile/configuration data and is not game save media.
Swanium initializes it as zero-filled deterministic state at startup, matching NewOswan's
newly-created `*_ieeprom.bin` behavior. The ws-test-suite `mono/eeprom/internal.ws`
oracle pins shared mono/Color command compatibility (6-bit mono-compatible and
10-bit Color command widths), erase-via-write, write lock/unlock, invalid
commands, the protected byte range starting at `0x60`, and `0xBE` DONE-bit
readback delay.

### System / keypad — Phase 7 core (`system.rs`, `keypad.rs`)
`System { cpu, bus }` owns the machine and exposes frame-boundary `run_frame(keys)`
(159 scanlines × 256 cycles, sequential CPU→APU/GDMA→scanline driving) plus the
RA-friendly, side-effect-free `read_memory_at(addr)`. 11 physical keys are modeled by
`KeyState` (`u16` bitset); `Bus::set_keys` raises a frame-granular `KeyPress` interrupt.

## Frontend & adapter crates — Phase 7

- `crates/video`: shade-index (0–15) → RGBA8 conversion (`shade_to_rgba` / `framebuffer_to_rgba`);
  90° clockwise/counter-clockwise rotation for vertical games (`write_rgba_rotated_cw` /
  `write_rgba_rotated_ccw`).
- `crates/audio`: cpal output stream + fixed-capacity `RingBuffer`; linear 24 kHz→device-rate
  resampler (replacing the earlier zero-order hold, which made channel-2 PCM streams such as
  *Last Alive* sound harsh on 48 kHz devices); audio–video sync via buffer-level frame pacing.
- `crates/input`: backend-agnostic `Button` enum (11 keys, stable `name`/`from_name`/`label`)
  + `keys_from`; gilrs gamepad (`gamepad::Gamepad`, event-driven digital + dead-zoned analog)
  with runtime-configurable bindings (`set_named_bindings`), a name↔button table for
  persistence, and `poll_capture` for rebind capture.
- `crates/common`: `tracing` logging (`logging::init`); typed `Config` with serde/TOML
  persistence at the platform config dir (`swanium/config.toml`), range-clamped on load.
  Persists window scale, fullscreen, BIOS-ROM startup mode (`BiosRomKind`: disabled /
  WonderSwan / WonderSwan Color / SwanCrystal), rotation (`RotationKind`: none/right/left),
  renderer (`RendererKind`), recent-ROM history (`push_recent` / `clear_recent`, capped),
  and keyboard/gamepad binding maps. BIOS files are loaded from the fixed-name
  `bios/` directory under the same platform config directory: `ws_irom.bin`
  (WonderSwan), `wsc_irom.bin` (WonderSwan Color), and `wc_irom.bin` (SwanCrystal,
  matching NewOswan's stub file name). NewOswan's stub files are 4 bytes shorter
  than their 4 KiB / 8 KiB mapped blocks, so the core pads boot ROMs to the next
  4 KiB boundary before mapping them at the top of the 20-bit address space.
  Those NewOswan stubs intentionally contain no boot splash or configuration menu;
  a splash requires a real console boot ROM image.
  Cartridge SRAM/EEPROM saves are stored in `saves/` under the same platform config
  directory, one raw `.sav` file per ROM file name.
- `crates/frontend`: Slint UI compiled from `ui/*.slint` via `build.rs` + `include_modules!`
  (`MainWindow`, `SettingsWindow`, `AboutWindow`). Audio-paced timer drives
  `System::run_frame` → `video::write_rgba[_rotated_cw]` → Slint image. Menu bar:
  File ▸ Open ROM… / Open Recent (dynamic history) / Clear History / Settings… / Quit;
  View ▸ Scale 1–4× / Fullscreen (aspect-preserving `image-fit: contain`) / Rotate Left /
  Rotate Right / Renderer (Nearest ↔ Bilinear via `image-rendering`). Menu
  checkmarks are title-prefix driven by state (not `checkable`, which toggles on activate).
  About is platform-aware: macOS uses the OS-standard application-menu About item, while
  Windows/Linux keep the Slint Help ▸ About dialog.
  Native Open ROM dialogs are opened outside the Slint frame timer, and
  emulation/audio/input stays idle while the picker is open.
  Emulation ▸ Pause (Ctrl+P, runtime-only toggle — not persisted) / Reset (Ctrl+R, reloads
  the current ROM and clears transient audio/input state) / BIOS Settings… (when a BIOS mode is
  selected, resets while holding Start for the first few frames, matching the real BIOS setup-menu
  entry gesture). View shortcuts include Ctrl+1–4 for Scale and selection-style rotation:
  Ctrl+Up for normal orientation, Ctrl+Left for rotate left, and Ctrl+Right for rotate right
  (reselecting the active rotation no longer toggles back to normal). Settings is available via
  Ctrl+Comma. Capture-based
  settings window with BIOS-ROM startup mode selection plus input remapping (keyboard via
  focus-scope key capture, controller via `poll_capture`) persisting to config. Changing the BIOS
  mode immediately resets/reloads the current emulation so the setting is visible without a manual
  reset. When a BIOS mode is selected, the frontend reads the corresponding fixed-name BIOS file and
  installs it into the core before reset; missing BIOS files fall back to direct boot with a status/log
  warning. Core panics while running BIOS/game code are caught at the frontend frame boundary,
  logged, and converted to a paused/stopped status instead of crashing the app. Reset is always
  enabled: with a loaded ROM it reloads the current ROM, and without one it starts the selected
  BIOS alone if available. Status bar (ROM name + FPS + master-volume slider,
  0–100, applied to the cpal output via `AudioStream::set_volume` / `audio::scale_volume` and
  persisted). Headless frame smoke test in `crates/core/tests/system_frame.rs`.

Remaining Phase 7 UI polish (deferred, non-blocking): Bicubic renderer (needs a future wgpu
upscaling pipeline — Slint's image path exposes only nearest/bilinear).

## Phase 8 — WonderSwan Color (complete)

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
  unchanged. `color/display/tile_screen_extended_range.wsc` confirms the second 2bpp tile bank plus
  upper-WRAM screen-map and sprite-table ranges in Color mode. 4bpp planar byte order and packed
  nibble order, background tile-map bank selection, and sprite attribute bit meanings are now
  source-confirmed against ares and Mednafen.
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
  0x0C bit 2, confirmed by ws-test-suite `mono/rtc/mapper.ws` (`rtc = true` generates flags `0x04`).
  Port `0xCA` reads ready/busy status rather than echoing the command byte, and the mapper oracle pins
  command payload lengths and ready-cleared-on-new behavior. Alarm-IRQ behavior is still unverified/deferred —
  see DevelopmentPlan 実装メモ（8e）.
- **8 addendum — HW_FLAGS 0xA0 / real WSC colour boot (done)**: real WSC ROMs (Final Fantasy, etc.) run
  as Color hardware and render in colour. The missing piece was the power-on hardware-detect register:
  `Bus::read_io(0xA0)` now returns `0x87` on Color/Crystal and `0x86` on mono (Mednafen `gfx.c`
  `wsc ? 0x87 : 0x86`); games poll it at boot to take their colour path. With this single fix FF/Dark Eyes/
  Dragonball/Digimon Tamers boot into colour mode (set port 0x60 bit 7, populate palette RAM at 0xFE00,
  render full colour) — verified against real ROMs. Confirmed against ares that colour enable = port 0x60
  bit 7 (`color() = mode.bit(2)`), i.e. the 8b assumption was correct. The frontend runs `.wsc` images as
  Color (`set_model`) and shows the model in the status bar. For real console BIOS images, `0xA0` writes
  with bit 7 set also disable the boot-ROM overlay so the BIOS can hand off to the cartridge after the
  splash/configuration path. See DevelopmentPlan 実装メモ（8 追補）.
- **8f (done)**: Color APU extension — **HyperVoice** (`crates/core/src/apu/mod.rs`). A fifth,
  wave-channel-independent 8-bit PCM source per Mednafen `wswan/sound.c`: control `0x6A`
  (enable bit 7 / sample-extension mode bits 3-2 / volume shift bits 1-0), routing `0x6B`
  (left bit 6 / right bit 5, write-masked `0x6F`), data latch `0x69`. `hypervoice_sample` expands the
  8-bit latch to a signed ~11-bit value (`i16`-truncated then `>> 5`) and `hypervoice_output` scales it
  by `MIX_SCALE` and routes it, so it sums into the same output domain as the four wave channels — which
  never saturate alone, so `mix` stays exact and the final clamp runs once after HyperVoice is added.
  Milestone 11 adds the signed 16-bit direct path at `0x64`–`0x67` and gates HyperVoice mixing on WSC
  color mode (`0x60` bit 7), not only the hardware model. The Color gate covers read and write (like the
  8d upper-RAM window): `Bus::write_io` drops mono `0x64`–`0x6B` writes and `Apu::tick(…, color)` skips
  the mix when HyperVoice is unavailable. Sound DMA is implemented in the
  bus: ports `0x4A`–`0x52` form a Color-only SDMA engine clocked from the APU path at `128 * rate`
  master cycles per byte, and delivered bytes go through the same `0x89` voice-latch helper as CPU I/O
  writes so `Apu::write_voice` sees the real stream. Terminal count clears bit 7 unless repeat is set;
  repeat reloads source/counter shadows; hold outputs zero without advancing. Deferred/unverified: the
  exact hardware bus-stall details. SDMA rate bits 0/1/2/3 are source-confirmed against ares and
  Mednafen as 4000/6000/12000/24000 Hz transfer cadence. HyperVoice `0x6A` speed bits are not applied
  because ares and Mednafen disagree on their software-visible timing. Port
  `0x9E` attenuation is intentionally not applied: MAME identifies it as the WSC volume setting, and
  emulator-wide output attenuation belongs in frontend/host volume control rather than deterministic
  core mixing.
- **8g (done)**: integration-level test consolidation for the Phase 8 features and mono-regression
  parity, plus this final doc pass. New end-to-end tests drive the Color paths through the same public
  API the frontend uses, each pinned against its mono-regression twin: `crates/core/tests/color_render.rs`
  (colour PPU rendering from palette RAM, HyperVoice stereo output + routing, and the CPU→I/O→PPU
  colour-bit path — with mono/colour-bit-clear falling back to the shade path and dropping HyperVoice) and
  two `crates/core/tests/system_frame.rs` cases (a colour tile reaching the framebuffer through
  `System::run_frame`, and the RTC free-running one second across frames from the frame driver alone).
- **Milestone 10 Color PPU validation (done at synthetic-test level)**: implementation tests now pin
  color-zero transparency, backdrop palette indexing, 4bpp planar byte order, packed high/low nibble
  order, background tile-map bank selection, and sprite attribute bit 13 as priority rather than a
  tile-bank selector. Hardware/public-ROM validation remains tracked in `RemainingWork.md`.
- Color-mode color-zero transparency, backdrop palette indexing, 4bpp planar byte order, packed
  high/low nibble order, background tile-bank selection, and sprite attribute semantics are
  source-confirmed against ares and Mednafen and recorded in `CompatibilityMatrix.md`. Further Color
  PPU work should wait for a concrete public-ROM, hardware-capture, reference-emulator, or
  title-specific discrepancy.

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

## Release tooling — macOS App Bundle

- **Unsigned universal macOS bundle script** — `scripts/build-macos-app.sh` builds the
  `frontend` package for `aarch64-apple-darwin` and `x86_64-apple-darwin`, with
  `MACOSX_DEPLOYMENT_TARGET=13.5`, combines the two release binaries via `lipo`, generates
  `Contents/Resources/Assets.car` from `assets/icons/AppIcon.png`, fills the
  `Info.plist` metadata used by macOS's standard About panel, and emits `target/release/Swanium.app`
  plus `target/release/Swanium-macos-universal.zip`.
  The bundle intentionally performs no code signing.
- **Platform-split CI** — GitHub Actions workflows are split into
  `.github/workflows/ci-linux.yml`, `.github/workflows/ci-macos.yml`, and
  `.github/workflows/ci-windows.yml`, with path filters so documentation-only changes do not run
  build/test jobs. The macOS workflow runs on `macos-26`, executes the same bundle script used
  locally, uploads the `.app` directory as the normal workflow artifact (avoiding zip-in-zip), and
  publishes the generated unsigned universal zip only for GitHub Releases via
  `.github/workflows/publish-release-assets.yml`.
