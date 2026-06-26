//! Emulator core: CPU, memory, interrupts, timers, DMA, PPU, APU, cartridge.
//!
//! This crate must never depend on GUI, audio backend, or input libraries
//! (see docs/dev/Blueprint.md and docs/dev/DevelopmentPlan.md). It is
//! implemented and tested in subsequent phases of the development plan.

pub mod bus;
pub mod cpu;
