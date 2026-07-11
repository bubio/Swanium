# Remaining work

Last updated: 2026-07-11.

This is the source of truth for open emulator work after Milestone 13's public
ROM precision pass. `Status.md` remains the source of truth for implemented
features; this file tracks what is intentionally still open.

## Current position

Milestone 13 is no longer blocked by known timing/register failures:

- FluBBaOfWard/WSTimingTest v0.4.0 pages 0-28 pass as an opt-in public ROM
  oracle.
- FluBBaOfWard/WSHWTest `Test All` passes as an opt-in public hardware-register
  oracle.
- WSCpuTest v0.7.1 `Test All` and the source-confirmed `ws-test-suite`
  CPU/interrupt/RTC/display/DMA/libc ROMs listed in `CompatibilityMatrix.md`
  already have decoded opt-in oracles.

The remaining work should therefore be evidence-driven. Do not start broad
cycle, dot, or analog rewrites without a public test ROM, hardware capture,
reference-emulator discrepancy, or known title demonstrating a software-visible
problem.

## P0 - Public ROM oracle expansion

The highest-value next work is to turn more public test ROMs into deterministic
opt-in regressions.

Current actionable state: the local public-ROM inventory is exhausted. Do not
spend the next session re-reading the same ws-test-suite sources unless new ROMs
or upstream changes have been added. The Color PPU and audio follow-ups below
have also been source-triaged and covered by focused synthetic tests where
possible. The next concrete work should therefore be either:

- add a newly discovered public ROM oracle with a source-confirmed result
  protocol, or
- move to the P2 maintenance items: keep the compatibility matrix current and
  verify profiling/benchmark tooling before any future precision change.

Scope:

- Inspect additional asiekierka/ws-test-suite ROM sources and identify their
  result protocols.
- Add protocol-specific pass/fail decoders to
  `crates/core/tests/public_roms.rs`.
- Keep public ROM binaries outside the repository; use env vars or the local
  `/Volumes/CrucialX6/roms/WonderSwan/Tests/...` convention.
- Reject unknown ROM protocols rather than falling back to generic HLT or WRAM
  conventions.
- Update `CompatibilityMatrix.md` for every newly automated public ROM.

Definition of done:

- Each new ROM has a source-confirmed result protocol documented in the test
  code.
- The focused ignored test passes locally with `--include-ignored`.
- `Status.md` and `CompatibilityMatrix.md` record the new evidence.

Triage notes:

- 2026-07-11: WSCpuTest was rechecked as a possible refresh path. The local
  `/Volumes/CrucialX6/roms/WonderSwan/Tests/WSCpuTest` source/ROM is the 2023
  release line, which is the last release to track here; do not chase a
  non-existent newer released WSCpuTest before moving on.
- 2026-07-11: `wonderful/libc/sbrk.ws` includes `test/pass_fail.h`, but
  `src/wonderful/libc/sbrk/main.c` never calls `draw_pass_fail`; it only prints
  the current `sbrk(0)` pointer and loops forever. It is intentionally not an
  oracle candidate under the current deterministic rules.
- 2026-07-11: Rechecked the remaining local ws-test-suite ROMs not already in
  `public_roms.rs`: `wonderful/libc/sbrk.ws`, `wonderful/benchmark/dma.ws`, and
  `tools/{eeprom_view_contents,hyper_voice_tester,power_draw_benchmark,startup_state_custom_crt0,timing_validator}.ws`.
  Their sources expose human-readable dumps, manual controls, or benchmark
  numbers, but no source-defined pass/fail condition suitable for the current
  deterministic oracle rules. The next expansion needs either a new upstream
  ws-test-suite ROM with an explicit result protocol or another public test ROM
  plus source/hardware notes defining its expected result.
- 2026-07-11: `mono/sound/quirks.ws` was promoted after adding APU
  CPU-visible sound-output readback ports, LFSR readback, and immediate
  noise-reset self-clear behavior; the evidence is recorded in
  `CompatibilityMatrix.md`.
- 2026-07-11: `color/dma/gdma_timing.wsc` was promoted after adding CPU-visible
  GDMA stall cycles and the APU fast-sweep test counter behavior used by the
  ROM's cycle-count harness; the evidence is recorded in `CompatibilityMatrix.md`.
- 2026-07-10: The remaining local asiekierka/ws-test-suite ROM with a
  source-confirmed `pass_fail.h` marker protocol was `mono/sound/quirks.ws`;
  it is now a passing oracle. Continue by inspecting additional upstream
  sources for deterministic protocols rather than broadening with unknown
  conventions.
