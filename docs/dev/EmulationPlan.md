# Emulation development plan

Last updated: 2026-07-07.

This document turns `docs/dev/EmulationBacklog.md` into an execution plan. The
goal is not to keep adding features blindly; it is to raise confidence that the
emulated hardware is correct enough to run a broad WonderSwan / WonderSwan Color
library and to keep regressions measurable.

## Guiding rules

- Keep `swanium-core` deterministic and platform-independent.
- Preserve the frame-boundary API (`System::run_frame`, stable memory reads) for
  future RetroAchievements integration.
- Prefer small, protocol-level tests before commercial-ROM visual checks.
- Treat reference-emulator behavior as evidence, not as the final spec, when a
  public test ROM or hardware observation is available.
- Do not expand frontend scope unless it is required to expose or persist an
  emulation feature.

## Milestone 9 — Compatibility baseline

Purpose: remove the two highest-risk gaps that make compatibility results hard
to trust: missing Sound DMA and weak public-ROM pass/fail decoding.

### 9a. Sound DMA implementation

Scope:

- Implement SDMA transfer state using ports `0x4A`-`0x52`.
- Route each delivered sample through the same logic as a CPU write to port
  `0x89`, so `Apu::write_voice` receives the real PCM write stream.
- Preserve register masking already present for source/counter segment ports.
- Decide, document, and test terminal-count behavior and enable-bit clearing.
- Keep GDMA synchronous on port `0x48`; do not couple SDMA to the GDMA path.

Implementation notes:

- Start in `crates/core/src/bus/mod.rs`, because the register shadows and the
  voice-port write hook already live there.
- Add a small internal helper for "write voice data latch" to avoid duplicating
  the port `0x89` side effect between CPU I/O and SDMA.
- Clock SDMA from the APU/frame-driving path only after confirming the expected
  cadence from WSdev / STS / WonderCrab / reference emulator behavior. If the
  cadence remains uncertain, implement the best-documented behavior and mark the
  exact timing as a follow-up in `EmulationBacklog.md`.

Tests:

- Unit tests for SDMA register masking and no-op behavior when disabled.
- Integration tests proving SDMA copies bytes from memory into the voice data
  latch and produces non-silent voice samples when voice mode is enabled.
- Regression test for terminal count and disable behavior.

Definition of done:

- `cargo test -p swanium-core sdma` passes.
- Existing APU direct-voice tests remain unchanged in behavior.
- `Status.md` and `EmulationBacklog.md` are updated to move SDMA out of P0.

### 9b. Public ROM result decoding

Scope:

- Confirm result conventions for selected `ws-test-suite` ROMs.
- Replace the placeholder HLT + `WRAM[0x0000] == 0` assertion with explicit
  pass/fail decoding.
- Keep the tests opt-in and env-gated; do not commit public ROM binaries.
- Add a short local-fixture policy note if a selected suite needs multiple ROMs.

Tests:

- `cargo test -p swanium-core --test public_roms -- --include-ignored wscputest`
  with `WS_CPU_TEST_ROM` set.
- `cargo test -p swanium-core --test public_roms -- --include-ignored ws_test_suite`
  with the selected `WS_TEST_SUITE_ROM` set.

Definition of done:

- WSCpuTest remains green.
- At least one `ws-test-suite` ROM has a real decoded pass/fail oracle.
- The `TODO(issue)` placeholders are either removed or replaced with issue
  numbers for concrete remaining ROM protocols.

### 9c. Compatibility matrix seed

Scope:

- Create `docs/dev/CompatibilityMatrix.md`.
- Track only license-clean metadata: ROM/test name, source, required env var or
  local path convention, subsystem coverage, expected result, current result,
  and notes.
- Separate automated opt-in tests from manual commercial-ROM smoke checks.

Definition of done:

- CPU, SDMA audio, mono PPU, Color PPU, RTC, and mapper/save rows exist, even if
  some start as "needs fixture".
- The matrix is referenced from `Status.md`.

## Milestone 10 — PPU correctness pass

Purpose: close visible rendering gaps that are likely to affect games before
attempting a full dot-level rewrite.

Status: complete at scanline-renderer scope. The 32-sprites-per-scanline limit
is implemented and tested; scanline-boundary raster behavior is covered by
`System::run_frame_traced` / frame-driver regressions; Color PPU assumptions are
pinned by synthetic tests. A dot-level PPU rewrite remains deferred until a
specific public test, hardware capture, or known title requires mid-scanline
effects.

### 10a. Sprite overflow behavior

Scope:

- Enforce the hardware limit of 32 sprites per scanline in OAM order.
- Keep current priority and transparency behavior intact.
- Add tests for overflow ordering and priority interaction.

Definition of done:

- Synthetic OAM tests prove sprite 33+ on a line are ignored.
- Existing PPU integration hashes or framebuffer assertions still pass.

### 10b. Raster-effect audit

Scope:

- Use `System::run_frame_traced` and focused ROM/code snippets to identify which
  mid-frame effects are already correct at scanline granularity.
- Add tests around line compare, HBlank timer, VBlank timing, and scroll changes
  at scanline boundaries.
- Decide whether a dot-level PPU core is justified now or should remain deferred
  until a known failing title/test requires it.

Definition of done:

- A short "dot-level required/not yet required" decision is added to
  `EmulationBacklog.md`.
- Any discovered scanline-boundary bugs have regression tests.

### 10c. Color PPU assumption validation

Scope:

- Validate color transparency, backdrop palette index, 4bpp planar byte order,
  4bpp packed nibble order, tile bank selection, and sprite attribute meanings.
