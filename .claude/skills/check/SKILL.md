---
name: check
description: Run the full local CI check for the Swanium workspace (fmt check, clippy with zero warnings, full test suite) and fix any failures. Use after completing any code change and before reporting work as done or committing.
allowed-tools: Bash(cargo:*), Read, Edit, Grep, Glob
---

Run the following steps in order. Do not skip a step, and do not proceed to the next step until the current one passes.

1. `cargo fmt --all -- --check`
   - If it fails, run `cargo fmt --all`, then re-run the check.
2. `cargo clippy --workspace --all-targets -- -D warnings`
   - Fix every warning at its root cause. Do not add `#[allow(...)]` to silence lints unless there is a justified `// SAFETY:`-style comment explaining why.
3. `cargo test --workspace`
   - If tests fail, read the failing test and the code under test, fix the code (or the test if the test itself is wrong), and re-run.

Finish with a short report: pass/fail per step, and for any fixes made, what was changed and why. If a step still fails after 3 fix attempts, stop and report the failure output verbatim instead of looping.