- 2026-07-10: `mono/eeprom/internal.ws` was promoted after fixing console
  IEEPROM command-width, protected-range, and DONE-bit behavior; the evidence is
  recorded in `CompatibilityMatrix.md`.
- 2026-07-10: `wonderful/libc/sbrk.ws`,
  `wonderful/benchmark/dma.ws`, and the `tools/*` ROMs inspected locally do not
  expose a pass/fail protocol suitable for deterministic automation under the
  current oracle rules.

## P1 - CPU and bus timing only where visible

Instruction-total timing is now guarded by WSTimingTest pages 0-28. The
remaining risk is lower-level timing decomposition: fetch, memory access, I/O,
prefetch, DMA stall, and exact interaction with PPU/APU clocks.

The currently source-confirmed ws-test-suite CPU/interrupt candidates, plus the
RTC mapper, mono palette writemask, sprite scanline limit, cartridge EEPROM
1kbit/16kbit, Color tile/screen extended range, GDMA alignment/access, SDMA
sound DMA, and libc malloc candidates have been promoted to passing opt-in oracles. Continue selecting new candidates
from the upstream source tree before changing CPU, interrupt, or DMA timing
behavior.

Scope:

- Keep `docs/dev/V30MZ-Timing.md` as the timing ledger for instruction-level
  behavior.
- Use WSTimingTest regressions as a guardrail for CPU execution changes.
- Investigate DMA stalls, REP/string cadence, interrupt acknowledge timing, or
  bus wait behavior only when a test or title exposes a problem.
- Preserve `System::run_frame` and stable memory reads for future
  RetroAchievements integration.

Definition of done:

- Any timing change has a focused unit test, public-ROM oracle, hardware
  capture note, or documented title-specific reproduction.
- Broad per-clock decomposition remains deferred unless a concrete failure
  requires it.

## P1 - Color PPU hardware validation

Color rendering is implemented and covered by synthetic tests; the previously
listed low-level assumptions now have reference-emulator evidence.

Color-zero transparency, backdrop palette-index behavior, Color 4bpp tile
byte/nibble ordering, background tile-bank selection, and sprite attribute bit
meanings are now validated against ares and Mednafen source and recorded in
`CompatibilityMatrix.md`. The audio follow-up also now has a license-clean
Bus-level PCM fixture for deterministic sample-sequence coverage. Next concrete
task: finish **P1 - HyperVoice, SDMA, and analog audio validation** only if new
public-ROM, hardware-capture, or title-specific evidence appears; otherwise move
to the P2/P3 follow-up items below.

Scope:

- Add any further Color PPU rules only when a public ROM, hardware capture,
  reference-emulator discrepancy, or known title exposes a concrete issue.
- Keep mono regression tests paired with shared PPU changes.
- Revisit dot-level PPU only when a specific mid-scanline effect fails.

Definition of done:

- `CompatibilityMatrix.md` records the external evidence for each validated
  Color PPU rule.
- Any changed renderer behavior has deterministic regression coverage.

Validation notes:

- 2026-07-11: Color-mode color-zero transparency and backdrop palette indexing
  are source-confirmed against ares and Mednafen. ares `ares/ws/ppu/memory.cpp`
  uses `iram.read16(0xfe00 + (color << 1))` for the backdrop and only treats
  color index 0 as opaque in 2bpp palettes whose bit 2 is clear. Mednafen
  `src/wswan/gfx.cpp` seeds the Color framebuffer from `BGColor` high/low
  nibbles and only overwrites Color pixels when `wsTileRow[x]` is nonzero.
  Swanium has a focused regression
  `color_zero_screen_pixel_falls_back_to_color_backdrop`.
- 2026-07-11: Color 4bpp tile byte/nibble ordering is source-confirmed against
  ares and Mednafen. ares `ares/ws/ppu/memory.cpp` fetches planar rows from
  `0x4000 + (tile << 5) + (y << 2)` with bits `7-x`, `15-x`, `23-x`, `31-x`,
  and packed rows as high/low nibbles selected by `x`. Mednafen
  `src/wswan/tcache.cpp` cases 6/7 decode the same plane0..plane3 and
  high-nibble-left ordering. Swanium has focused regressions
  `color_4bpp_planar_renders_plane_bits_left_to_right` and
  `color_4bpp_packed_renders_nibbles_left_to_right`.
