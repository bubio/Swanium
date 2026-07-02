---
name: hw-component
description: Workflow for implementing a new WonderSwan hardware subsystem (I/O port bus, interrupt controller, timers, DMA, PPU, APU, cartridge, RTC) in swanium-core. Use when starting work on a new hardware component or a new phase of docs/dev/DevelopmentPlan.md.
argument-hint: "[component-name]"
---

Follow this workflow to implement hardware component: $ARGUMENTS

## 1. Research before writing code
- Read the relevant phase section in `docs/dev/DevelopmentPlan.md`, including "サイクル精度設計の考慮点" and the RetroAchievements-compatibility constraints.
- Read `docs/dev/Blueprint.md` for how the component fits the architecture.
- Read `docs/dev/Status.md` for what already exists and what is deferred waiting on this component (e.g. IN/OUT waits on the I/O bus; INT/IRET waits on the interrupt controller).
- List the hardware registers/ports, their addresses, read/write behavior, and cycle timing before implementing.

## 2. Design constraints
- Implement inside `crates/core` only; keep it platform-independent (no GUI/audio/input deps).
- Follow the existing trait-based pattern: the CPU talks to memory via the `MemoryBus` trait — new buses/components should be similarly trait-based and independently testable.
- State must be plain data (serializable-friendly, no global state) for future save states and rcheevos.

## 3. Test-first implementation
- Write unit tests first, colocated like the existing ones (see `crates/core/src/cpu/tests/`), using flat/test-only implementations of the traits.
- Implement until tests pass. Cover register read/write, edge cases, and cycle counts where the plan specifies them.

## 4. Wire-up and verification
- Un-defer any CPU instructions that were waiting on this component (search `unimplemented!` in `crates/core/src/cpu`).
- Run the `/check` skill and fix all failures.
- Update `docs/dev/Status.md` to reflect the new component and any newly un-deferred instructions.
