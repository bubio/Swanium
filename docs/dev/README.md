# Development docs map

Last updated: 2026-07-10.

Start here when checking project progress.

## Current status at a glance

| Area | Status | Where to look |
|---|---|---|
| Overall implementation inventory | Phases 1-8 complete/substantially complete; emulator milestones 9-12 complete | [`Status.md`](./Status.md) |
| Current emulator milestone | Milestone 13: timing precision phase | [`EmulationPlan.md`](./EmulationPlan.md) |
| Remaining emulator work | Public ROM oracle expansion first; dot-level/timing/audio validation only when evidence requires it | [`RemainingWork.md`](./RemainingWork.md) |
| Compatibility evidence | Seed matrix exists; automated rows cover CPU, SDMA, PPU, WSC audio, RTC, mapper/save classes | [`CompatibilityMatrix.md`](./CompatibilityMatrix.md) |

## Document roles

- [`Blueprint.md`](./Blueprint.md): stable project vision and crate architecture.
- [`DevelopmentPlan.md`](./DevelopmentPlan.md): historical phased roadmap and design notes. It is not the quickest way to check current progress.
- [`Status.md`](./Status.md): source of truth for current implementation status. Update this when progress changes.
- [`EmulationPlan.md`](./EmulationPlan.md): execution plan for emulator-focused milestones. Completed milestones remain for context.
- [`RemainingWork.md`](./RemainingWork.md): source of truth for open emulator work after Milestone 13.
- [`CompatibilityMatrix.md`](./CompatibilityMatrix.md): license-clean evidence for public tests, synthetic tests, and manual smoke checks.
- [`AudioAccuracy.md`](./AudioAccuracy.md): audio-specific validation notes and manual comparison plan.
- [`V30MZ-Timing.md`](./V30MZ-Timing.md): CPU timing ledger for Milestone 13 work.
- [`Profiling.md`](./Profiling.md): performance measurements and profiling notes.
- [`archive/`](./archive/): superseded planning documents kept for historical context.

## Update rule

When implementation progress changes, update [`Status.md`](./Status.md) first.
Then update [`EmulationPlan.md`](./EmulationPlan.md), [`RemainingWork.md`](./RemainingWork.md),
or [`CompatibilityMatrix.md`](./CompatibilityMatrix.md) only if the milestone,
remaining-risk list, or evidence changed.
