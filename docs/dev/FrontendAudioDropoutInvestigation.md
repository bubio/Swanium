# Frontend audio dropout investigation

Date: 2026-07-13

## Summary

Linux audio output is available and `cpal` can open the stream, but the Slint GUI
path can run substantially below the WonderSwan frame cadence. The original
frontend advanced emulation, pushed audio, and uploaded the framebuffer from the
same GUI timer callback, so low GUI FPS directly starved the audio ring buffer
and produced audible dropouts.

This was not primarily an APU-silence issue. The user later confirmed audio was
present but quiet; the remaining observed problem at investigation time was
crackling / frequent dropout.

## Resolution (implemented 2026-07-14)

Emulation and audio production are now independent of the Slint event loop:

```text
swanium-emulation worker
  -> run System frames from real-time/audio-ring pacing
  -> enqueue audio for every emulated frame
  -> publish the latest RGB444 framebuffer snapshot

Slint GUI thread
  -> publish input/volume snapshots and FIFO commands
  -> copy and display only the latest completed framebuffer
```

`AudioStream::open()` now separates the platform `cpal::Stream` lifetime from a
movable `AudioProducer`. The stream remains on the GUI/platform thread, while
the worker owns `System`, the resampler, and the producer half of the shared
ring. ROM/reset/pause/save RAM/save-state/debug operations use a command channel;
input and volume use atomics; framebuffer locking is limited to copying a
preallocated snapshot. Native file dialogs publish neutral input but no longer
stop emulation or audio production.

Automated tests verify that the producer is `Send`, ring enqueue/clear behavior,
initial/latest framebuffer publication, and that the worker advances even when
the GUI never polls a frame. This removes GUI rendering cadence from the audio
production critical path. Runtime validation on the originally affected Linux
host remains useful to confirm the end-user symptom is gone.

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

## Original frontend scheduling shape

Before the resolution above, the frontend timer did one emulated frame and one
UI upload in the same callback:

```text
Slint timer tick
  -> system.run_frame(keys)
  -> audio.push(system.audio_samples())
  -> system.clear_audio_samples()
  -> update_window_frame(...)
```

Relevant implementation points at the time of investigation:

- `crates/frontend/src/main.rs`
  - `POLL_INTERVAL = 4 ms`
  - `audio_stream` is opened once with `audio::AudioStream::open()`
  - frame pacing checks `audio.ring_fill()`
  - after every `System::run_frame`, the frontend pushes the accumulated APU
    samples and then uploads a new Slint image
- `crates/audio/src/stream.rs`
  - `AudioStream` uses a fixed-size ring buffer.
  - cpal underruns are padded with silence.

This meant rendering throughput controlled audio production throughput. If the
GUI only reaches ~40 frames/s, the core only produces ~40 frames/s worth of audio.
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

### GUI runtime sampling

`perf` was not available in the agent environment (`perf_event_paranoid = 3`),
so the CI-like release GUI binary was sampled with `gdb` while running the same
ROM. This is coarse, but it is enough to distinguish “busy in core emulation”
from “blocked in the window/rendering stack”.

Default Slint backend samples showed the main thread mostly outside the emulator
core:

```text
Thread 1 "frontend":
  libc syscall
  radeonsi_dri.so
  libGLX_mesa.so
  <...OpenGLContext as ...OpenGLInterface>::swap_buffers
```

Other samples repeatedly showed synchronous X11/winit property queries:

```text
Thread 1 "frontend":
  poll
  libxcb.so
  xcb_wait_for_reply64
  x11rb::connection::RequestConnection::wait_for_reply_or_error
  winit::platform_impl::linux::x11::...get_motif_hints
  winit::platform_impl::linux::x11::window::UnownedWindow::set_decorations_inner
  i_slint_backend_winit::winitwindowadapter::WinitWindowAdapter::update_window_properties
```

One render sample showed the expected Slint/femtovg/OpenGL path:

```text
Thread 1 "frontend":
  radeonsi_dri.so
  femtovg::renderer::opengl::OpenGl::clear_rect
  femtovg::Canvas<T>::flush_to_output
  i_slint_renderer_femtovg::FemtoVGRenderer<B>::render
  i_slint_backend_winit::winitwindowadapter::WinitWindowAdapter::draw
```

CPU usage for a 20 second default-backend GUI run was also low:

```text
real 20.11
user 6.61
sys  0.56
```

That is roughly 36% CPU, so the process is not simply saturating a CPU core with
emulation work. The samples point at OpenGL/Mesa/X11 synchronization and window
property work as major contributors in this environment.

