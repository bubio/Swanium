# CLAUDE.md

@AGENTS.md

## Claude Code specific notes

- A PostToolUse hook auto-runs `rustfmt` on every edited `.rs` file — do not run `cargo fmt` manually after individual edits.
- Before finishing any code change, run the `/check` skill (fmt check, clippy, full test suite) and fix all failures.
- When implementing a new hardware subsystem (I/O ports, timers, DMA, PPU, APU, cartridge, RTC), use the `/hw-component` skill to follow the project workflow.
- After significant changes to `crates/core`, ask the `code-reviewer` subagent to review the diff.
