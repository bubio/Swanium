# Emulation backlog

Last updated: 2026-07-07.

This backlog tracks emulator-core work that remains after the minimal playable
application milestone. GUI polish, packaging, and frontend convenience features
belong in `Status.md` or separate UI issues; this file is only for emulation
correctness, compatibility, determinism, and hardware coverage.

Execution order and milestone definitions are tracked in
`docs/dev/EmulationPlan.md`.

## Priority guide

- **P0**: likely to break real software or block meaningful compatibility
  testing.
- **P1**: affects accuracy or important software classes, but has a usable
  workaround or narrower blast radius.
- **P2**: quality, validation, performance, or long-tail hardware behavior.

## P0 — compatibility blockers

### Public ROM verification harness

The WSCpuTest opt-in path is meaningful and verified. `ws-test-suite` now has
one decoded oracle (`mono/cpu/80186_quirks.ws`), but broader suite coverage still
needs source-confirmed protocols before it can act as a general regression
signal.

Expected scope:

- Confirm the official result protocol for additional selected `ws-test-suite`
  ROMs.
- Add protocol-specific pass/fail decoding for each selected ROM in
  `crates/core/tests/public_roms.rs`.
- Keep ROMs env-gated and uncommitted per `tests/README.md`.
- Keep unknown ROMs rejected rather than falling back to generic HLT/WRAM
  conventions.

## P1 — accuracy and hardware coverage

### PPU dot-level and sprite limits

Rendering is scanline-driven. This is sufficient for many games, but it cannot
model mid-scanline register effects exactly and it does not yet enforce the
hardware's 32-sprites-per-scanline limit.

Expected scope:

- Decide whether the next step is full dot-level PPU timing or a narrower
  scanline renderer enhancement.
- Enforce the 32 visible sprites per scanline limit in OAM order.
- Preserve the existing scanline-optimized renderer behavior for games that do
  not need dot-level effects.
- Add regression tests for sprite overflow ordering and raster register changes
  around HBlank/line-compare timing.

### CPU and bus timing decomposition

The CPU now uses V30MZ instruction-level cycle counts and interleaves APU ticking
after each instruction. The remaining gap is per-clock bus/PPU/APU interaction:
instruction costs are not decomposed into fetch, memory access, I/O, and prefetch
events.

Expected scope:

- Decompose timing only where software-visible behavior requires it, rather than
  rewriting the CPU for theoretical purity.
- Audit taken branches, REP/string I/O, interrupt acknowledge, and DMA stalls
  against the V30MZ timing notes.
- Keep the frame-boundary `System::run_frame` API stable for RetroAchievements
  compatibility.

### RTC protocol and alarm IRQ validation

The cartridge RTC is implemented with deterministic injected time and free-runs
from the emulated master clock. Remaining uncertainty is around protocol details
and interrupt behavior.

Expected scope:

- Verify footer RTC detection bit, command codes, byte order, status bits, and
  read/write sequencing against hardware, test ROMs, or multiple reference
  emulators.
- Wire alarm-match IRQ behavior if software is found to depend on it.
- Design a versioned save-data framing format if RTC state must be persisted
  alongside SRAM/EEPROM in one frontend save file.

### Color PPU edge cases

Color display works for real WSC ROMs, including palette RAM and 2bpp/4bpp tile
modes. Several low-level rules are still documented assumptions.

Expected scope:

- Verify color transparency rules, backdrop palette indexing, and 4bpp
  planar/packed byte order with test ROMs or reference captures.
- Validate tile bank selection and sprite attribute interpretation in Color mode.
- Keep mono compatibility tests paired with Color tests when changing shared PPU
  logic.

### HyperVoice and WSC audio extensions

HyperVoice 8-bit PCM and SDMA feeding are implemented, but several WSC audio
paths remain unverified or missing.

Expected scope:

- Implement or explicitly rule out the 16-bit direct output path at ports
  `0x64`-`0x67`.
- Verify the HyperVoice data-port choice and sample-extension mode semantics.
- Implement the WSC master volume bits at port `0x9E`.
- Confirm the sample-rate divisor behavior if software-visible.
- Validate SDMA's exact bus-stall and sample-cadence timing against public tests,
  reference emulators, or hardware captures. The current implementation feeds one
  byte every `128 * rate` master cycles and is covered by register/terminal-count
  tests, but not by a public SDMA ROM yet.

## P2 — quality, validation, and long-tail work

### Audio reconstruction quality

The current voice PCM path reconstructs direct `0x89` writes well enough for
known multiplexed streams, but residual ripple from scanline-jittered write
timing remains open.

Expected scope:

- Capture representative PCM-heavy games and compare against Mednafen/ares or
  hardware recordings.
- Consider a band-limited resampler or a more hardware-like analog-output model.
- Keep deterministic core output; host-device resampling belongs in
  `crates/audio`.

### Mapper and cartridge edge validation

Bandai 2001/2003, SRAM, EEPROM, and RTC are implemented, but some details are
reference-derived rather than hardware-verified.

Expected scope:

- Validate Bandai 2003 high-byte bank ports on known large ROMs.
- Expand EEPROM protocol tests with real save initialization patterns.
- Confirm open-bus behavior for absent cart peripherals and mapper-specific
  register reads.

### Internal EEPROM persistence

The console internal EEPROM is modeled enough for BIOS startup paths, but
frontend persistence and full configuration behavior are not yet treated as a
compatibility feature.

Expected scope:

- Decide whether internal EEPROM should be persisted per configured console
  model.
- If persisted, keep it separate from cartridge save media.
- Add tests for BIOS configuration writes once a stable boot-ROM fixture policy
  exists.

### Test ROM corpus and compatibility matrix

The core has strong unit coverage, and `docs/dev/CompatibilityMatrix.md` now
seeds the external-ROM matrix. It still needs more public fixtures and manual
smoke rows as compatibility work expands.

Expected scope:

- Maintain a local, uncommitted manifest of public or user-provided test ROMs.
- Record expected pass/fail/status for CPU, PPU, APU, DMA, RTC, and Color tests.
- Prefer small public test ROMs over commercial-ROM screenshots when possible.
- Document commercial-ROM manual checks separately from automated CI tests.

### Performance after precision work

Current PPU and frame profiling infrastructure is in place. Any move toward
dot-level timing, SDMA, or more accurate audio should be profiled before and
after.

Expected scope:

- Use `swanium-core`'s `profiling` feature and Criterion benches before large
  timing rewrites.
- Preserve the current scanline fast path where exact mid-line behavior is not
  needed.
- Add focused benchmarks for SDMA-fed audio and sprite-heavy scenes.

## Suggested next milestone

The next emulator-focused milestone should be:

1. Implement Sound DMA feeding into voice PCM.
2. Replace the `ws-test-suite` placeholder with a real pass/fail decoder.
3. Add a small compatibility matrix covering CPU, SDMA audio, mono PPU, Color
   PPU, RTC, and mapper saves.

That milestone gives the project a stronger compatibility signal before taking
on larger timing rewrites.
