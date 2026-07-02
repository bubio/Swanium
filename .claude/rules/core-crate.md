---
paths:
  - "crates/core/**"
---

# swanium-core constraints (crates/core)

- This crate must stay platform-independent and headless: never add dependencies on GUI, audio, input, or windowing libraries (Slint, cpal, gilrs, wgpu, rfd, winit, ...). Check `crates/core/Cargo.toml` after any dependency change.
- Cycle-accuracy is a first-class constraint. Before changing CPU/PPU/APU/timer/DMA synchronization code, read the "サイクル精度設計の考慮点" section of `docs/dev/DevelopmentPlan.md`.
- Keep the public API deterministic and FFI-friendly: no global state, plain data types, stable memory-read API, execution callable at frame boundaries (required for RetroAchievements/rcheevos).
- The package name is `swanium-core`, not `core`. Run single tests with `cargo test -p swanium-core <test_name>`.
- When CPU instruction coverage or deferred-feature status changes, update `docs/dev/Status.md`.
