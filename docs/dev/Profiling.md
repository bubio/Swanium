# Profiling & benchmarking

How to find and track down performance bottlenecks in Swanium. There are three
complementary tools, from coarse to fine:

| Tool | Granularity | Use when |
| --- | --- | --- |
| In-core frame profiler (`profiling` feature) | Per subsystem (CPU / PPU / APU / DMA) | "Which subsystem dominates a frame?" |
| Criterion benches (`cargo bench`) | Per entry point, statistically | Track regressions; before/after a change |
| External sampling profiler (`samply`, `perf`) | Per function / line | "Which function inside that subsystem is hot?" |

All three run headless — no GUI, audio, or input — and default to a tiny
self-contained **synthetic ROM** (a 64 KiB image whose reset vector is an
infinite `JMP $`), so nothing needs a copyrighted ROM. Point them at a real
title with `SWANIUM_BENCH_ROM=/path/to/rom.ws` (or pass the path as the first
argument to the `profile` example) for a realistic CPU workload.

Always measure under the **release** profile (`--release` / `cargo bench`). The
workspace `[profile.release]`/`[profile.bench]` use `lto = "thin"` and
`codegen-units = 1`; debug builds are far slower and give misleading ratios.

## 1. In-core frame profiler (subsystem split)

Gated behind the `profiling` feature so a normal build has **zero** overhead and
stays fully deterministic (it reads wall-clock `Instant` only when the feature
is on, and never influences emulated state — see `crates/core/src/profile.rs`).

```sh
# Synthetic ROM:
cargo run -p swanium-core --features profiling --example profile --release
# Real ROM:
cargo run -p swanium-core --features profiling --example profile --release -- path/to/rom.ws
```

Example output:

```
600 frames, 0.161 ms/frame | CPU 18.8% PPU 29.6% APU 40.9% DMA 1.7% | 3052800 insns
  CPU    0.030 ms/frame (18.8%)
  PPU    0.048 ms/frame (29.6%)
  APU    0.066 ms/frame (40.9%)
  DMA    0.003 ms/frame ( 1.7%)
  → 6214 frames/s headroom (target 75)
```

To read the split programmatically, build with `--features profiling` and call
`System::profile_snapshot()` (returns the plain-data `ProfileSnapshot`) /
`System::reset_profile()`.

> Note: the synthetic ROM's CPU is a trivial spin loop, so its CPU share is much
> lower than a real game's. Use a real ROM via `SWANIUM_BENCH_ROM` for a
> representative CPU/PPU/APU balance.

## 2. Criterion benchmarks (regression tracking)

```sh
cargo bench -p swanium-core                       # all benches
cargo bench -p swanium-core --bench frame -- run_frame   # one bench by name
```

`crates/core/benches/frame.rs` provides:

- `run_frame` — a whole frame (the top-level number to watch),
- `render_scanline` — the PPU renderer over 144 visible lines,
- `tick_apu_frame` — the APU over one frame's sound-clock ticks.

Criterion writes HTML reports and compares against the previous run under
`target/criterion/`. Typical before/after workflow:

```sh
git stash            # or checkout the baseline
cargo bench -p swanium-core -- --save-baseline before
git stash pop        # apply the change
cargo bench -p swanium-core -- --baseline before
```

## 3. External sampling profiler (function/line hotspots)

Once the frame profiler says *which* subsystem is hot, a sampling profiler shows
*which function* inside it. These need line info in the optimized binary; add it
temporarily via an env override rather than committing it:

```sh
# One-off release build with debug info, without editing Cargo.toml:
CARGO_PROFILE_RELEASE_DEBUG=1 cargo build -p swanium-core \
    --features profiling --example profile --release
```

### macOS — samply (recommended)

```sh
cargo install samply
CARGO_PROFILE_RELEASE_DEBUG=1 cargo build -p swanium-core \
    --features profiling --example profile --release
samply record ./target/release/examples/profile path/to/rom.ws
```

`samply` opens an interactive Firefox-Profiler flame graph in the browser.
Alternatively, Xcode Instruments via `cargo instruments` (`cargo install
cargo-instruments`, then `cargo instruments -t time ...`).

### Linux — perf / flamegraph

```sh
cargo install flamegraph
CARGO_PROFILE_RELEASE_DEBUG=1 cargo flamegraph -p swanium-core \
    --features profiling --example profile -- path/to/rom.ws
# → flamegraph.svg
```

## Where the frame pipeline is

The hot path is `System::drive_frame` (`crates/core/src/system.rs`): for each of
159 scanlines it runs the CPU for a scanline's cycles, ticks the APU, runs GDMA,
and (for the 144 visible lines) renders via `Bus::render_scanline`. The frame
profiler's four buckets bracket exactly these calls.
