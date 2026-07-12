//! Frame-accurate driver tying the [`Cpu`] to the [`Bus`].
//!
//! [`System`] owns the whole emulated machine and advances it one video frame
//! at a time via [`System::run_frame`]. A frame is driven scanline by scanline:
//! the CPU runs until the next scanline boundary while the APU advances after
//! each CPU instruction, then the PPU renders (or, during the vertical-blank
//! period, merely advances the line counter and fires the VBlank interrupt).
//! GDMA is not driven here — it executes synchronously when a game arms it via
//! port 0x48.
//!
//! The frame-boundary shape is deliberate: it satisfies the RetroAchievements
//! requirement that the core be callable one frame at a time and expose a stable
//! memory-read API ([`System::read_memory_at`]) — see
//! `docs/dev/DevelopmentPlan.md` §7.
//!
//! Timing is still scanline-framed for PPU events, but APU ticking is
//! instruction-interleaved so mid-scanline PCM writes affect the generated
//! samples at the right point in the sound timeline.

use crate::bus::Bus;
use crate::cpu::{Cpu, MemoryBus};
use crate::keypad::KeyState;
use crate::model::HardwareModel;
#[cfg(feature = "profiling")]
use crate::profile::{FrameProfile, ProfileSnapshot};

const STATE_MAGIC: &[u8; 8] = b"SWANST01";
const STATE_VERSION: u32 = 1;
const STATE_HEADER_LEN: usize = STATE_MAGIC.len() + std::mem::size_of::<u32>();

/// Time the given block, adding its wall-clock nanoseconds into `$slot`.
///
/// Under the `profiling` feature this brackets the block with
/// [`std::time::Instant`] reads; otherwise it expands to just the block, so a
/// default build carries no profiling overhead.
macro_rules! time_into {
    ($self:ident, $field:ident, $body:block) => {{
        #[cfg(feature = "profiling")]
        let __start = std::time::Instant::now();
        let __ret = $body;
        #[cfg(feature = "profiling")]
        {
            $self.profile.$field += __start.elapsed().as_nanos() as u64;
        }
        __ret
    }};
}

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

/// Display-register state captured at one visible scanline by
/// [`System::run_frame_traced`], for diagnosing per-scanline (raster) effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanlineTrace {
    /// Visible scanline (0–143).
    pub line: u8,
    /// Display-control register (port 0x00).
    pub disp_ctrl: u8,
    /// SCR1 vertical scroll (port 0x11).
    pub scr1_scroll_y: u8,
    /// SCR2 vertical scroll (port 0x13).
    pub scr2_scroll_y: u8,
    /// LCD line-compare register (port 0x03).
    pub line_compare: u8,
}

/// The complete emulated WonderSwan: CPU plus the hardware [`Bus`].
#[derive(serde::Serialize, serde::Deserialize)]
pub struct System {
    cpu: Cpu,
    bus: Bus,
    /// CPU cycles already spent into the next scanline because the previous
    /// instruction crossed a line boundary.
    cycle_carry: u32,
    /// Cumulative per-subsystem frame timings (present only with the
    /// `profiling` feature). See [`crate::profile`].
    #[cfg(feature = "profiling")]
    #[serde(skip, default)]
    profile: FrameProfile,
}

