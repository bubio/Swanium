//! Emulator core: CPU, memory, interrupts, timers, DMA, PPU, APU, cartridge.
//!
//! This crate must never depend on GUI, audio backend, or input libraries
//! (see docs/dev/Blueprint.md and docs/dev/DevelopmentPlan.md). It is
//! implemented and tested in subsequent phases of the development plan.

pub mod apu;
pub mod bus;
pub mod cpu;
pub mod keypad;
pub mod model;
pub mod ppu;
#[cfg(feature = "profiling")]
pub mod profile;
pub mod system;

pub use model::HardwareModel;
#[cfg(feature = "profiling")]
pub use profile::{FrameProfile, ProfileSnapshot};