Running the same binary with Slint's software backend changed the sampled stack:

```sh
SLINT_BACKEND=winit-software ./target/release/frontend \
  '/home/bubio/ROMs/WonderSwan/Wizardry_-_Scenario_1_-_Kyounou_no_Shiren_Jou_(J)_[M][!].wsc'
```

Software-backend samples no longer showed Mesa/radeonsi `swap_buffers` as the
main-thread blocker. They instead landed in core and software image rendering:

```text
Thread 1 "frontend":
  swanium_core::system::System::drive_frame
  swanium_core::bus::Bus::render_scanline
  swanium_core::ppu::sample_sprite_pixel
```

and:

```text
Thread 1 "frontend":
  i_slint_renderer_software::RenderToBuffer<B>::foreach_ranges
  i_slint_renderer_software::SceneBuilder<T>::draw_image_impl
  i_slint_core::items::ImageItemVTable::render
```

CPU usage for a 20 second software-backend GUI run was similar:

```text
real 20.03
user 6.98
sys  0.40
```

This comparison strongly suggests the default Linux Slint backend path
(femtovg/OpenGL on Mesa/radeonsi plus X11 property synchronization) is a major
runtime factor on this PC. The software backend is not necessarily faster, but
it removes one class of OpenGL/Mesa stalls and is a useful diagnostic/compatibility
option.

### Core hot-path issue found during sampling

One software-backend sample landed in:

```text
std::env::_var_os
<swanium_core::bus::Bus as swanium_core::cpu::bus::MemoryBus>::write_u8
swanium_core::cpu::Cpu::execute
swanium_core::system::System::drive_frame
```

The source is the Clock Tower debug trace gate:

```rust
fn clock_tower_trace_enabled() -> bool {
    std::env::var_os("SWANIUM_CT_TRACE").is_some()
}
```

This check was called from hot write paths (`trace_clock_tower_io_write` /
`trace_clock_tower_wram_write`). Even when tracing was disabled, reading the
process environment from emulated memory/I/O writes added avoidable overhead.
As of 2026-07-14 the result is cached with `std::sync::OnceLock<bool>`, so the
environment is read only on the first trace check.

### Core optimization follow-up (2026-07-14)

The original Linux number above came from the in-core `profiling` feature. A
follow-up on macOS ARM64 showed that the profiler's per-instruction `Instant`
measurements materially perturb this workload: with the same local Wizardry ROM,
normal release Criterion measured about 1.0125 ms/frame before the changes while
the profiler-enabled example measured about 1.243 ms/frame. The synthetic ROM
was affected much more strongly. Consequently the profiler remains useful for
subsystem orientation, but its frame time must not be treated as normal release
throughput.

Four low-risk steady-state changes were then applied:

- cache the Clock Tower trace environment flag once;
- update APU output-register readback once at each `Apu::tick` boundary instead
  of every sound clock (sample generation and `tick(1)` SDMA behavior remain
  unchanged);
- process background scanlines by at-most-eight-pixel tile spans;
- rasterize sprites once per scanline into buffers that preserve OAM order and
  front/behind-SCR2 priority.

Normal release Criterion after all four changes measured 367.24–368.68 us/frame
on the same macOS machine and ROM, about 64% less time than the 1.0125 ms baseline
(about 2.75x throughput). This is a core-only result and does not remove the need
to fix Linux GUI/audio scheduling.

A continuation pass sampled the normal optimized Criterion executable rather
than the instrumented profiler. `Apu::update_output_ports` accounted for about
6% of samples because it rebuilt and stored all three CPU-visible digital mixer
words after every CPU instruction even though software rarely reads them. The
ports `0x96`–`0x9B` are now derived from current channel/noise/voice state only
when the CPU reads one. This does not change APU clocks or host sample generation,
and the ws-test-suite sound-quirks oracle still passes. Against the immediately
preceding Criterion baseline, the same Wizardry ROM improved from 354.87 us/frame
to 341.13 us/frame (Criterion change estimate -4.28%). Relative to the original
1.0125 ms/frame release baseline, the complete pass now uses about 66% less time
per frame (2.97x throughput).

## Interpretation

WonderSwan runs at about 75 frames/s. In this frontend design, one emulated frame
is also the unit that produces one batch of APU samples for the host audio ring.
At ~40 GUI FPS, only about 53% of the required audio batches are generated per
second. The ring buffer cannot compensate for sustained underproduction; it can
only absorb short jitter.

