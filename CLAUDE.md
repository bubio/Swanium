# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Phase 1 of `docs/dev/DevelopmentPlan.md` is in progress. `crates/core/src/cpu` implements the V30MZ register file, flags, ModRM decoding, and a broad instruction subset against a `MemoryBus` trait, using a test-only flat-memory implementation:

- Data movement: MOV, XCHG, PUSH/POP, LAHF/SAHF/PUSHF/POPF, XLAT, CBW/CWD.
- Arithmetic/logic: ADD/OR/ADC/SBB/AND/SUB/XOR/CMP/TEST and their immediate/group forms, INC/DEC, NOT/NEG, MUL/IMUL/DIV/IDIV (group F6/F7), shift/rotate group (D0-D3).
- Control flow: JMP/Jcc/CALL/RET, LOOP/LOOPE/LOOPNE/JCXZ, flag instructions, NOP/HLT.

Still unimplemented (panics via `unimplemented!` with a message): segment override prefixes, string instructions (MOVS/CMPS/SCAS/LODS/STOS) and REP, IN/OUT port I/O, INT/IRET (DIV/IDIV by zero or overflow also defer to this — real hardware raises INT0), far jumps/calls, AAA/AAS/AAM/AAD (BCD — flagged as a V30MZ-vs-8086 risk area pending hardware verification), and segment register MOV/PUSH/POP. Memory map, interrupt controller, timers, DMA, PPU, APU, and cartridge logic are not yet implemented — see `docs/dev/DevelopmentPlan.md` for the phase-by-phase roadmap.

The workspace has lint configuration in the root `Cargo.toml` (`[workspace.lints]`), inherited by each crate via `[lints]\nworkspace = true` — plain `cargo clippy` enforces `-D warnings`-equivalent behavior without needing the explicit flag.

Read `docs/dev/Blueprint.md` (vision/architecture) and `docs/dev/DevelopmentPlan.md` (phased roadmap, cycle-accuracy design notes, RetroAchievements-compatibility constraints, test strategy) before making non-trivial changes.

## Commands

```sh
cargo build --workspace
cargo test --workspace
cargo test -p swanium-core <test_name>   # run a single test in the core crate
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

CI (`.github/workflows/ci.yml`) runs `fmt --check`, `clippy -D warnings`, and `test` on Linux/macOS/Windows.

## What this project is

Swanium is a cross-platform WonderSwan / WonderSwan Color emulator written in Rust. It is intended both as an accurate (cycle-accurate) hardware emulator and as a learning project for modern Rust application architecture.

## Architecture

The emulator core (`crates/core`, package name `swanium-core`) has **no dependency on GUI, audio backend, or gamepad libraries** — it must remain usable headlessly. The frontend wires the core to Slint for UI, cpal for audio output (via `crates/audio`), and gilrs for input (via `crates/input`).

```
Slint GUI -> Frontend App -> { Audio (cpal), Input (gilrs) }
                                          |
                                          v
                Emulator Core: CPU (V30MZ), Memory, Interrupts, Timers, DMA, PPU, APU, Cartridge, RTC
```

**Note on the `core` crate's package name**: the crate lives at `crates/core` but its Cargo package name is `swanium-core` (not `core`), because naming a package `core` collides with the implicit sysroot `core` crate and causes ambiguous-name errors in any dependent crate that uses `core::...` paths from std.

## Cargo workspace layout

```
swanium/
├── Cargo.toml              # workspace root
├── rust-toolchain.toml     # stable channel, rustfmt + clippy components
├── crates/
│   ├── core/       # (package: swanium-core) CPU, memory map, interrupts, timers, DMA, PPU, APU, cartridge, save RAM — platform-independent
│   ├── frontend/   # Slint UI, menus, settings, ROM management, save states, debug windows (binary crate)
│   ├── audio/      # cpal backend, ring buffer, audio synchronization
│   ├── video/      # wgpu rendering, scaling, filters, future shader support
│   ├── input/      # keyboard + gilrs gamepad support
│   └── common/     # shared error types, configuration, logging
├── docs/
│   └── dev/        # Blueprint.md (vision), DevelopmentPlan.md (roadmap)
└── tests/
    ├── fixtures/    # CPU/PPU/cartridge test fixtures — see tests/README.md for the public-test-ROM policy
    └── README.md
```

Dependency/build order: `common` → `core` → (`video`, `audio`, `input` independent, parallelizable) → `frontend`.

## Development principles

- Prefer stable Rust over nightly.
- Keep the emulator core (`crates/core`) platform-independent — no GUI, audio backend, or input library dependencies leak into it. This is enforced by review, not currently by an automated CI check.
- Minimize `unsafe` code.
- Cycle-accuracy is a first-class design constraint, not an afterthought — see "サイクル精度設計の考慮点" in `docs/dev/DevelopmentPlan.md` before changing CPU/PPU/APU/timer/DMA synchronization code.
- Keep the core's public API deterministic and FFI-friendly (no global state, plain data types, a stable memory-read API, frame-boundary-callable execution) — this is required for the planned RetroAchievements (rcheevos) integration; see the dedicated section in `docs/dev/DevelopmentPlan.md`.
- Test each subsystem (CPU, memory, PPU, APU, DMA, cartridge, RTC) independently.
- Public test ROMs are never committed to this repository (licensing); see `tests/README.md`.
