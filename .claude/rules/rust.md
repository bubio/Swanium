---
paths:
  - "**/*.rs"
---

# Rust conventions for this workspace

- Stable Rust only (see `rust-toolchain.toml`); no nightly features.
- Workspace lints already deny warnings: plain `cargo clippy --workspace --all-targets` must pass with zero warnings. Never silence a lint with `#[allow(...)]` without a comment justifying it.
- Minimize `unsafe`; any new `unsafe` block needs a `// SAFETY:` comment.
- Follow the existing test layout: CPU tests live in `crates/core/src/cpu/tests/` as `#[cfg(test)]` modules; each subsystem is tested independently.
- Never commit ROM files or copyrighted test ROMs (see `tests/README.md`).
