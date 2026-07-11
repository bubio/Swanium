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
or upstream changes have been added. The next concrete emulator work should move
to **P1 - Color PPU hardware validation**, starting with one visible rule from
that section and recording the evidence in `CompatibilityMatrix.md`.

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

Color rendering is implemented and covered by synthetic tests, but several rules
still need external confirmation.

Color-zero transparency and backdrop palette-index behavior are now validated
against ares and Mednafen source and recorded in `CompatibilityMatrix.md`. Next
concrete task: validate Color 4bpp planar byte order and packed nibble order
against a public ROM, hardware capture, or at least two mature reference
emulators. If the evidence confirms current behavior, add a
`CompatibilityMatrix.md` row; if it disagrees, change the renderer with a
focused regression test.

Scope:

- Validate 4bpp planar byte order, 4bpp packed nibble order, background
  tile-bank selection, and sprite attribute bit meanings against public ROMs,
  hardware captures, or multiple reference emulators.
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

## P1 - HyperVoice, SDMA, and analog audio validation

HyperVoice 8-bit PCM, 16-bit direct output, and SDMA feeding are implemented.
The remaining uncertainty is validation quality, not basic feature presence.

Scope:

- Confirm the sample-rate divisor/update cadence if software-visible.
- Validate SDMA bus-stall behavior and sample cadence against public tests,
  reference emulators, or hardware captures.
- Validate port `0x9E` speaker main-volume analog transfer and zero-write
  behavior before applying it to the mix.
- Continue using `AudioAccuracy.md` for manual PCM comparison notes.

Definition of done:

- Audio behavior changes are backed by deterministic sample-level tests or
  recorded comparison evidence.
- Host-device resampling remains in `crates/audio`; deterministic core audio
  remains in `crates/core`.

## P2 - Compatibility matrix and local evidence

The compatibility matrix is useful only if it stays current and
license-clean.

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
requires more.

Scope:

- Use the `swanium-core` `profiling` feature and Criterion benches before and
  after large timing, PPU, SDMA, or audio changes.
- Preserve the scanline fast path for software that does not need dot-level
  effects.
- Add focused benches for any new dot-level, SDMA-heavy, or sprite-heavy path.

Definition of done:

- Large precision changes include before/after profiling notes in
  `Profiling.md` or the relevant change record.

## Deferred frontend polish

Frontend Phase 7 is usable. The remaining listed polish is non-blocking for
emulation correctness.

- Bicubic renderer / shader-style scaling remains deferred until a future wgpu
  rendering path exists.
- Save states, rewind, cheats, debugger, and developer tools remain future
  product features rather than current emulator-core blockers.
