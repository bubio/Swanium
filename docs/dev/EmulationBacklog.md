# Emulation backlog

Last updated: 2026-07-07.

This backlog tracks emulator-core work that remains after the minimal playable
application milestone. GUI polish, packaging, and frontend convenience features
belong in `Status.md` or separate UI issues; this file is only for emulation
correctness, compatibility, determinism, and hardware coverage.

Current implementation status is summarized in `docs/dev/Status.md`. Execution
order and milestone definitions are tracked in `docs/dev/EmulationPlan.md`.

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

### PPU dot-level timing

Rendering is scanline-driven. This is sufficient for many games, but it cannot
model mid-scanline register effects exactly.

Milestone 10 decided **not to move to a dot-level PPU core yet**. The current
evidence only requires scanline-boundary behavior: `System::run_frame_traced`
now has regression coverage for trace coverage, CPU-written scroll state before
line rendering, line-compare IRQs, HBlank timer frame coverage, and VBlank IRQ
timing. The hardware's 32-sprites-per-scanline limit is implemented in the
scanline renderer.

Expected scope:

- Preserve the existing scanline-optimized renderer behavior for games that do
  not need dot-level effects.
- Revisit dot-level timing only when a public test ROM, hardware capture, or
  known title demonstrates a mid-scanline register effect that the current
  renderer cannot represent.

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

### Color PPU edge cases

Color display works for real WSC ROMs, including palette RAM and 2bpp/4bpp tile
modes. Milestone 10 added synthetic regression coverage for the Color PPU
assumptions currently modeled in the core: color-zero transparency, backdrop
palette indexing, 4bpp planar byte order, 4bpp packed nibble order, background
tile-bank selection, and the sprite bit-13 priority meaning.

Expected scope:

- Validate these Color PPU rules against public test ROMs, hardware captures, or
  multiple reference emulators; the current checks are implementation-level
  tests rather than hardware proof.
- Keep mono compatibility tests paired with Color tests when changing shared PPU
  logic.

### HyperVoice and WSC audio validation

HyperVoice 8-bit PCM, 16-bit direct output, and SDMA feeding are implemented.
The remaining risk is validation against public ROMs, reference emulators, or
hardware captures rather than untriaged register coverage.

Expected scope:

- Confirm the sample-rate divisor/update cadence if software-visible. The
  current 8-bit path follows the existing Mednafen-style latch conversion, while
  the 16-bit direct path at `0x64`-`0x67` writes signed left/right output words.
- Validate the exact analog transfer curve for port `0x9E`. It is implemented
  as the documented built-in speaker main-volume register, with low two bits
  retained for readback. Attenuation is deliberately not applied yet because
  the exact analog curve and zero-write behaviour need hardware validation.
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

Milestone 13 is the current emulator-focused milestone. Keep the next work tied
to evidence:

1. Use `docs/dev/V30MZ-Timing.md` to audit CPU/bus timing gaps.
2. Expand public-ROM coverage when a source-confirmed result protocol is known.
3. Revisit dot-level PPU, exact SDMA cadence, or audio analog behaviour only when
   tests, hardware captures, or known titles show a software-visible issue.