- Prefer public test ROMs or small locally built ROMs. If reference emulators are
  used, record which ones and the observed behavior.

Definition of done:

- Each assumption currently documented in Phase 8b/8c is either verified or
  explicitly kept as an open risk with a test plan.
- Mono regression tests are paired with any shared PPU changes.

## Milestone 11 — Audio and WSC extension pass

Purpose: finish the remaining WSC audio paths after SDMA establishes the
sample-feeding path.

Status: complete at implementation/test-triage scope. The 16-bit HyperVoice
direct-output path is implemented and covered by deterministic tests. Port
`0x9E` is implemented as the WonderSwan built-in speaker main-volume register:
all models keep the low two bits for software-visible readback. The analog
speaker-volume transfer is not applied to the mix yet because the exact curve
and reset/write behaviour remain hardware-uncertain and a literal 0=mute mapping
breaks existing software. PCM quality measurement is documented in
`docs/dev/AudioAccuracy.md`; actual commercial-ROM comparisons remain
local/manual evidence.

### 11a. WSC master volume and direct output

Scope:

- Implement port `0x9E` master-volume behavior if confirmed software-visible.
- Implement or explicitly rule out the 16-bit direct output path at ports
  `0x64`-`0x67`.
- Verify HyperVoice data port and sample-extension mode behavior.

Definition of done:

- Focused unit tests cover the implemented register semantics.
- `EmulationBacklog.md` no longer lists these as untriaged unknowns.

### 11b. PCM quality measurement

Scope:

- Pick two or three PCM-heavy titles or public tests as manual/audio fixtures.
- Compare current output against Mednafen/ares or hardware recordings.
- Decide whether the next improvement is in core reconstruction, host resampling,
  or documentation of acceptable approximation.

Definition of done:

- `docs/dev/AudioAccuracy.md` records the fixtures, observed issues, and chosen
  next step.
- Any core audio change has deterministic sample-level tests.

## Milestone 12 — Cartridge, RTC, and persistence pass

Purpose: tighten save-related hardware behavior without compromising the simple
core save API.

Status: complete. Cartridge save persistence is implemented for both SRAM and
cartridge EEPROM: the core exposes raw bytes through `save_data` / `load_save_data`,
and the frontend reads/writes `saves/<ROM file name>.sav` under the platform config directory. RTC footer
detection, the 0xCA/0xCB command/data protocol, BCD byte order, status/alarm
registers, and absent-device open-bus behavior are pinned by tests. RTC state is
kept separate from cartridge SRAM/EEPROM save bytes. Bandai 2003 high-byte bank
selection is covered on a large ROM image, and cartridge EEPROM tests cover
common initialization commands plus absent-device open bus. Console internal
EEPROM is a separate BIOS/profile device, not cartridge save media; Swanium
keeps it deterministic and zero-filled at startup.

### 12a. RTC protocol validation

Scope:

- Verify RTC footer detection, command codes, data order, status bits, and
  readable/writeable command sequencing.
- Model alarm registers as RTC state; do not raise a cartridge IRQ from alarm
  match.
- Keep RTC state outside the raw cartridge SRAM/EEPROM save byte slice.

Definition of done:

- RTC assumptions in `DevelopmentPlan.md` / `Status.md` are updated with
  the Milestone 12 behavior.
- Tests cover any protocol changes.

### 12b. Mapper and EEPROM validation

Scope:

- Validate Bandai 2003 high-byte bank ports on large ROMs.
- Expand EEPROM tests around real initialization patterns and absent-device
  open-bus behavior.
- Keep mapper behavior deterministic and independent of frontend state.

Definition of done:

- Compatibility matrix has at least one mapper/save row for each supported save
  medium class: SRAM, cart EEPROM, RTC-bearing cart, and no-save cart.

### 12c. Internal EEPROM persistence decision

Scope:

- Treat console internal EEPROM as BIOS/profile state, not cartridge save media.
- Document console internal EEPROM as BIOS/profile state, separate from
  cartridge saves.
- Keep console internal EEPROM deterministic and zero-filled at startup.

Definition of done:

- The decision is recorded in `Status.md`.

## Milestone 13 — Timing precision phase

Purpose: move from instruction-level timing toward finer hardware scheduling only
where there is evidence that it matters.

Scope:

- Audit taken branches, REP/string I/O, interrupt acknowledge, DMA stalls, and
  I/O write visibility.
- Use `docs/dev/V30MZ-Timing.md` as the working timing ledger.
- Decompose instruction timing only for operations whose internal timing is
  software-visible.
- Keep `System::run_frame` as the public stepping boundary; internal scheduling
  may become finer-grained.

Definition of done:

- A known failing timing-sensitive test/title improves, or the milestone exits
  with a documented "no proven need yet" decision.
- Criterion/frame profiling is captured before and after any scheduler rewrite.
- No frontend API churn is required for timing changes.

## Release gates for an emulator-focused build

Before calling the emulator side "v1-compatible enough", require:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- Opt-in WSCpuTest pass.
- At least one decoded `ws-test-suite` pass.
- Compatibility matrix updated for the tested ROM set.
- Manual smoke checks for a small set of mono and Color commercial titles, with
  results recorded but no ROM assets committed.

## Recommended immediate sequence

1. Milestone 9a: implement and test SDMA.
2. Milestone 9b: make `ws-test-suite` pass/fail decoding real.
3. Milestone 9c: create the compatibility matrix while the test evidence is
   fresh.
4. Milestone 10a: enforce sprite-per-line overflow.
5. Reassess PPU dot-level timing after the matrix shows real failures.