/// Errors raised while serialising or restoring a whole-machine save state.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// The byte stream does not start with Swanium's save-state header.
    #[error("invalid save-state magic")]
    InvalidMagic,
    /// The save state was written by an incompatible format version.
    #[error("unsupported save-state version {0}")]
    UnsupportedVersion(u32),
    /// The byte stream ended before the fixed header was complete.
    #[error("truncated save-state header")]
    TruncatedHeader,
    /// bincode failed while encoding the state.
    #[error("save-state encode error: {0}")]
    Encode(#[from] bincode::error::EncodeError),
    /// bincode failed while decoding the state.
    #[error("save-state decode error: {0}")]
    Decode(#[from] bincode::error::DecodeError),
}

impl System {
    /// Build a system from a ROM image, parsing the cartridge header to allocate
    /// the save medium and select the mapper, and reset the CPU to the power-on
    /// vector (`CS:IP = 0xFFFF:0x0000`, physical 0xFFFF0).
    pub fn from_rom(rom: Vec<u8>) -> Self {
        Self::with_bus(Bus::from_rom(rom))
    }

    /// Build a system from a ROM image and an internal boot ROM image.
    pub fn from_rom_with_boot_rom(rom: Vec<u8>, boot_rom: Vec<u8>) -> Self {
        let mut system = Self::from_rom(rom);
        system.install_boot_rom(boot_rom);
        system
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
        Self {
            cpu,
            bus,
            cycle_carry: 0,
            #[cfg(feature = "profiling")]
            profile: FrameProfile::default(),
        }
    }

    /// Run the machine for one full video frame.
    ///
    /// `keys` is the host input for this frame. Audio samples accumulate in the
    /// APU across frames; the caller drains them with [`System::audio_samples`]
    /// and [`System::clear_audio_samples`].
    pub fn run_frame(&mut self, keys: KeyState) {
        self.drive_frame(keys, None);
    }

    /// Run one frame while recording the display registers seen at each visible
    /// scanline. For PPU debugging (e.g. diagnosing raster split effects); the
    /// returned vector has one entry per visible line.
    pub fn run_frame_traced(&mut self, keys: KeyState) -> Vec<ScanlineTrace> {
        let mut trace = Vec::with_capacity(VISIBLE_SCANLINES as usize);
        self.drive_frame(keys, Some(&mut trace));
        trace
    }

    fn drive_frame(&mut self, keys: KeyState, mut trace: Option<&mut Vec<ScanlineTrace>>) {
        #[cfg(feature = "profiling")]
        let frame_start = std::time::Instant::now();

        self.bus.set_keys(keys);
        // Advance the cartridge RTC (if any) off the emulated clock, one frame's
        // worth of master-clock cycles — keeps timekeeping deterministic.
        self.bus.tick_rtc(CYCLES_PER_FRAME);
        self.bus.latch_sprites();
        for line in 0..SCANLINES_PER_FRAME {
            let budget = CYCLES_PER_SCANLINE.saturating_sub(self.cycle_carry);
            // Keep CPU and APU buckets exclusive: APU ticking happens inside
            // `run_cpu_cycles`, so measure the scanline span once and subtract
            // the APU time already recorded by the nested `tick_apu` calls.
            #[cfg(feature = "profiling")]
            let cpu_start = std::time::Instant::now();
            #[cfg(feature = "profiling")]
            let apu_before = self.profile.apu_ns;
            let spent = self.run_cpu_cycles(budget);
            #[cfg(feature = "profiling")]
            {
                let elapsed = cpu_start.elapsed().as_nanos() as u64;
                let apu_delta = self.profile.apu_ns.saturating_sub(apu_before);
                self.profile.cpu_ns += elapsed.saturating_sub(apu_delta);
            }
            self.cycle_carry = (self.cycle_carry + spent).saturating_sub(CYCLES_PER_SCANLINE);
            // GDMA is not ticked here: it runs synchronously the instant a game
            // writes the enable bit to port 0x48 (see `Bus::write_io`), matching
            // the hardware's CPU-stalling burst. Its cost is therefore folded
            // into `cpu_ns` (it executes inside `run_cpu_cycles`), so the
            // profiler's `dma_ns` bucket stays zero.

            if line < VISIBLE_SCANLINES {
                if let Some(trace) = trace.as_deref_mut() {
                    trace.push(ScanlineTrace {
                        line: line as u8,
                        disp_ctrl: self.bus.peek_io(0x00),
                        scr1_scroll_y: self.bus.peek_io(0x11),
                        scr2_scroll_y: self.bus.peek_io(0x13),
                        line_compare: self.bus.peek_io(0x03),
                    });
                }
                // Renders the line and advances the line-compare / HBlank hooks.
                time_into!(self, ppu_ns, {
                    self.bus.render_scanline(line as u8);
                });
            } else {
                // Vertical-blank period: keep the line counter (and its compare
                // interrupt) live without rendering. The HBlank timer still
                // ticks for these scanlines; games use all 159 line periods as
                // an audio-rate timebase for streamed PCM.
                self.bus.set_current_scanline(line as u8);
                self.bus.on_hblank();
            }
            self.service_pending_rep_irq_after_scanline_event();

            if line == VISIBLE_SCANLINES {
                self.bus.on_vblank();
                self.service_pending_rep_irq_after_scanline_event();
            }
        }

        #[cfg(feature = "profiling")]
        {
            self.profile.total_ns += frame_start.elapsed().as_nanos() as u64;
            self.profile.frames += 1;
        }
    }

    /// Run the CPU for `budget` cycles, servicing enabled maskable interrupts
    /// between instructions.
    fn run_cpu_cycles(&mut self, budget: u32) -> u32 {
        let mut spent = 0;
        while spent < budget {
            let maskable_inhibited = self.cpu.interrupt_inhibit > 0;
            if maskable_inhibited {
                self.cpu.interrupt_inhibit -= 1;
            }
            if let Some(vector) = self.bus.pending_irq() {
                if self.cpu.flags.interrupt && !maskable_inhibited {
                    let cycles = if let Some(ip) = self.cpu.take_interrupt_return_override_ip() {
                        self.cpu.handle_irq_at_ip(&mut self.bus, vector, ip)
                    } else {
                        self.cpu.handle_irq(&mut self.bus, vector)
                    };
                    spent += cycles;
                    time_into!(self, apu_ns, {
                        self.bus.tick_apu(cycles);
                    });
                } else if self.cpu.halted
                    && self.bus.peek_io(0xB4) & (1 << crate::bus::IrqSource::VBlank as u8) != 0
                {
                    self.cpu.halted = false;
                }
            }
            let cycles = self.cpu.step(&mut self.bus);
            spent += cycles;
            time_into!(self, apu_ns, {
                self.bus.tick_apu(cycles);
            });
            let trap_inhibited = self.cpu.trap_inhibit > 0;
            if trap_inhibited {
                self.cpu.trap_inhibit -= 1;
            } else if self.cpu.flags.trap {
                let cycles = self.cpu.handle_irq(&mut self.bus, 1);
                spent += cycles;
                time_into!(self, apu_ns, {
                    self.bus.tick_apu(cycles);
                });
            }
            #[cfg(feature = "profiling")]
            {
                self.profile.instructions += 1;
            }
        }
        spent
    }

    fn service_pending_rep_irq_after_scanline_event(&mut self) {
        if !self.cpu.flags.interrupt {
            return;
        }
        let Some(ip) = self.cpu.interrupt_return_override_ip() else {
            return;
        };
        let Some(vector) = self.bus.pending_irq() else {
            return;
        };
        self.cpu.take_interrupt_return_override_ip();
        let cycles = time_into!(self, cpu_ns, {
            self.cpu.handle_irq_at_ip(&mut self.bus, vector, ip)
        });
        self.cycle_carry = self.cycle_carry.saturating_add(cycles);
        time_into!(self, apu_ns, {
            self.bus.tick_apu(cycles);
        });
    }

    // ── Output accessors ──────────────────────────────────────────────────

    /// The current framebuffer: 224×144 [`Rgb444`](crate::ppu::Rgb444) colors,
    /// row-major.
    pub fn framebuffer(&self) -> &[u16] {
        self.bus.framebuffer()
    }

    /// The emulated hardware model.
    pub fn model(&self) -> HardwareModel {
        self.bus.model()
    }

    /// Override the emulated hardware model (Mono / Color / Crystal).
    pub fn set_model(&mut self, model: HardwareModel) {
        self.bus.set_model(model);
    }

    /// Install an internal boot ROM image, used for BIOS-driven startup.
    pub fn install_boot_rom(&mut self, boot_rom: Vec<u8>) {
        self.bus.install_boot_rom(boot_rom);
    }

    /// Interleaved stereo audio samples accumulated since the last clear.
    pub fn audio_samples(&self) -> &[i16] {
        self.bus.audio_samples()
    }

    /// Drop buffered audio samples (call after the frontend consumes them).
    pub fn clear_audio_samples(&mut self) {
        self.bus.clear_audio_samples();
    }

    /// Inject an absolute date/time into the cartridge RTC (no-op without one).
    ///
    /// The frontend calls this once from the host clock at ROM load; the core
    /// never reads wall-clock time itself, keeping execution deterministic for
    /// RetroAchievements. Components are decimal (not BCD): `year` is the
    /// two-digit calendar year within 2000–2099, `weekday` is 0–6 (0 = Sunday).
    #[allow(clippy::too_many_arguments)]
    pub fn set_rtc_datetime(
        &mut self,
        year: u8,
        month: u8,
        day: u8,
        weekday: u8,
        hour: u8,
        minute: u8,
        second: u8,
    ) {
        self.bus
            .set_rtc_datetime(year, month, day, weekday, hour, minute, second);
    }

    /// The cartridge's persistent save bytes (for the frontend to write to disk).
    pub fn save_data(&self) -> &[u8] {
        self.bus.save_data()
    }

    /// Restore the cartridge's save medium from previously serialised bytes.
    pub fn load_save_data(&mut self, data: &[u8]) {
        self.bus.load_save_data(data);
    }

    /// Serialise the complete emulated machine for a frontend/tooling save state.
    ///
    /// This is separate from cartridge SRAM/EEPROM persistence: it captures CPU,
    /// bus, PPU, APU, timers, RAM, ROM, and mapper state so execution can resume
    /// at the same emulated instant.
    pub fn save_state_bytes(&self) -> Result<Vec<u8>, StateError> {
        let payload = bincode::serde::encode_to_vec(self, bincode::config::standard())?;
        let mut out = Vec::with_capacity(STATE_HEADER_LEN + payload.len());
        out.extend_from_slice(STATE_MAGIC);
        out.extend_from_slice(&STATE_VERSION.to_le_bytes());
        out.extend_from_slice(&payload);
        Ok(out)
    }

    /// Restore a complete emulated machine from [`save_state_bytes`](Self::save_state_bytes).
    pub fn load_state_bytes(&mut self, data: &[u8]) -> Result<(), StateError> {
        if data.len() < STATE_HEADER_LEN {
            return Err(StateError::TruncatedHeader);
        }
        if &data[..STATE_MAGIC.len()] != STATE_MAGIC {
            return Err(StateError::InvalidMagic);
        }
        let version_start = STATE_MAGIC.len();
        let version = u32::from_le_bytes(
            data[version_start..STATE_HEADER_LEN]
                .try_into()
                .expect("fixed-size save-state version"),
        );
        if version != STATE_VERSION {
            return Err(StateError::UnsupportedVersion(version));
        }
        let (restored, _) = bincode::serde::decode_from_slice(
            &data[STATE_HEADER_LEN..],
            bincode::config::standard(),
        )?;
        *self = restored;
        Ok(())
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

    // ── Profiling (feature = "profiling") ─────────────────────────────────

    /// A plain-data snapshot of the cumulative per-subsystem frame timings
    /// gathered since the last [`reset_profile`](System::reset_profile).
    ///
    /// Only available with the `profiling` feature; see [`crate::profile`].
    #[cfg(feature = "profiling")]
    pub fn profile_snapshot(&self) -> ProfileSnapshot {
        self.profile.snapshot()
    }

    /// Clear the accumulated profiling counters back to zero.
    #[cfg(feature = "profiling")]
    pub fn reset_profile(&mut self) {
        self.profile.reset();
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
    fn save_state_restores_complete_system_state() {
        let mut system = System::new(halting_rom());
        system.run_frame(KeyState::A);
        let saved = system.save_state_bytes().expect("save state");
        let saved_fb = system.framebuffer().to_vec();
        let saved_cpu = *system.cpu();

        system.run_frame(KeyState::B);
        system.load_state_bytes(&saved).expect("load state");

        assert_eq!(system.framebuffer(), saved_fb);
        assert_eq!(*system.cpu(), saved_cpu);
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

    #[test]
    fn boot_rom_overrides_reset_vector_region() {
        let mut boot_rom = vec![0u8; 0x10000];
        boot_rom[0xFFF0] = 0xA5;
        let system = System::from_rom_with_boot_rom(halting_rom(), boot_rom);
        assert_eq!(system.read_memory_at(0xFFFF0), 0xA5);
    }

    #[test]
    fn a0_boot_disable_exposes_cartridge_reset_vector() {
        let mut boot_rom = vec![0u8; 0x10000];
        boot_rom[0xFFF0] = 0xA5;
        let mut system = System::from_rom_with_boot_rom(halting_rom(), boot_rom);
        assert_eq!(system.read_memory_at(0xFFFF0), 0xA5);

        system.bus_mut().write_io(0xA0, 0x88);
        assert_eq!(system.read_memory_at(0xFFFF0), 0xF4);
    }

    #[test]
    fn newoswan_stub_boot_rom_is_aligned_to_reset_vector() {
        let mut boot_rom = vec![0u8; 0x1FFC];
        boot_rom[0x1FF0] = 0xEA;
        boot_rom[0x1FF1] = 0x1B;
        let system = System::from_rom_with_boot_rom(halting_rom(), boot_rom);
        assert_eq!(system.read_memory_at(0xFFFF0), 0xEA);
        assert_eq!(system.read_memory_at(0xFFFF1), 0x1B);
        assert_eq!(system.read_memory_at(0xFFFFF), 0xFF);
    }

    #[test]
    fn run_frame_halts_from_boot_rom_reset_vector() {
        let mut boot_rom = vec![0u8; 0x10000];
        boot_rom[0xFFF0] = 0xF4;
        let mut system = System::from_rom_with_boot_rom(vec![0u8; 0x10000], boot_rom);
        system.run_frame(KeyState::NONE);
        assert!(system.cpu().halted);
    }
}
