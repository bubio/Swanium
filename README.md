# Swanium

> **Work in progress — not yet functional**

A cycle-accurate WonderSwan / WonderSwan Color emulator written in Rust.

## Status

| Phase | Description | Status |
|-------|-------------|--------|
| 0 | Cargo workspace bootstrap | ✅ Complete |
| 1 | V30MZ CPU core (full instruction set) | ✅ Complete (80 tests) |
| 2 | Memory map, interrupts, timers, DMA | 🔲 Not started |
| 3 | CPU test ROM verification | 🔲 Not started |
| 4 | PPU (graphics) | 🔲 Not started |
| 5 | APU (audio) | 🔲 Not started |
| 6 | Cartridge / save RAM | 🔲 Not started |
| 7 | Frontend (Slint + wgpu + cpal) | 🔲 Not started |
| 8 | WonderSwan Color extensions | 🔲 Not started |

See [`docs/dev/DevelopmentPlan.md`](docs/dev/DevelopmentPlan.md) for the full roadmap.

## Building

Requires Rust stable (see `rust-toolchain.toml`).

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
```

## Architecture

```
Frontend (Slint) → App → { Audio (cpal), Input (gilrs) }
                               ↓
         Emulator Core: CPU · Memory · PPU · APU · DMA · Cartridge
```

The emulator core (`crates/core`) has no dependency on GUI, audio, or input libraries and can be used headlessly.

## License

MIT — see [LICENSE](LICENSE).
