//! Frame-accurate driver tying the [`Cpu`] to the [`Bus`].
//!
//! [`System`] owns the whole emulated machine and advances it one video frame
//! at a time via [`System::run_frame`]. A frame is driven scanline by scanline:
//! the CPU runs for a scanline's worth of cycles, the APU and DMA advance by the
//! same budget, then the PPU renders (or, during the vertical-blank period,
//! merely advances the line counter and fires the VBlank interrupt).
//!
//! The frame-boundary shape is deliberate: it satisfies the RetroAchievements
//! requirement that the core be callable one frame at a time and expose a stable
//! memory-read API ([`System::read_memory_at`]) — see
//! `docs/dev/DevelopmentPlan.md` §7.
//!
//! Timing is the sequential "run CPU, then advance peers by the same cycle
//! count" model from `docs/dev/DevelopmentPlan.md` §5; cycle-exact interleaving
//! is a later-phase refinement.

use crate::bus::Bus;
use crate::cpu::{Cpu, MemoryBus};
use crate::keypad::KeyState;

/// V30MZ master clock in Hz (also the sound clock).
pub const MASTER_CLOCK_HZ: u32 = 3_072_000;

/// Total scanlines per frame: 144 visible plus the vertical-blank period.
pub const SCANLINES_PER_FRAME: u16 = 159;

/// Number of visible (rendered) scanlines.
pub const VISIBLE_SCANLINES: u16 = 144;

/// Master-clock cycles per scanline (visible area plus horizontal blank).
pub const CYCLES_PER_SCANLINE: u32 = 256;

/// CPU cycles in one full frame ([`SCANLINES_PER_FRAME`] × [`CYCLES_PER_SCANLINE`]).
pub const CYCLES_PER_FRAME: u32 = SCANLINES_PER_FRAME as u32 * CYCLES_PER_SCANLINE;

/// The complete emulated WonderSwan: CPU plus the hardware [`Bus`].
pub struct System {
    cpu: Cpu,
    bus: Bus,
}

impl System {
    /// Build a system from a ROM image, parsing the cartridge header to allocate
    /// the save medium and select the mapper, and reset the CPU to the power-on
    /// vector (`CS:IP = 0xFFFF:0x0000`, physical 0xFFFF0).
    pub fn from_rom(rom: Vec<u8>) -> Self {
        Self::with_bus(Bus::from_rom(rom))
    }

    /// Build a system from a ROM image with no save medium.
    pub fn new(rom: Vec<u8>) -> Self {
        Self::with_bus(Bus::new(rom))
    }

    /// Build a system from a ROM image and an explicit SRAM buffer.
    pub fn with_sram(rom: Vec<u8>, sram: Vec<u8>) -> Self {
        Self::with_bus(Bus::with_sram(rom, sram))
    }

    fn with_bus(bus: Bus) -> Self {
        let mut cpu = Cpu::new();
        // V30MZ power-on reset vector: the OR-with-modulo address decode maps
        // 0xFFFF0 onto the last 16 ROM bytes (the footer / boot entry).
        cpu.reset(0xFFFF, 0x0000);
        Self { cpu, bus }
    }

    /// Run the machine for one full video frame.
    ///
    /// `keys` is the host input for this frame. Audio samples accumulate in the
    /// APU across frames; the caller drains them with [`System::audio_samples`]
    /// and [`System::clear_audio_samples`].
    pub fn run_frame(&mut self, keys: KeyState) {
        self.bus.set_keys(keys);
        for line in 0..SCANLINES_PER_FRAME {
            self.run_cpu_cycles(CYCLES_PER_SCANLINE);
            self.bus.tick_apu(CYCLES_PER_SCANLINE);
            self.bus.tick_gdma();

            if line < VISIBLE_SCANLINES {
                // Renders the line and advances the line-compare / HBlank hooks.
                self.bus.render_scanline(line as u8);
            } else {
                // Vertical-blank period: keep the line counter (and its compare
                // interrupt) live without rendering.
                self.bus.set_current_scanline(line as u8);
            }

            if line == VISIBLE_SCANLINES {
                self.bus.on_vblank();
            }
        }
    }

