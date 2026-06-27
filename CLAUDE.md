# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Phase 1 of `docs/dev/DevelopmentPlan.md` is substantially complete (80 unit tests). `crates/core/src/cpu` implements the V30MZ register file, flags, ModRM decoding, and a near-complete 8086-compatible instruction set against a `MemoryBus` trait, using a test-only flat-memory implementation:

- Data movement: MOV (all forms incl. segment registers and memory-direct 0xA0–0xA3), XCHG, PUSH/POP (incl. segment register forms), LAHF/SAHF/PUSHF/POPF, XLAT, CBW/CWD, LEA, LES, LDS.
- Arithmetic/logic: ADD/OR/ADC/SBB/AND/SUB/XOR/CMP/TEST and their immediate/group forms, INC/DEC, NOT/NEG, MUL/IMUL/DIV/IDIV (group F6/F7), shift/rotate group (D0-D3), BCD instructions DAA/DAS/AAA/AAS/AAM/AAD.
- Control flow: JMP (near/far)/Jcc/CALL (near/far)/RET (near/far), LOOP/LOOPE/LOOPNE/JCXZ, flag instructions, NOP/HLT, ENTER/LEAVE, indirect CALL/JMP/PUSH via Group FF.
- String instructions: MOVS/CMPS/SCAS/LODS/STOS (byte and word, 0xA4–0xAF) with REP/REPE/REPNE (0xF2/0xF3).
- 80186/V30 additions (the V30MZ is 80186-class): PUSHA/POPA (0x60/0x61), BOUND (0x62, INT 5 on range error), PUSH imm16/imm8 (0x68/0x6A), IMUL r16,r/m16,imm16/imm8 (0x69/0x6B), immediate-count shift/rotate (0xC0/0xC1), POP r/m16 (0x8F). Added when a commercial ROM (Lode Runner) exercised them; see `crates/core/src/cpu/tests/v30_extensions.rs`.
- Prefixes: segment override (0x26 ES:, 0x2E CS:, 0x36 SS:, 0x3E DS:) stored in `Cpu::seg_override`; REP stored in `Cpu::rep_prefix`.

Still deferred (panics via `unimplemented!`): IN/OUT port I/O (needs Phase 2 I/O bus), INT/IRET (needs interrupt controller — DIV/IDIV/AAM by zero also defer here), ENTER with nesting level > 0. Memory map, interrupt controller, timers, DMA, PPU, APU, and cartridge logic are not yet implemented — see `docs/dev/DevelopmentPlan.md` for the phase-by-phase roadmap.

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
