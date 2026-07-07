# Development docs map

Last updated: 2026-07-07.

Start here when checking project progress.

## Current status at a glance

| Area | Status | Where to look |
|---|---|---|
| Overall implementation inventory | Phases 1-8 complete/substantially complete; emulator milestones 9-12 complete | [`Status.md`](./Status.md) |
| Current emulator milestone | Milestone 13: timing precision phase | [`EmulationPlan.md`](./EmulationPlan.md) |
| Remaining emulator risks | Public ROM coverage, dot-level timing if proven needed, timing decomposition, hardware validation | [`EmulationBacklog.md`](./EmulationBacklog.md) |
| Compatibility evidence | Seed matrix exists; automated rows cover CPU, SDMA, PPU, WSC audio, RTC, mapper/save classes | [`CompatibilityMatrix.md`](./CompatibilityMatrix.md) |

## Document roles

- [`Blueprint.md`](./Blueprint.md): stable project vision and crate architecture.
- [`DevelopmentPlan.md`](./DevelopmentPlan.md): historical phased roadmap and design notes. It is not the quickest way to check current progress.
- [`Status.md`](./Status.md): source of truth for current implementation status. Update this when progress changes.
- [`EmulationPlan.md`](./EmulationPlan.md): execution plan for emulator-focused milestones. Completed milestones remain for context.
- [`EmulationBacklog.md`](./EmulationBacklog.md): remaining emulator risks and validation work, grouped by priority.
- [`CompatibilityMatrix.md`](./CompatibilityMatrix.md): license-clean evidence for public tests, synthetic tests, and manual smoke checks.
- [`AudioAccuracy.md`](./AudioAccuracy.md): audio-specific validation notes and manual comparison plan.
- [`V30MZ-Timing.md`](./V30MZ-Timing.md): CPU timing ledger for Milestone 13 work.
- [`Profiling.md`](./Profiling.md): performance measurements and profiling notes.

## Update rule

When implementation progress changes, update [`Status.md`](./Status.md) first.
Then update [`EmulationPlan.md`](./EmulationPlan.md), [`EmulationBacklog.md`](./EmulationBacklog.md),
or [`CompatibilityMatrix.md`](./CompatibilityMatrix.md) only if the milestone,
remaining-risk list, or evidence changed.
