# Audio accuracy notes

Last updated: 2026-07-11.

This document records manual PCM/audio fixtures and the current decision for the
next audio-quality step. It intentionally stores only license-clean metadata:
fixture names, what to listen/measure for, comparison targets, and conclusions.
Do not commit commercial ROMs, recordings, or extracted assets.

## Current implementation summary

- Channel 2 voice PCM is signed (`0x80` = silence) and is reconstructed from the
  raw `0x89` write stream through a 2-tap moving average plus compensating gain.
- The frame driver advances the APU after each CPU instruction, so HBlank-driven
  PCM writes land in the audio timeline instead of being collapsed to scanline
  batches.
- Host output uses the `crates/audio` linear 24 kHz to device-rate resampler.
- WSC HyperVoice supports the 8-bit latch path, SDMA feeding, and the signed
  16-bit direct-output path at ports `0x64`-`0x67`.

## Fixture candidates

| Fixture | Type | Coverage | Comparison target | Status | Notes |
|---|---|---|---|---|---|
| *Last Alive* | Local commercial ROM | HBlank-timer voice PCM, time-multiplexed `0x89` writes | Mednafen/ares recording or hardware capture | Selected manual fixture | Previously used to identify scanline-batched APU timing and multiplex buzz. Recheck residual ripple after any reconstruction change. |
| SDMA/HyperVoice-heavy WSC title | Local commercial ROM or future public ROM | Sound DMA feeding, HyperVoice color-mode gate, direct/8-bit PCM paths | Mednafen/ares recording or hardware capture | Needs selection | Prefer a public fixture before relying on commercial-ROM evidence. |
| Self-built PCM fixture | Swanium core integration test | Deterministic sample-level regression for known `0x89`, SDMA, and HyperVoice write patterns | Analytical expected samples | Added as `crates/core/tests/pcm_fixture.rs` | License-clean Bus-level fixture; future work can promote the same patterns into a public/self-built ROM oracle. |
| Public synthetic PCM ROM | Public/self-built ROM | Guest-code oracle for the same `0x89`, SDMA, and HyperVoice write patterns | Analytical expected samples | Future fixture | Only needed if we want to validate the CPU/interrupt/guest-code path in addition to the deterministic Bus-level sample sequence. |

## Observed issues

- The current voice reconstruction removes the strongest multiplex buzz, but
  residual ripple from scanline-jittered write timing remains possible.
- HyperVoice update cadence is reference-triaged but not hardware-validated.
  ares applies `0x6A` bits 4-6 as extra speed divisors, while Mednafen ignores
  them and updates from the current latch/direct value at sound-update
  timestamps. Swanium keeps the Mednafen-like current-latch behavior until a
  public ROM, hardware capture, or known-title discrepancy proves the divider is
  software-visible.
- SDMA sample cadence is source-confirmed against ares and Mednafen as 24 kHz
  APU sample ticks divided by 6/4/2/1 for rate bits 0/1/2/3. CPU bus-stall
  behavior remains unvalidated.
- Port `0x9E` is implemented as the built-in speaker main-volume register. It
  keeps the documented low two bits for readback, but the value is not applied
  to the audio mix. ares applies it as a final stream amplitude on non-ASWAN
  SoCs, while Mednafen does not implement it in the audio path and its bundled
  tech note lists the port as unknown. MAME comments it as the WSC volume
  setting. Swanium treats it as a BIOS/body volume setting that matters on real
  hardware but is redundant for an emulator, where the frontend/host can adjust
  output volume freely. Treating value 0 as literal mute has been observed to
  break software that initializes the register to zero.

## Decision

The next improvement should be measurement-led, not another unconditional core
filter change. Priority order:

1. Promote the self-built PCM fixture patterns into a guest-code ROM only if an
   audio issue needs CPU/interrupt-path coverage beyond the Bus-level sample
   sequence.
2. Capture short Mednafen/ares comparisons for *Last Alive* and one WSC
   SDMA/HyperVoice-heavy title, then decide whether the remaining difference is
   core reconstruction or host resampling.
3. Only change `crates/audio` resampling if the core sample stream already
   matches the reference closely and the audible issue appears after host-rate
   conversion.
4. Keep port `0x9E` as readback-only BIOS/body volume-setting state in the core;
   host/frontend volume control is the correct place for emulator-wide output
   attenuation unless software proves it depends on mixer-side behavior.