The follow-up macOS Criterion result shows that the normal core has substantial
headroom on that machine after the steady-state fixes. The original Linux
profiler result is not a reliable absolute release baseline because the timing
probes perturb the workload. Linux GUI cost from Slint image upload, scaling,
compositing, status updates, input polling, or window-system synchronization can
still push total throughput below the required cadence and must be measured
separately on the affected host.

The GUI sampling makes this more specific for the tested Linux PC:

- default backend: much of the sampled main-thread time is in Slint/femtovg,
  Mesa/radeonsi, GLX buffer swap, or synchronous X11 property queries;
- software backend: samples move to core PPU work and software image rendering;
- process CPU usage is low enough that a pure CPU bottleneck is unlikely for the
  default backend run;
- the clear core hot-path cleanup (`SWANIUM_CT_TRACE` environment-variable
  polling) has since been completed along with the follow-up optimizations above.

## Recommended fix order

### 1. Remove known core hot-path overhead (done 2026-07-14)

`SWANIUM_CT_TRACE` is now cached once, so normal memory/I/O writes no longer read
the environment repeatedly. The same core pass also optimized PPU tile/sprite
scanline work, then moved the CPU-visible APU mixer readback from eager per-tick
updates to on-demand reads as described above.

### 2. Add frontend timing instrumentation (optional follow-up)

Add a gated diagnostic mode that records, per rendered frame:

- worker `System::run_frame` duration
- `AudioProducer::push` duration
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

### 3. Compare Linux Slint backends explicitly

Test and document the FPS/audio behavior for:

```sh
SLINT_BACKEND=winit-femtovg ./target/release/frontend ...
SLINT_BACKEND=winit-software ./target/release/frontend ...
```

If `winit-software` improves stability on low-end Mesa/radeonsi systems, expose
it as a Linux troubleshooting option or consider a Linux-specific default only
after testing on other machines. If the default backend remains preferable on
modern GPUs, keep the backend selectable rather than hard-coding one path.

Also investigate why `update_window_properties` / `set_decorations_inner` shows
up repeatedly. If the frontend is pushing unchanged window properties every
tick, avoid doing so; window property changes should only occur when fullscreen,
scale, rotation, title, or related UI state actually changes.

### 4. Short-term mitigation: automatic frame skip (superseded)

Implement an adaptive frame-skip path:

```text
if emulation/audio is behind:
  run multiple emulated frames
  push audio for every frame
  upload only the final framebuffer
```

This would have preserved audio production better than the original “one run =
one draw” path. It was not needed after implementing the stronger thread
separation below: the GUI now naturally skips intermediate snapshots while the
worker still generates every frame's audio.

Important constraint: frame skip can only recover time spent outside skipped
draws. It will not fix titles or hosts where normal (non-profiled) core execution
itself cannot reach real time.

### 5. Decouple emulation/audio from GUI rendering (done 2026-07-14)

Emulation and audio production were moved off the Slint GUI callback:

```text
emulation/audio thread
  -> run frames against real-time/audio-ring pacing
  -> push audio every emulated frame
  -> publish latest framebuffer snapshot

GUI thread
  -> draw latest available framebuffer at whatever rate the UI can sustain
```

This architecture allows audio to remain stable
when GUI rendering is slower than the emulated display cadence.

The implementation provides explicit synchronization for:

- atomic input and volume snapshots;
- FIFO ROM reset/load/pause/debug commands;
- synchronous save-state and save-RAM requests at worker frame boundaries;
- a short-held mutex around the latest framebuffer copy;
- ordered pause/save/replace/resume during ROM changes, so the old machine
  cannot advance after its final save snapshot.

### 6. Continue measurement-driven performance work

The 2026-07-14 steady-state pass produced a large normal-release improvement.
An external sampling follow-up removed the remaining eager APU mixer-readback
hotspot. The enabled profiler previously identified CPU and APU as the largest
core buckets for this ROM:

```text
CPU 53.9%, APU 29.5%, PPU 16.2%
```

These percentages are observer-affected and should only guide the next external
profile. Future optimization should use `docs/dev/Profiling.md` and measure
before/after with normal release Criterion on the same ROM or a license-clean
equivalent. CPU memory-map restructuring and event-driven APU scheduling were
deliberately deferred because they carry more cycle-accuracy risk.

The 2026-07-14 checkpoint in `docs/dev/Profiling.md` records the final
remeasurement, remaining 1.3x-1.6x realistic headroom, and why another 2x is not
an expected outcome without those higher-risk architectural changes. Core
performance work is paused at that checkpoint while release preparation takes
priority.

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