- 2026-07-11: Color background tile-map bank selection and sprite attribute
  meanings are source-confirmed against ares and Mednafen. ares
  `ares/ws/ppu/screen.cpp` folds background attribute bit 13 into the tile
  number, while `ares/ws/ppu/sprite.cpp` decodes sprite bit 12 as the window
  region, bit 13 as priority, and bits 14/15 as flips. Mednafen
  `src/wswan/gfx.cpp` passes background `0x2000` as the tile-bank flag but calls
  sprite `wsGetTile(..., 0)`, using the sprite attribute byte's `0x10`, `0x20`,
  `0x40`, and `0x80` bits for window, priority, hflip, and vflip. Swanium has
  focused regressions `color_2bpp_bank_bit_selects_second_tile_bank`,
  `color_4bpp_renders_pixel_from_second_tile_bank_area`, and
  `color_sprite_attribute_bit_13_is_priority_not_tile_bank`.

## P1 - HyperVoice, SDMA, and analog audio validation

HyperVoice 8-bit PCM, 16-bit direct output, and SDMA feeding are implemented.
The remaining uncertainty is validation quality, not basic feature presence.
SDMA sample cadence is now source-confirmed against ares and Mednafen and
recorded in `CompatibilityMatrix.md`. Port `0x9E` speaker main-volume was
triaged against ares, Mednafen, and MAME. MAME documents it as a WSC volume
setting; Swanium keeps software-visible readback but does not apply attenuation
to the core mix because emulators already expose arbitrary host/frontend volume
control. HyperVoice update cadence was triaged against ares and Mednafen; the
mature references disagree on `0x6A` bits 4-6, so Swanium keeps the current
Mednafen-like current-latch behavior. A license-clean Bus-level PCM fixture now
pins deterministic `0x89`, SDMA, and HyperVoice sample sequences without
committing a ROM binary. Next concrete task: do not change audio behavior
further unless a public test, hardware capture, or known-title discrepancy
exposes a concrete mismatch. If audio validation must continue, first promote
the fixture patterns into a public/self-built guest-code ROM so the CPU,
interrupt, and I/O path is covered in addition to the Bus-level sample sequence.

Scope:

- Promote the self-built PCM fixture patterns into a guest-code ROM only if a
  future issue needs CPU/interrupt-path coverage beyond the Bus-level sample
  sequence.
- Validate SDMA bus-stall behavior against public tests, reference emulators,
  or hardware captures.
- Keep port `0x9E` as software-visible volume-setting readback only; do not
  apply it to the core mix unless a concrete title/reference discrepancy shows
  software depends on mixer-side attenuation.
- Continue using `AudioAccuracy.md` for manual PCM comparison notes.

Definition of done:

- Audio behavior changes are backed by deterministic sample-level tests or
  recorded comparison evidence.
- Host-device resampling remains in `crates/audio`; deterministic core audio
  remains in `crates/core`.

Validation notes:

- 2026-07-11: SDMA sample cadence is source-confirmed against ares and
  Mednafen. ares `ares/ws/apu/dma.cpp` evaluates SDMA once per 24 kHz APU sample
  and maps rate bits 0/1/2/3 to 6/4/2/1 sample ticks. Mednafen
  `src/wswan/memory.cpp` reloads `SoundDMATimer` to 5/3/1/0 after each transfer,
  producing the same 6/4/2/1 call cadence. Swanium has the focused regression
  `sdma_rate_bits_select_apu_sample_divider`. CPU bus-stall behavior remains
  unvalidated.
- 2026-07-11: Port `0x9E` speaker main-volume was triaged against ares,
  Mednafen, and MAME. ares `ares/ws/apu/io.cpp` reads/writes
  `io.masterVolume` on non-ASWAN SoCs, and `ares/ws/apu/apu.cpp` applies it as
  final stream amplitude. Mednafen does not implement `0x9E` in
  `wswan/sound.cpp`; its bundled `wstech24.txt` lists `0x9E` as default `0x03`
  unknown. MAME `src/mame/bandai/wswan.cpp` comments `0x9e/2` as the WSC volume
  setting. Swanium treats it as a BIOS/body volume setting: it keeps low-two-bit
  readback for software visibility, but does not apply it to the deterministic
  core mix because emulator users can freely control output volume at the
  frontend/host layer.
