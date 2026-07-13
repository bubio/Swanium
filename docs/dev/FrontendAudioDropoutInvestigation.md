# Frontend audio dropout investigation

Date: 2026-07-13

## Summary

Linux audio output is available and `cpal` can open the stream, but the Slint GUI
path can run substantially below the WonderSwan frame cadence. Because the
current frontend advances emulation, pushes audio, and uploads the framebuffer
from the same GUI timer callback, low GUI FPS directly starves the audio ring
buffer and produces audible dropouts.

This is not primarily an APU-silence issue. The user later confirmed audio was
present but quiet; the remaining observed problem is crackling / frequent
dropout.

## Observed facts

- Windows and macOS builds reportedly produce audio.
- On the tested Linux desktop, YouTube and system audio work.
- From a normal desktop process, the Linux audio stack is reachable:
  - `pactl info` reports `PulseAudio (on PipeWire 1.0.0)`.
  - `pw-cli info 0` connects to `pipewire-0`.
  - `aplay -L` lists `default` as `Default ALSA Output (currently PipeWire Media Server)`.
- The earlier `aplay -l` / `pactl` failures were from the restricted agent
  sandbox and should not be treated as host-audio evidence.
- Running the frontend directly outside the sandbox logged:

```text
audio stream opened device_rate=44100
```

So `AudioStream::open()` succeeds on Linux in this environment.

## Current frontend scheduling shape

The frontend timer currently does one emulated frame and one UI upload in the
same callback:

```text
Slint timer tick
  -> system.run_frame(keys)
  -> audio.push(system.audio_samples())
  -> system.clear_audio_samples()
  -> update_window_frame(...)
```

Relevant implementation points:

- `crates/frontend/src/main.rs`
  - `POLL_INTERVAL = 4 ms`
  - `audio_stream` is opened once with `audio::AudioStream::open()`
  - frame pacing checks `audio.ring_fill()`
  - after every `System::run_frame`, the frontend pushes the accumulated APU
    samples and then uploads a new Slint image
- `crates/audio/src/stream.rs`
  - `AudioStream` uses a fixed-size ring buffer.
  - cpal underruns are padded with silence.

This means rendering throughput controls audio production throughput. If the GUI
only reaches ~40 frames/s, the core only produces ~40 frames/s worth of audio.
The output device continues consuming at 44.1/48 kHz, so the ring repeatedly
drains to silence.

## Measurements

Using the user-provided local ROM:

```text
/home/bubio/ROMs/WonderSwan/Wizardry_-_Scenario_1_-_Kyounou_no_Shiren_Jou_(J)_[M][!].wsc
```

Headless core profiler:

```sh
cargo run -p swanium-core --features profiling --example profile --release -- \
  '/home/bubio/ROMs/WonderSwan/Wizardry_-_Scenario_1_-_Kyounou_no_Shiren_Jou_(J)_[M][!].wsc'
```

Result:

```text
600 frames, 13.472 ms/frame | CPU 53.9% PPU 16.2% APU 29.5% DMA 0.0% | 7540672 insns
  CPU    7.268 ms/frame (53.9%)
  PPU    2.181 ms/frame (16.2%)
  APU    3.979 ms/frame (29.5%)
  DMA    0.000 ms/frame ( 0.0%)
  → 74 frames/s headroom (target 75)
```

Debug core profiler result for the same ROM:

```text
600 frames, 193.906 ms/frame | CPU 28.5% PPU 15.3% APU 55.9% DMA 0.0% | 7540672 insns
  CPU   55.259 ms/frame (28.5%)
  PPU   29.746 ms/frame (15.3%)
  APU  108.401 ms/frame (55.9%)
  DMA    0.000 ms/frame ( 0.0%)
  → 5 frames/s headroom (target 75)
```

The user clarified that their problematic build is a CI release artifact, not a
debug build, and observed roughly 40 FPS in the GUI. Therefore the actionable
problem is not “debug is slow”; it is “the full GUI path is below the audio
cadence even when core-only release is near the target”.

## Interpretation

WonderSwan runs at about 75 frames/s. In this frontend design, one emulated frame
is also the unit that produces one batch of APU samples for the host audio ring.
At ~40 GUI FPS, only about 53% of the required audio batches are generated per
second. The ring buffer cannot compensate for sustained underproduction; it can
only absorb short jitter.

Core-only release is already near the target, but with little margin. Any
additional GUI cost from Slint image upload, scaling, compositing, status updates,
input polling, or window-system overhead can push total throughput below the
required cadence.

## Recommended fix order

### 1. Add frontend timing instrumentation

Add a gated diagnostic mode that records, per rendered frame:

- `System::run_frame` duration
- `AudioStream::push` duration
- `update_window_frame` duration
- produced sample count
- non-zero sample count
- ring fill level before/after push
- displayed FPS

Keep it behind an environment variable such as `SWANIUM_FRONTEND_PROFILE=1` so
normal builds have no noise.

This will separate:

- core frame cost,
- audio enqueue/resample cost,
- RGBA conversion + Slint image upload cost,
- timer/event-loop pacing effects.

### 2. Short-term mitigation: automatic frame skip

Implement an adaptive frame-skip path:

```text
if emulation/audio is behind:
  run multiple emulated frames
  push audio for every frame
  upload only the final framebuffer
```

This preserves audio production better than the current “one run = one draw”
path. The visual result may skip frames under load, but audio should crackle
less because sound generation is prioritized over drawing every frame.

Important constraint: because core-only release measured ~74 FPS on the tested
ROM, frame skip can only recover time spent outside the core frame. It will not
fix titles where the core itself cannot reach real time.

### 3. Medium-term fix: decouple emulation/audio from GUI rendering

Move emulation and audio production off the Slint GUI callback:

```text
emulation/audio thread
  -> run frames against real-time/audio-ring pacing
  -> push audio every emulated frame
  -> publish latest framebuffer snapshot

GUI thread
  -> draw latest available framebuffer at whatever rate the UI can sustain
```

This is the standard emulator architecture. It allows audio to remain stable
when GUI rendering is slower than the emulated display cadence.

The core API is already compatible with this direction, but the frontend will
need careful synchronization for:

- input state snapshots,
- ROM reset/load lifecycle,
- save-state operations,
- save RAM flushes,
- framebuffer ownership/copying.

### 4. Performance work remains useful

The headless release profile for this ROM is very close to the 75 FPS target, so
optimization still matters. The profiler shows CPU and APU as the largest core
buckets for this ROM:

```text
CPU 53.9%, APU 29.5%, PPU 16.2%
```

Future optimization should use `docs/dev/Profiling.md` and measure before/after
with the same ROM or a license-clean equivalent.

## Related but separate issue: perceived volume

The user initially thought audio was absent, then confirmed it was audible but
quiet compared with YouTube. That is a separate gain/mixing UX issue from the
dropouts.

Potential frontend/audio-layer follow-ups:

- add optional output gain above 100% with clipping protection,
- revisit `MIX_SCALE` / voice gain only if sample-level audio evidence supports
  changing deterministic core mix behavior,
- expose a clearer volume control or per-platform default.

Do not conflate volume with underrun. The dropout symptom is explained by the
frontend scheduling / FPS issue.

