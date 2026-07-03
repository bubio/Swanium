//! Per-subsystem frame profiler (compiled only under the `profiling` feature).
//!
//! [`System`](crate::system::System) can be built with `--features profiling`
//! to accumulate wall-clock time spent in each part of the frame pipeline —
//! CPU, PPU, APU, and DMA — so the biggest cost centre can be identified before
//! optimising. When the feature is off, none of this code is compiled and the
//! emulator has zero profiling overhead.
//!
//! # Determinism
//!
//! This module reads [`std::time::Instant`] (wall-clock, non-deterministic).
//! It never influences emulated state — it only measures — and it is entirely
//! absent from a default build, so the core's deterministic, FFI-friendly
//! contract (required for RetroAchievements; see `docs/dev/DevelopmentPlan.md`
//! §7) is preserved.

use core::fmt;

/// Cumulative timing counters, summed across every frame since the last
/// [`reset`](FrameProfile::reset).
///
/// The `System` owns one of these and adds to it inside its frame driver. Read
/// a stable, plain-data view with [`snapshot`](FrameProfile::snapshot).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameProfile {
    /// Nanoseconds spent running the CPU (instruction fetch/execute + IRQ).
    pub cpu_ns: u64,
    /// Nanoseconds spent rendering PPU scanlines.
    pub ppu_ns: u64,
    /// Nanoseconds spent ticking the APU.
    pub apu_ns: u64,
    /// Nanoseconds spent in general-purpose DMA transfers.
    pub dma_ns: u64,
    /// Nanoseconds spent in a whole frame (wall-clock, including the small
    /// amount not attributed to the four buckets above).
    pub total_ns: u64,
    /// Number of frames measured.
    pub frames: u64,
    /// Number of CPU instructions executed (steps), across all frames.
    pub instructions: u64,
}

impl FrameProfile {
    /// Clear every counter back to zero.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Take an immutable, plain-data view enriched with derived percentages and
    /// per-frame averages. Safe to hand across an FFI boundary or log directly.
    pub fn snapshot(&self) -> ProfileSnapshot {
        let total = self.total_ns as f64;
        let pct = |ns: u64| {
            if total > 0.0 {
                (ns as f64 / total * 100.0) as f32
            } else {
                0.0
            }
        };
        let per_frame = |ns: u64| ns.checked_div(self.frames).unwrap_or(0);
        ProfileSnapshot {
            cpu_ns: self.cpu_ns,
            ppu_ns: self.ppu_ns,
            apu_ns: self.apu_ns,
            dma_ns: self.dma_ns,
            total_ns: self.total_ns,
            frames: self.frames,
            instructions: self.instructions,
            cpu_pct: pct(self.cpu_ns),
            ppu_pct: pct(self.ppu_ns),
            apu_pct: pct(self.apu_ns),
            dma_pct: pct(self.dma_ns),
            avg_frame_ns: per_frame(self.total_ns),
        }
    }
}

/// A frozen, plain-data view of a [`FrameProfile`] with derived shares and
/// averages. `Copy` and free of references so it is trivial to pass around,
/// log, or expose over FFI.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProfileSnapshot {
    /// Total CPU nanoseconds across all measured frames.
    pub cpu_ns: u64,
    /// Total PPU nanoseconds across all measured frames.
    pub ppu_ns: u64,
    /// Total APU nanoseconds across all measured frames.
    pub apu_ns: u64,
    /// Total DMA nanoseconds across all measured frames.
    pub dma_ns: u64,
    /// Total wall-clock nanoseconds across all measured frames.
    pub total_ns: u64,
    /// Number of frames measured.
    pub frames: u64,
    /// CPU instructions executed across all measured frames.
    pub instructions: u64,
    /// CPU share of total frame time, as a percentage (0–100).
    pub cpu_pct: f32,
    /// PPU share of total frame time, as a percentage (0–100).
    pub ppu_pct: f32,
    /// APU share of total frame time, as a percentage (0–100).
    pub apu_pct: f32,
    /// DMA share of total frame time, as a percentage (0–100).
    pub dma_pct: f32,
    /// Mean wall-clock nanoseconds per frame.
    pub avg_frame_ns: u64,
}

impl fmt::Display for ProfileSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} frames, {:.3} ms/frame | CPU {:.1}% PPU {:.1}% APU {:.1}% DMA {:.1}% | {} insns",
            self.frames,
            self.avg_frame_ns as f64 / 1.0e6,
            self.cpu_pct,
            self.ppu_pct,
            self.apu_pct,
            self.dma_pct,
            self.instructions,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_computes_shares_and_averages() {
        let p = FrameProfile {
            cpu_ns: 600,
            ppu_ns: 300,
            apu_ns: 50,
            dma_ns: 50,
            total_ns: 1000,
            frames: 2,
            instructions: 42,
        };
        let s = p.snapshot();
        assert_eq!(s.cpu_pct, 60.0);
        assert_eq!(s.ppu_pct, 30.0);
        assert_eq!(s.apu_pct, 5.0);
        assert_eq!(s.dma_pct, 5.0);
        assert_eq!(s.avg_frame_ns, 500);
        assert_eq!(s.instructions, 42);
    }

    #[test]
    fn snapshot_of_empty_profile_is_all_zero() {
        let s = FrameProfile::default().snapshot();
        assert_eq!(s.cpu_pct, 0.0);
        assert_eq!(s.avg_frame_ns, 0);
        assert_eq!(s.frames, 0);
    }

    #[test]
    fn reset_clears_counters() {
        let mut p = FrameProfile {
            cpu_ns: 10,
            frames: 1,
            ..Default::default()
        };
        p.reset();
        assert_eq!(p, FrameProfile::default());
    }
}