- 2026-07-11: HyperVoice update cadence was triaged against ares and Mednafen.
  ares `ares/ws/apu/channel5.cpp` treats `0x6A` bits 4-6 as speed divisors
  `{1,2,3,4,5,6,8,12}` and only updates changed L/R outputs when that divider
  elapses. Mednafen `src/wswan/sound.cpp` ignores bits 4-6, updates HyperVoice
  at `WSwan_SoundUpdate()` timestamps, and uses `0x6A` only for enable,
  extension mode, and shift. Since the references disagree and no public ROM or
  hardware capture pins software-visible timing, Swanium keeps the Mednafen-like
  current-latch behavior. Focused regression:
  `hypervoice_speed_bits_do_not_change_current_latch_output`.
- 2026-07-11: Added `crates/core/tests/pcm_fixture.rs`, a license-clean
  Bus-level PCM sample-sequence oracle. It pins CPU `0x89` voice writes
  `0xC0,0x80,0x40` as `(2048,2048),(2048,2048),(-2048,-2048)`, fastest SDMA-fed
  voice writes `0xC0,0x40` as `(2048,2048),(0,0)`, and HyperVoice latch writes
  `0x10,0x20` as `(4096,4096),(8192,8192)`. A future public/self-built ROM can
  reuse the same patterns if guest-code-path coverage becomes necessary.

## P2 - Compatibility matrix and local evidence

The compatibility matrix is useful only if it stays current and
license-clean. Current actionable state: no new compatibility row is needed
until a public test, synthetic regression, manual smoke check, or source-triage
result is actually performed. When continuing without new evidence, move to
**P2 - Performance after precision work**.

Scope:

- Add rows for public tests, synthetic tests, and manual smoke checks as they
  are performed.
- Keep commercial ROM assets and screenshots out of the repository.
- Prefer public test ROMs over commercial-ROM observations when both can cover
  the same behavior.
- Maintain any local ROM manifest outside the repository.

Definition of done:

- `CompatibilityMatrix.md` clearly states the source, expected result, current
  result, and notes for each evidence row.

## P2 - Performance after precision work

Precision work can easily regress performance. The current scanline renderer and
profiling tools should remain the default path unless exact timing evidence
requires more. Current actionable state: the profiling feature and Criterion
bench definitions already exist; periodically verify they still build after
core changes, but do not add performance work without a measured regression or a
planned precision rewrite.

Scope:

- Use the `swanium-core` `profiling` feature and Criterion benches before and
  after large timing, PPU, SDMA, or audio changes.
- Preserve the scanline fast path for software that does not need dot-level
  effects.
- Add focused benches for any new dot-level, SDMA-heavy, or sprite-heavy path.

Definition of done:

- Large precision changes include before/after profiling notes in
  `Profiling.md` or the relevant change record.

Validation notes:

- 2026-07-11: P0/P1 precision, PPU, and audio follow-ups were reclassified as
  evidence-driven. The next session should not re-triage the same local public
  ROMs or reference-source rules unless new inputs appear; run the profiling
  example/benches only as a tooling check or before/after record for a concrete
  performance-sensitive change.
- 2026-07-11: The profiling example was checked with the built-in synthetic ROM
  (`cargo run -p swanium-core --features profiling --example profile --release`).
  During that check, CPU/APU bucket overlap was fixed so the in-core profiler
  reports exclusive CPU, PPU, APU, and DMA shares. Criterion benches remain the
  tool for before/after statistical regression tracking.
- 2026-07-11: Criterion frame benchmarks were build-checked with
  `cargo bench -p swanium-core --bench frame --no-run`. This verifies the
  `run_frame`, `render_scanline`, and `tick_apu_frame` bench definitions still
  compile under the bench profile. Do not spend the next session running full
  Criterion measurements unless there is a concrete before/after performance
  question.
- 2026-07-11: Full workspace validation passed after the P0/P1 evidence-driven
  cleanup and P2 tooling checks: `cargo test --workspace` and
  `cargo clippy --workspace --all-targets -- -D warnings`. Do not repeat this
  as the next task unless new code changes land; the next useful step needs new
  compatibility evidence, a concrete frontend polish request, or a planned
  performance/precision change.

## Deferred frontend polish

Frontend Phase 7 is usable. The remaining listed polish is non-blocking for
emulation correctness.

- Bicubic renderer / shader-style scaling remains deferred until a future wgpu
  rendering path exists.
- Save states, rewind, cheats, debugger, and developer tools remain future
  product features rather than current emulator-core blockers.