    /// Run the CPU for `budget` cycles, servicing enabled maskable interrupts
    /// between instructions.
    fn run_cpu_cycles(&mut self, budget: u32) {
        let mut spent = 0;
        while spent < budget {
            if self.cpu.flags.interrupt {
                if let Some(vector) = self.bus.pending_irq() {
                    self.cpu.handle_irq(&mut self.bus, vector);
                }
            }
            spent += self.cpu.step(&mut self.bus);
        }
    }

    // ── Output accessors ──────────────────────────────────────────────────

    /// The current framebuffer: 224×144 monochrome shade indices, row-major.
    pub fn framebuffer(&self) -> &[u8] {
        self.bus.framebuffer()
    }

    /// Interleaved stereo audio samples accumulated since the last clear.
    pub fn audio_samples(&self) -> &[i16] {
        self.bus.audio_samples()
    }

    /// Drop buffered audio samples (call after the frontend consumes them).
    pub fn clear_audio_samples(&mut self) {
        self.bus.clear_audio_samples();
    }

    /// The cartridge's persistent save bytes (for the frontend to write to disk).
    pub fn save_data(&self) -> &[u8] {
        self.bus.save_data()
    }

    /// Restore the cartridge's save medium from previously serialised bytes.
    pub fn load_save_data(&mut self, data: &[u8]) {
        self.bus.load_save_data(data);
    }

    /// Read one byte of the 20-bit physical address space.
    ///
    /// Stable, side-effect-free* memory-inspection API for tooling and the
    /// planned RetroAchievements integration (§7 of the development plan).
    /// (*Reads of WRAM/ROM/SRAM have no side effects; this does not touch the
    /// side-effecting I/O ports.)
    pub fn read_memory_at(&self, address: u32) -> u8 {
        self.bus.read_u8(address)
    }

    /// Shared access to the hardware bus.
    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    /// Mutable access to the hardware bus (for setup and tooling).
    pub fn bus_mut(&mut self) -> &mut Bus {
        &mut self.bus
    }

    /// Shared access to the CPU (for tooling and tests).
    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 64 KiB ROM whose reset entry (last 16 bytes) starts with `HLT` so the CPU
    /// stops immediately; the frame loop must still run without panicking.
    fn halting_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x10000];
        let len = rom.len();
        rom[len - 16] = 0xF4; // HLT at physical 0xFFFF0
        rom
    }

    #[test]
    fn cycles_per_frame_matches_components() {
        assert_eq!(CYCLES_PER_FRAME, 159 * 256);
    }

    #[test]
    fn frame_rate_is_about_75hz() {
        let fps = MASTER_CLOCK_HZ / CYCLES_PER_FRAME;
        assert_eq!(fps, 75);
    }

    #[test]
    fn framebuffer_has_full_screen_size() {
        let system = System::new(halting_rom());
        assert_eq!(system.framebuffer().len(), 224 * 144);
    }

    #[test]
    fn run_frame_halted_cpu_does_not_panic() {
        let mut system = System::new(halting_rom());
        system.run_frame(KeyState::NONE);
        // Reaching here without panicking is the assertion.
        assert_eq!(system.framebuffer().len(), 224 * 144);
    }

    #[test]
    fn reset_vector_reads_hlt_opcode() {
        let system = System::new(halting_rom());
        assert_eq!(system.read_memory_at(0xFFFF0), 0xF4);
    }

    #[test]
    fn cpu_halts_on_reset_vector_hlt() {
        let mut system = System::new(halting_rom());
        system.run_frame(KeyState::NONE);
        assert!(system.cpu().halted);
    }

    #[test]
    fn read_memory_at_reflects_wram_writes() {
        let mut system = System::new(halting_rom());
        system.bus_mut().write_u8(0x0100, 0x5A);
        assert_eq!(system.read_memory_at(0x0100), 0x5A);
    }
}
