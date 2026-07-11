//! WonderSwan hardware bus: memory map, I/O ports, interrupt controller,
//! HBlank/VBlank timers, and GDMA/SDMA.
//!
//! See `docs/dev/DevelopmentPlan.md` Phase 2 for the full design rationale.

mod cart;
#[cfg(test)]
mod tests;

pub use cart::{Cartridge, CartridgeHeader, Eeprom, Mapper, Rtc, SaveType};

use crate::apu::Apu;
use crate::cpu::MemoryBus;
use crate::keypad::KeyState;
use crate::model::HardwareModel;
use crate::ppu::{ColorPaletteResolver, MonoPaletteResolver, Ppu, Rgb444};

/// Open-bus return value for unmapped reads on WonderSwan mono.
const OPEN_BUS: u8 = 0x90;
const ADDRESS_MASK: u32 = 0xF_FFFF;
const ADDRESS_SPACE_SIZE: u32 = ADDRESS_MASK + 1;
const BOOT_ROM_ALIGNMENT: usize = 0x1000;

/// Sound-control (port 0x90) bit 5: channel 2 acts as the 8-bit PCM voice.
const SND_CTRL_VOICE: u8 = 0x20;
const SDMA_ENABLE: u8 = 0x80;
const SDMA_DECREMENT: u8 = 0x40;
const SDMA_REPEAT: u8 = 0x08;
const SDMA_HOLD: u8 = 0x04;
const SDMA_CYCLES_PER_SAMPLE: u16 = 128;
const SYSTEM_CTRL1_ROM_WAIT: u8 = 0x08;
const SERIAL_ENABLE: u8 = 0x80;
const SERIAL_TX_READY_IRQ: u8 = 1 << IrqSource::SerialReceive as u8;

/// Hardware interrupt request sources (bit positions in INT_CAUSE / INT_ENABLE).
///
/// Priority: higher bit number = higher priority (bit 7 checked first).
/// Unverified against real hardware; see "リスクと不確実性への対処方針" in
/// `docs/dev/DevelopmentPlan.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IrqSource {
    SerialReceive = 0,
    KeyPress = 1,
    Cartridge = 2,
    GdmaComplete = 3,
    ScanlineMatch = 4,
    VBlankTimer = 5,
    VBlank = 6,
    HBlankTimer = 7,
}

/// WonderSwan hardware bus.
///
/// Owns all hardware state except the CPU: WRAM, cartridge (ROM + SRAM),
/// I/O port registers, timer counters, and DMA transfer state.
///
/// The [`MemoryBus`] implementation dispatches the 20-bit physical address
/// space according to the WonderSwan memory map:
///
/// | Physical range      | Hardware                              |
/// |---------------------|---------------------------------------|
/// | 0x00000–0x03FFF     | Internal WRAM (16 KiB, mono)          |
/// | 0x04000–0x0FFFF     | Open bus (mono); WRAM on Color        |
/// | 0x10000–0x1FFFF     | Cartridge SRAM (`ram_bank`)           |
/// | 0x20000–0x2FFFF     | Cartridge ROM bank 0 (`rom_bank0`)    |
/// | 0x30000–0x3FFFF     | Cartridge ROM bank 1 (`rom_bank1`)    |
/// | 0x40000–0xFFFFF     | Cartridge ROM linear range            |
///
/// If an internal boot ROM is installed, it overlays the top of the 20-bit
/// space; a 64 KiB BIOS therefore maps at 0xF0000–0xFFFFF.
pub struct Bus {
    /// 64 KiB work RAM (only first 16 KiB accessible on WonderSwan mono).
    wram: Box<[u8]>,
    /// Cartridge ROM, SRAM, and bank-switch registers.
    cart: Cartridge,
    /// Optional internal boot ROM, top-aligned in the 20-bit physical address
    /// space. A 64 KiB image maps at 0xF0000–0xFFFFF and supplies the reset
    /// vector at 0xFFFF0.
    boot_rom: Option<Box<[u8]>>,
    /// Console internal EEPROM (IEEPROM), used by the real BIOS for owner
    /// settings and startup/configuration data through ports 0xBA–0xBE.
    ieeprom: Eeprom,
    /// Internal EEPROM status timing/protect state. The IEEPROM control port is
    /// not quite the same as cartridge EEPROM: READ clears DONE briefly before
    /// it becomes observable as complete.
    ieeprom_status: IeepromStatus,
    /// Shadow of all 256 I/O port registers.
    /// Exceptions (side-effect on read, read-only bits, etc.) are handled
    /// explicitly in `read_io` / `write_io`.
    ports: [u8; 0x100],
    /// CPU-visible wait cycles accumulated by synchronous I/O side effects.
    pending_wait_cycles: u32,
    /// Picture processing unit (renders from `wram` + display registers).
    ppu: Ppu,
    /// Audio processing unit (samples waveforms from `wram` + sound registers).
    apu: Apu,
    /// Sound DMA internal state. Register shadows live in `ports[0x4A..=0x52]`;
    /// these fields track the currently-running transfer and sample-rate clock.
    sdma: SdmaState,
    /// Currently-held keys, presented on port 0xB5 when scanned.
    keys: KeyState,
    /// Emulated hardware variant, selecting model-dependent behaviour (palette
    /// resolver, tile formats, RAM window). Defaults to [`HardwareModel::Mono`].
    model: HardwareModel,
}

impl Bus {
    fn init_port_defaults(ports: &mut [u8; 0x100]) {
        // Keep the speaker path audible unless software explicitly lowers the
        // built-in speaker main volume. Treating reset as 0 mutes games/tests
        // that rely on the default speaker route and never touch port 0x9E.
        ports[0x9E] = 0x03;
    }

    fn mono_palette_port_mask(port: u8) -> u8 {
        match port {
            0x20..=0x27 | 0x30..=0x37 => 0x77,
            0x28..=0x2F | 0x38..=0x3F if port & 1 == 0 => 0x70,
            0x28..=0x2F | 0x38..=0x3F => 0x77,
            _ => 0xFF,
        }
    }

    /// Create a bus with the given ROM bytes (and no SRAM).
    pub fn new(rom: Vec<u8>) -> Self {
        let mut bus = Self {
            wram: vec![0u8; 0x10000].into_boxed_slice(),
            cart: Cartridge::new(rom, Vec::new()),
            boot_rom: None,
            ieeprom: Eeprom::new(vec![0; 2048], 10),
            ieeprom_status: IeepromStatus::default(),
            ports: [0u8; 0x100],
            pending_wait_cycles: 0,
            ppu: Ppu::new(),
            apu: Apu::new(),
            sdma: SdmaState::default(),
            keys: KeyState::NONE,
            model: HardwareModel::Mono,
        };
        Self::init_port_defaults(&mut bus.ports);
        bus
    }

    /// Create a bus with the given ROM and SRAM bytes.
    pub fn with_sram(rom: Vec<u8>, sram: Vec<u8>) -> Self {
        let mut bus = Self {
            wram: vec![0u8; 0x10000].into_boxed_slice(),
            cart: Cartridge::new(rom, sram),
            boot_rom: None,
            ieeprom: Eeprom::new(vec![0; 2048], 10),
            ieeprom_status: IeepromStatus::default(),
            ports: [0u8; 0x100],
            pending_wait_cycles: 0,
            ppu: Ppu::new(),
            apu: Apu::new(),
            sdma: SdmaState::default(),
            keys: KeyState::NONE,
            model: HardwareModel::Mono,
        };
        Self::init_port_defaults(&mut bus.ports);
        bus
    }

    /// Create a bus from a ROM image, allocating the cartridge's save medium
    /// (SRAM or EEPROM) and configuring its mapper from the parsed header. The
    /// hardware model defaults to the header's Color-required flag (see
    /// [`HardwareModel::from_color_flag`]); use [`Bus::set_model`] to override.
    pub fn from_rom(rom: Vec<u8>) -> Self {
        let cart = Cartridge::from_rom(rom);
        let model = HardwareModel::from_color_flag(cart.header().is_some_and(|h| h.color));
        let mut bus = Self {
            wram: vec![0u8; 0x10000].into_boxed_slice(),
            cart,
            boot_rom: None,
            ieeprom: Eeprom::new(vec![0; 2048], 10),
            ieeprom_status: IeepromStatus::default(),
            ports: [0u8; 0x100],
            pending_wait_cycles: 0,
            ppu: Ppu::new(),
            apu: Apu::new(),
            sdma: SdmaState::default(),
            keys: KeyState::NONE,
            model,
        };
        Self::init_port_defaults(&mut bus.ports);
        bus
    }

    /// Returns a shared reference to the cartridge (ROM, SRAM, bank registers).
    pub fn cart(&self) -> &Cartridge {
        &self.cart
    }

    /// The cartridge's persistent save bytes (SRAM or EEPROM contents), for the
    /// frontend to write to disk. Empty if the cartridge has no save medium.
    pub fn save_data(&self) -> &[u8] {
        self.cart.save_data()
    }

    /// Restore the cartridge's save medium from previously serialised bytes.
    pub fn load_save_data(&mut self, data: &[u8]) {
        self.cart.load_save_data(data);
    }

    /// Returns a mutable reference to the cartridge.
    pub fn cart_mut(&mut self) -> &mut Cartridge {
        &mut self.cart
    }

    /// Install an internal boot ROM image. The image is mapped top-aligned in
    /// the 20-bit address space and overrides cartridge ROM reads there.
    ///
    /// NewOswan's boot-ROM stubs are distributed four bytes shorter than their
    /// mapped 4 KiB / 8 KiB blocks. Pad to the next 4 KiB boundary so the reset
    /// entry at file offset 0x0FF0 / 0x1FF0 lands at physical 0xFFFF0.
    pub fn install_boot_rom(&mut self, boot_rom: Vec<u8>) {
        let mapped_len = boot_rom
            .len()
            .next_multiple_of(BOOT_ROM_ALIGNMENT)
            .min(ADDRESS_SPACE_SIZE as usize);
        let mut mapped = vec![0xFF; mapped_len];
        let copy_len = boot_rom.len().min(mapped_len);
        mapped[..copy_len].copy_from_slice(&boot_rom[..copy_len]);
        self.boot_rom = Some(mapped.into_boxed_slice());
    }

    /// Read the raw shadow value of an I/O port without side effects.
    ///
    /// Unlike [`MemoryBus::read_io`], this never clears edge-triggered bits or
    /// applies read masks; it is a debugging/tooling accessor for ports that
    /// hold plain register state (display control, scroll, line-compare, …).
    pub fn peek_io(&self, port: u8) -> u8 {
        self.ports[port as usize]
    }

    /// Debug helper: the raw `(pixel, palette)` a background layer samples at
    /// screen coordinate `(x, y)` (`scr2 = true` selects SCR2). For diagnosing
    /// layer compositing and transparency.
    pub fn debug_bg_sample(&self, scr2: bool, x: usize, y: u8) -> (u8, u8) {
        self.ppu
            .debug_bg_sample(&self.wram, &self.ports, scr2, x, y)
    }

    // ── Key matrix ────────────────────────────────────────────────────────

    /// Set the currently-held keys (read back through port 0xB5).
    ///
    /// Newly-pressed keys raise [`IrqSource::KeyPress`]; the frontend calls this
    /// once per frame with the host input mapped to a [`KeyState`]. The press
    /// interrupt is modelled at frame granularity (an edge versus the previous
    /// call), which is sufficient for the maskable wake-from-HALT use case.
    pub fn set_keys(&mut self, keys: KeyState) {
        let newly_pressed = keys.bits() & !self.keys.bits();
        self.keys = keys;
        if newly_pressed != 0 {
            self.request_irq(IrqSource::KeyPress);
        }
    }

    // ── Interrupt controller ──────────────────────────────────────────────

    /// Assert a hardware interrupt request source.
    pub fn request_irq(&mut self, src: IrqSource) {
        self.ports[0xB4] |= (1 << src as u8) & self.ports[0xB2];
    }

    fn highest_int_cause_bit(&self) -> u8 {
        for priority in (0..8u8).rev() {
            if self.ports[0xB4] & (1 << priority) != 0 {
                return priority;
            }
        }
        0
    }

    fn refresh_serial_tx_irq(&mut self) {
        if self.model == HardwareModel::Mono
            && self.ports[0xB2] & SERIAL_TX_READY_IRQ != 0
            && self.ports[0xB3] & SERIAL_ENABLE != 0
        {
            self.ports[0xB4] |= SERIAL_TX_READY_IRQ;
        }
    }

    /// Returns the interrupt vector number for the highest-priority pending
    /// and enabled interrupt, or `None` if there is nothing to service.
    ///
    /// The vector = `INT_BASE` (port 0xB0) + IRQ priority bit position.
    /// The caller must check `cpu.flags.interrupt` for maskable IRQs before
    /// calling `cpu.handle_irq(bus, vector)`.
    pub fn pending_irq(&self) -> Option<u8> {
        let pending = self.ports[0xB4];
        if pending == 0 {
            return None;
        }
        // Highest bit wins (priority: 7 > 6 > … > 0)
        for priority in (0..8u8).rev() {
            if pending & (1 << priority) != 0 {
                return Some(self.ports[0xB0].wrapping_add(priority));
            }
        }
        None
    }

    // ── Timer events (called by the PPU / display controller) ────────────

    /// Notify the bus that an HBlank period has begun.
    ///
    /// Decrements the HBlank timer counter (if enabled); fires
    /// [`IrqSource::HBlankTimer`] when the counter reaches zero.
    pub fn on_hblank(&mut self) {
        let counter = u16::from_le_bytes([self.ports[0xA8], self.ports[0xA9]]);
        if counter == 0 {
            return;
        }
        let irq_bit = 1 << IrqSource::HBlankTimer as u8;
        if self.ports[0xA2] & 1 == 0 && (counter != 1 || self.ports[0xB2] & irq_bit == 0) {
            return; // HBlank timer disabled, except the enabled counter=1 latch quirk.
        }
        if counter == 1 {
            self.ports[0xB4] |= irq_bit & self.ports[0xB2];
            if self.ports[0xA2] & 2 != 0 {
                // auto-reload
                self.ports[0xA8] = self.ports[0xA4];
                self.ports[0xA9] = self.ports[0xA5];
            } else {
                self.ports[0xA8] = 0;
                self.ports[0xA9] = 0;
            }
        } else {
            let [lo, hi] = (counter - 1).to_le_bytes();
            self.ports[0xA8] = lo;
            self.ports[0xA9] = hi;
        }
    }

    /// Notify the bus that a VBlank period has begun.
    ///
    /// Fires [`IrqSource::VBlank`] (always enabled) and decrements the
    /// VBlank timer counter, firing [`IrqSource::VBlankTimer`] at zero.
    pub fn on_vblank(&mut self) {
        // VBLANK interrupt (latched only when enabled; WSHWTest verifies that
        // disabled interrupt sources do not appear in INT_CAUSE).
        self.ports[0xB4] |= (1 << IrqSource::VBlank as u8) & self.ports[0xB2];

        if self.ports[0xA2] & 4 == 0 {
            return; // VBlank timer disabled
        }
        let counter = u16::from_le_bytes([self.ports[0xAA], self.ports[0xAB]]);
        if counter == 0 {
            return;
        }
        if counter == 1 {
            self.ports[0xB4] |= (1 << IrqSource::VBlankTimer as u8) & self.ports[0xB2];
            if self.ports[0xA2] & 8 != 0 {
                // auto-reload
                self.ports[0xAA] = self.ports[0xA6];
                self.ports[0xAB] = self.ports[0xA7];
            } else {
                self.ports[0xAA] = 0;
                self.ports[0xAB] = 0;
            }
        } else {
            let [lo, hi] = (counter - 1).to_le_bytes();
            self.ports[0xAA] = lo;
            self.ports[0xAB] = hi;
        }
    }

    /// Notify the bus that the current scanline matches the compare register.
    /// Fires [`IrqSource::ScanlineMatch`] if enabled.
    pub fn on_scanline_match(&mut self) {
        self.ports[0xB4] |= (1 << IrqSource::ScanlineMatch as u8) & self.ports[0xB2];
    }

    /// Update the current scanline register (port 0x02) and fire a scanline
    /// match interrupt when `line == ports[0x03]`.
    pub fn set_current_scanline(&mut self, line: u8) {
        self.ports[0x02] = line;
        if self.ports[0x02] == self.ports[0x03] {
            self.on_scanline_match();
        }
    }

    // ── GDMA ─────────────────────────────────────────────────────────────

    /// Execute a pending GDMA transfer synchronously (if armed via port 0x48).
    ///
    /// Normally invoked from the port-0x48 write handler the instant a game arms
    /// the transfer, mirroring the hardware's synchronous, CPU-stalling burst.
    /// Kept public for tests that arm and drive GDMA directly.
    ///
    /// Returns the CPU wait cycles consumed by the transfer (0 if GDMA was not
    /// active or the source immediately aborts). Fires [`IrqSource::GdmaComplete`]
    /// on completion.
    ///
    /// The transfer is aborted if the source address enters the SRAM range
    /// (0x10000–0x1FFFF), matching WonderCrab behaviour.
    pub fn tick_gdma(&mut self) -> u32 {
        if self.ports[0x48] & 0x80 == 0 {
            return 0;
        }
        let src_off = u16::from_le_bytes([self.ports[0x40], self.ports[0x41]]) as u32;
        let src_seg = (self.ports[0x42] & 0x0F) as u32;
        let mut src = (src_seg << 16) | src_off;

        let mut dst = u16::from_le_bytes([self.ports[0x44], self.ports[0x45]]) as u32;
        let mut len = u16::from_le_bytes([self.ports[0x46], self.ports[0x47]]);

        if len == 0 {
            self.ports[0x48] &= 0x7F;
            return 0;
        }

        let decrement = self.ports[0x48] & 0x40 != 0;
        let mut transferred = 0u32;

        while len > 0 {
            if self.gdma_source_blocked(src) {
                break;
            }
            let byte = self.read_u8_phys(src);
            self.write_wram(dst & 0xFFFF, byte);

            if decrement {
                src = src.wrapping_sub(1);
                dst = dst.wrapping_sub(1);
            } else {
                src = src.wrapping_add(1);
                dst = dst.wrapping_add(1);
            }
            len -= 1;
            transferred += 1;
        }

        // Write back updated pointers and length
        let [lo, hi] = (src as u16).to_le_bytes();
        self.ports[0x40] = lo;
        self.ports[0x41] = hi;
        self.ports[0x42] = ((src >> 16) & 0x0F) as u8;
        let [lo, hi] = (dst as u16).to_le_bytes();
        self.ports[0x44] = lo;
        self.ports[0x45] = hi;
        let [lo, hi] = len.to_le_bytes();
        self.ports[0x46] = lo;
        self.ports[0x47] = hi;
        self.ports[0x48] &= 0x7F; // clear enable bit

        self.ports[0xB4] |= (1 << IrqSource::GdmaComplete as u8) & self.ports[0xB2];
        if transferred == 0 {
            0
        } else {
            5 + transferred
        }
    }

    fn gdma_source_blocked(&self, src: u32) -> bool {
        // SRAM sources abort the burst. The color DMA alignment ROM also
        // confirms that the 0x80000 slow-ROM window is unavailable while the
        // ROM wait bit is set in SYSTEM_CTRL1. Keep the rest of the linear ROM
        // range available; commercial titles such as FF4 use upper-ROM GDMA
        // while this flag is set.
        (0x10000..=0x1FFFF).contains(&src)
            || ((0x80000..=0x8FFFF).contains(&src)
                && (self.ports[0xA0] & SYSTEM_CTRL1_ROM_WAIT != 0))
    }

    // ── PPU ──────────────────────────────────────────────────────────────

    /// Render visible scanline `line` (0–143) into the PPU framebuffer and
    /// advance the per-scanline timing hooks: the LCD line-compare interrupt
    /// ([`Bus::set_current_scanline`]) and the HBlank timer
    /// ([`Bus::on_hblank`]).
    ///
    /// The frontend (or a future system-level driver) calls this once per
    /// visible scanline, interleaved with CPU execution, then
    /// [`Bus::on_vblank`] at the end of the frame.
    pub fn render_scanline(&mut self, line: u8) {
        if self.color_rendering_enabled() {
            let resolver = ColorPaletteResolver::new(&self.wram);
            self.ppu
                .render_scanline(line, &self.wram, &self.ports, &resolver);
        } else {
            self.ppu
                .render_scanline(line, &self.wram, &self.ports, &MonoPaletteResolver);
        }
        self.set_current_scanline(line);
        self.on_hblank();
    }

    /// Whether the PPU should render in WonderSwan Color mode: the emulated
    /// model has color features *and* the video-mode register (I/O port 0x60)
    /// has the color-mode bit (bit 7) set. A Color console running a
    /// monochrome-compatible title (bit 7 clear) uses the mono shade path.
    ///
    /// (Games normally rely on the boot ROM to set port 0x60; when booting
    /// without it, a Color title must set the bit itself.)
    fn color_rendering_enabled(&self) -> bool {
        self.model.is_color() && (self.ports[0x60] & 0x80 != 0)
    }

    fn hypervoice_enabled(&self) -> bool {
        self.color_rendering_enabled()
    }

    /// The current PPU framebuffer: 224×144 [`Rgb444`] colors, row-major.
    /// Stable read API for the frontend and RetroAchievements.
    pub fn framebuffer(&self) -> &[Rgb444] {
        self.ppu.framebuffer()
    }

    /// The emulated hardware model.
    pub fn model(&self) -> HardwareModel {
        self.model
    }

    /// Override the emulated hardware model (e.g. to force Color or Crystal on a
    /// Color-capable cartridge). Takes effect on the next rendered scanline.
    pub fn set_model(&mut self, model: HardwareModel) {
        self.model = model;
    }

    // ── APU ──────────────────────────────────────────────────────────────

    /// Advance the APU by `cycles` sound-clock ticks (one per CPU cycle),
    /// generating audio samples from the waveform data in WRAM and the sound
    /// registers. Sweep and noise write back into the register file.
    pub fn tick_apu(&mut self, cycles: u32) {
        if !self.model.is_color() || self.ports[0x52] & SDMA_ENABLE == 0 {
            self.sdma.running = false;
            self.sdma.clock = 0;
            let hypervoice = self.hypervoice_enabled();
            self.apu
                .tick(cycles, &self.wram, &mut self.ports, hypervoice);
            return;
        }

        for _ in 0..cycles {
            self.tick_sdma_cycle();
            let hypervoice = self.hypervoice_enabled();
            self.apu.tick(1, &self.wram, &mut self.ports, hypervoice);
        }
    }

    /// The interleaved stereo samples (`L, R, …`) generated so far, at
    /// [`Apu::OUTPUT_SAMPLE_RATE`]. Stable read API for the audio frontend.
    pub fn audio_samples(&self) -> &[i16] {
        self.apu.samples()
    }

    /// Drop all buffered audio samples (call after the frontend consumes them).
    pub fn clear_audio_samples(&mut self) {
        self.apu.clear_samples();
    }

    // ── Cartridge RTC ─────────────────────────────────────────────────────

    /// Whether the inserted cartridge carries a real-time clock.
    pub fn has_rtc(&self) -> bool {
        self.cart.has_rtc()
    }

    /// Advance the cartridge RTC (if any) by `cycles` master-clock ticks.
    ///
    /// The RTC free-runs off the emulated clock rather than wall-clock time, to
    /// keep the core deterministic; see [`Rtc`].
    pub fn tick_rtc(&mut self, cycles: u32) {
        if let Some(rtc) = self.cart.rtc_mut() {
            rtc.tick(cycles);
        }
    }

    /// Inject an absolute date/time into the cartridge RTC (no-op without one).
    ///
    /// Called once by the frontend from the host clock at ROM load; the core
    /// never reads wall-clock time itself. See [`Rtc::set_datetime`] for the
    /// component encoding.
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
        if let Some(rtc) = self.cart.rtc_mut() {
            rtc.set_datetime(year, month, day, weekday, hour, minute, second);
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────

    /// Physical memory read without going through the `MemoryBus` trait
    /// (avoids borrow conflicts in `tick_gdma`).
    fn read_u8_phys(&self, addr: u32) -> u8 {
        let addr = addr & ADDRESS_MASK;
        if let Some(value) = self.read_boot_rom(addr) {
            return value;
        }
        match addr {
            a @ 0x00000..=0x0FFFF => self.read_wram(a),
            a @ 0x10000..=0x1FFFF => self.cart.read_sram(a),
            a @ 0x20000..=0x2FFFF => self.cart.read_rom0(a),
            a @ 0x30000..=0x3FFFF => self.cart.read_rom1(a),
            a @ 0x40000..=0xFFFFF => self.cart.read_rom_ex(a),
            _ => OPEN_BUS,
        }
    }

    /// Read an internal-RAM byte for physical address `a` in 0x00000–0x0FFFF.
    ///
    /// WonderSwan mono has 16 KiB of internal RAM (0x00000–0x03FFF); the upper
    /// 48 KiB window (0x04000–0x0FFFF) — which holds the Color palette RAM at
    /// 0xFE00 and the 4bpp tile banks at 0x4000 — is only present on Color
    /// models. On mono it reads as open bus.
    fn read_wram(&self, a: u32) -> u8 {
        if a <= 0x03FFF || self.model.is_color() {
            self.wram[a as usize]
        } else {
            OPEN_BUS
        }
    }

    /// Write an internal-RAM byte for physical address `a` in 0x00000–0x0FFFF.
    /// The upper 48 KiB window is writable only on Color models; on mono it is
    /// open bus and the write is dropped. See [`Bus::read_wram`].
    fn write_wram(&mut self, a: u32, value: u8) {
        if a <= 0x03FFF || self.model.is_color() {
            self.wram[a as usize] = value;
        }
    }

    fn read_boot_rom(&self, addr: u32) -> Option<u8> {
        let boot_rom = self.boot_rom.as_ref()?;
        let len = u32::try_from(boot_rom.len()).ok()?.min(ADDRESS_SPACE_SIZE);
        let base = ADDRESS_SPACE_SIZE - len;
        (addr >= base).then(|| boot_rom[(addr - base) as usize])
    }

    fn write_voice_data_latch(&mut self, value: u8) {
        self.ports[0x89] = value;
        if self.ports[0x90] & SND_CTRL_VOICE != 0 {
            self.apu.write_voice(value);
        }
    }

    fn write_hypervoice_direct(&mut self, port: u8, value: u8) {
        self.ports[port as usize] = value;
        self.ports[0x69] = 0;
    }

    fn write_hypervoice_data_latch(&mut self, value: u8) {
        self.ports[0x64..=0x67].fill(0);
        self.ports[0x69] = value;
    }

    fn sdma_rate(&self) -> u16 {
        match self.ports[0x52] & 0x03 {
            0 => 6,
            1 => 4,
            2 => 2,
            _ => 1,
        }
    }

    fn sdma_source_from_ports(&self) -> u32 {
        let offset = u16::from_le_bytes([self.ports[0x4A], self.ports[0x4B]]) as u32;
        let segment = (self.ports[0x4C] & 0x0F) as u32;
        (segment << 16) | offset
    }

    fn sdma_counter_from_ports(&self) -> u32 {
        let offset = u16::from_le_bytes([self.ports[0x4E], self.ports[0x4F]]) as u32;
        let segment = (self.ports[0x50] & 0x0F) as u32;
        (segment << 16) | offset
    }

    fn write_sdma_source(&mut self, value: u32) {
        let [lo, hi] = (value as u16).to_le_bytes();
        self.ports[0x4A] = lo;
        self.ports[0x4B] = hi;
        self.ports[0x4C] = ((value >> 16) & 0x0F) as u8;
    }

    fn write_sdma_counter(&mut self, value: u32) {
        let [lo, hi] = (value as u16).to_le_bytes();
        self.ports[0x4E] = lo;
        self.ports[0x4F] = hi;
        self.ports[0x50] = ((value >> 16) & 0x0F) as u8;
    }

    fn start_sdma_if_needed(&mut self) -> bool {
        if !self.model.is_color() || self.ports[0x52] & SDMA_ENABLE == 0 {
            self.sdma.running = false;
            self.sdma.clock = 0;
            return false;
        }
        if self.sdma.running {
            return true;
        }

        let counter = self.sdma_counter_from_ports();
        if counter == 0 {
            self.ports[0x52] &= !SDMA_ENABLE;
            return false;
        }

        let source = self.sdma_source_from_ports();
        self.sdma.source = source;
        self.sdma.counter = counter;
        self.sdma.source_shadow = source;
        self.sdma.counter_shadow = counter;
        self.sdma.clock = 0;
        self.sdma.running = true;
        true
    }

    fn tick_sdma_cycle(&mut self) {
        if !self.start_sdma_if_needed() {
            return;
        }

        self.sdma.clock += 1;
        let period = SDMA_CYCLES_PER_SAMPLE * self.sdma_rate();
        if self.sdma.clock < period {
            return;
        }
        self.sdma.clock -= period;
        self.step_sdma_transfer();
    }

    fn step_sdma_transfer(&mut self) {
        let ctrl = self.ports[0x52];
        if ctrl & SDMA_HOLD != 0 {
            self.write_voice_data_latch(0);
            return;
        }

        let byte = self.read_u8_phys(self.sdma.source);
        self.write_voice_data_latch(byte);

        if ctrl & SDMA_DECREMENT != 0 {
            self.sdma.source = self.sdma.source.wrapping_sub(1) & ADDRESS_MASK;
        } else {
            self.sdma.source = self.sdma.source.wrapping_add(1) & ADDRESS_MASK;
        }
        self.sdma.counter = self.sdma.counter.wrapping_sub(1) & ADDRESS_MASK;

        if self.sdma.counter == 0 {
            if ctrl & SDMA_REPEAT != 0 {
                self.sdma.source = self.sdma.source_shadow;
                self.sdma.counter = self.sdma.counter_shadow;
            } else {
                self.ports[0x52] &= !SDMA_ENABLE;
                self.sdma.running = false;
                self.sdma.clock = 0;
            }
        }

        self.write_sdma_source(self.sdma.source);
        self.write_sdma_counter(self.sdma.counter);
    }

    fn read_ieeprom_status(&mut self) -> u8 {
        let status = self.ieeprom_status.value();
        self.ieeprom_status.poll();
        status
    }

    fn execute_ieeprom(&mut self, command: u16) {
        let address_bits = ieeprom_command_address_bits(command);
        self.ieeprom
            .execute_with_address_bits(command, address_bits);
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct IeepromStatus {
    done: bool,
    read_done_delay: u8,
    protect_fault: bool,
}

impl IeepromStatus {
    fn ready(&mut self) {
        self.done = false;
        self.read_done_delay = 0;
    }

    fn ready_done(&mut self) {
        self.done = true;
        self.read_done_delay = 0;
    }

    fn start_read(&mut self) {
        self.done = false;
        self.read_done_delay = 1;
    }

    fn value(&self) -> u8 {
        0x02 | u8::from(self.done) | if self.protect_fault { 0x80 } else { 0x00 }
    }

    fn poll(&mut self) {
        if self.read_done_delay > 0 {
            self.read_done_delay -= 1;
            if self.read_done_delay == 0 {
                self.done = true;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct SdmaState {
    source: u32,
    counter: u32,
    source_shadow: u32,
    counter_shadow: u32,
    clock: u16,
    running: bool,
}

fn ieeprom_command_address_bits(command: u16) -> u8 {
    if command & (1 << 12) != 0 {
        10
    } else {
        6
    }
}

fn command_byte_addr(command: u16) -> u16 {
    let mask = (1 << ieeprom_command_address_bits(command)) - 1;
    (command & mask) * 2
}

impl MemoryBus for Bus {
    fn read_u8(&self, addr: u32) -> u8 {
        let addr = addr & ADDRESS_MASK;
        if let Some(value) = self.read_boot_rom(addr) {
            return value;
        }
        match addr {
            a @ 0x00000..=0x0FFFF => self.read_wram(a),
            a @ 0x10000..=0x1FFFF => self.cart.read_sram(a),
            a @ 0x20000..=0x2FFFF => self.cart.read_rom0(a),
            a @ 0x30000..=0x3FFFF => self.cart.read_rom1(a),
            a @ 0x40000..=0xFFFFF => self.cart.read_rom_ex(a),
            _ => OPEN_BUS,
        }
    }

    fn write_u8(&mut self, addr: u32, value: u8) {
        match addr & 0xF_FFFF {
            a @ 0x00000..=0x0FFFF => self.write_wram(a, value),
            a @ 0x10000..=0x1FFFF => self.cart.write_sram(a, value),
            _ => {} // ROM is read-only
        }
    }

    fn read_io(&mut self, port: u8) -> u8 {
        match port {
            // DISP_CTRL: layer/window enable bits; upper bits read as zero.
            0x00 => self.ports[0x00] & 0x3F,
            // BG_PAL/backdrop: mono-compatible mode exposes a 3-bit shade-pool
            // index; color mode uses the full palette-RAM index.
            0x01 if self.color_rendering_enabled() => self.ports[0x01],
            0x01 => self.ports[0x01] & 0x07,
            // SPR_AREA: color mode can address the larger WRAM/tile space.
            0x04 if self.color_rendering_enabled() => self.ports[0x04] & 0x3F,
            0x04 => self.ports[0x04] & 0x1F,
            // Sprite start/count and screen-map base masks.
            0x05 => self.ports[0x05] & 0x7F,
            0x07 if self.color_rendering_enabled() => self.ports[0x07],
            0x07 => self.ports[0x07] & 0x77,
            // LCD segment data exposes the six icon bits.
            0x15 => self.ports[0x15] & 0x3F,
            // Unused LCD control holes.
            0x19 | 0x1B => 0,
            // Monochrome palette mapping ports expose only 3-bit shade-pool
            // selectors per nibble; some entries have no low-nibble selector.
            0x20..=0x3F => self.ports[port as usize] & Self::mono_palette_port_mask(port),
            // Color DMA register window is not visible in mono-compatible mode.
            0x40..=0x5F if !self.color_rendering_enabled() => 0,
            // GDMA source low: bit 0 always reads as 0
            0x40 => self.ports[0x40] & 0xFE,
            // GDMA source segment: bits 4-7 undefined (read as 0)
            0x42 => self.ports[0x42] & 0x0F,
            0x43 => 0,
            // GDMA destination: bit 0 always reads as 0
            0x44 => self.ports[0x44] & 0xFE,
            // GDMA counter: bit 0 always reads as 0
            0x46 => self.ports[0x46] & 0xFE,
            // GDMA ctrl: upper 2 bits only; self-clears on read
            0x48 => {
                let v = self.ports[0x48] & 0xC0;
                self.ports[0x48] = 0;
                v
            }
            0x49 => 0,
            // SDMA source segment: bits 4-7 undefined
            0x4C => self.ports[0x4C] & 0x0F,
            0x4D => 0,
            // SDMA counter segment: bits 4-7 undefined
            0x50 => self.ports[0x50] & 0x0F,
            0x51 => 0,
            0x52 => self.ports[0x52] & 0xDF,
            0x53..=0x5F => 0,
            // Unused system-control holes.
            0x61 | 0x63 => 0,
            // HyperVoice direct/data latches feed the audio path but are not
            // readable as ordinary state registers.
            0x64..=0x69 => 0,
            0x6A => self.ports[0x6A],
            0x6B => self.ports[0x6B] & 0x6F,
            0x6C..=0x7F => 0,
            // APU register masks.
            0x81 | 0x83 | 0x85 | 0x87 => self.ports[port as usize] & 0x07,
            0x8D => self.ports[0x8D] & 0x1F,
            0x8E => self.ports[0x8E] & 0x17,
            0x90 => self.ports[0x90] & 0xEF,
            0x91 => self.ports[0x91] & 0x8F,
            0x92 | 0x93 => self.ports[port as usize],
            0x94 => self.ports[0x94] & 0x0F,
            0x9E => self.ports[0x9E] & 0x03,
            0x9F => 0,
            0xA1 => 0,
            0xA2 | 0xA3 => self.ports[port as usize] & 0x0F,
            // HW_FLAGS (0xA0): console-model / system flags. Reads as a fixed
            // pattern with bit 0 = colour hardware — 0x87 on Color/Crystal, 0x86
            // on mono (Mednafen `gfx.c`: `wsc ? 0x87 : 0x86`). Games read this at
            // boot to detect a WonderSwan Color and take their colour path; a raw
            // 0x00 here makes them mis-initialise or hang.
            0xA0 => {
                let base = if self.model.is_color() { 0x87 } else { 0x86 };
                base | (self.ports[0xA0] & 0x08)
            }
            // HBlank/VBlank timer counters (read-only)
            0xA8 => self.ports[0xA8],
            0xA9 => self.ports[0xA9],
            0xAA => self.ports[0xAA],
            0xAB => self.ports[0xAB],
            0xAD..=0xAF => 0,
            0xB0 if self.model == HardwareModel::Mono => {
                (self.ports[0xB0] & 0xF8) | self.highest_int_cause_bit()
            }
            0xB0 => self.ports[0xB0] & 0xF8,
            0xB1 => self.ports[0xB1],
            0xB2 => self.ports[0xB2],
            // SERIAL_STATUS: writable control/status bits exposed by WSHWTest.
            0xB3 => self.ports[0xB3] & 0xC4,
            // INT_CAUSE: reading clears edge-triggered bits (1, 4, 5, 6, 7)
            0xB4 => {
                self.refresh_serial_tx_irq();
                let v = self.ports[0xB4];
                self.ports[0xB4] &= !0b1111_0010;
                v
            }
            // KEYPAD: low nibble = scanned keys, high nibble = group selector
            0xB5 => self.keys.scan(self.ports[0xB5] & 0x70),
            // INT_CAUSE_CLEAR is write-only
            0xB6 => 0,
            // INT_NMI_CTRL: clears all but bit 4 on read
            0xB7 => {
                let v = self.ports[0xB7] & 0x10;
                self.ports[0xB7] = v;
                v
            }
            // Internal EEPROM data/command/status ports.
            0xBA => self.ieeprom.read_data().to_le_bytes()[0],
            0xBB => self.ieeprom.read_data().to_le_bytes()[1],
            0xBC..=0xBD => self.ports[port as usize],
            0xBE => self.read_ieeprom_status(),
            0xBF => self.ports[0xBF] & 0x01,
            // Cartridge bank registers (low byte; both mappers)
            0xC0 => self.cart.linear_off,
            0xC1 => self.cart.ram_bank,
            0xC2 => self.cart.rom_bank0,
            0xC3 => self.cart.rom_bank1,
            // Cartridge serial-EEPROM data/command latches
            0xC4..=0xC7 if self.cart.has_eeprom() => self.ports[port as usize],
            // EEPROM status: bit 1 is ready; bit 0 is the latched DONE status.
            0xC8 if self.cart.has_eeprom() => 0x02 | (self.ports[0xC8] & 0x01),
            0xC4..=0xC9 => OPEN_BUS,
            // Bandai 2003 high-byte bank registers (open bus on 2001)
            0xD0..=0xD5 if self.cart.mapper() == Mapper::Bandai2003 => match port {
                0xD0 => self.cart.ram_bank,
                0xD1 => self.cart.ram_bank_hi,
                0xD2 => self.cart.rom_bank0,
                0xD3 => self.cart.rom_bank0_hi,
                0xD4 => self.cart.rom_bank1,
                _ => self.cart.rom_bank1_hi,
            },
            0xD0..=0xD5 => OPEN_BUS,
            // Cartridge RTC command/status and data ports (only when present)
            0xCA if self.cart.has_rtc() => self.cart.rtc().map_or(OPEN_BUS, |r| r.read_command()),
            0xCB if self.cart.has_rtc() => self.cart.rtc_mut().map_or(OPEN_BUS, |r| r.read_data()),
            0xCA..=0xCB => OPEN_BUS,
            // Default: return raw shadow value
            p => self.ports[p as usize],
        }
    }

    fn write_io(&mut self, port: u8, value: u8) {
        match port {
            // DISP_CTRL: layer/window enable bits; upper bits are not writable.
            0x00 => self.ports[0x00] = value & 0x3F,
            0x04 if self.color_rendering_enabled() => self.ports[0x04] = value & 0x3F,
            0x04 => self.ports[0x04] = value & 0x1F,
            0x05 => self.ports[0x05] = value & 0x7F,
            0x07 if self.color_rendering_enabled() => self.ports[0x07] = value,
            0x07 => self.ports[0x07] = value & 0x77,
            0x15 => self.ports[0x15] = value & 0x3F,
            0x19 | 0x1B => {}
            0x20..=0x3F => {
                self.ports[port as usize] = value & Self::mono_palette_port_mask(port);
            }
            0x40..=0x5F if !self.color_rendering_enabled() => {}
            // LCD_LINE (0x02) is read-only
            0x02 => {}
            // GDMA source low: bit 0 forced to 0
            0x40 => self.ports[0x40] = value & 0xFE,
            // GDMA source segment: bits 4-7 ignored
            0x42 => self.ports[0x42] = value & 0x0F,
            0x43 => {}
            // GDMA destination: bit 0 forced to 0
            0x44 => self.ports[0x44] = value & 0xFE,
            // GDMA counter: bit 0 forced to 0
            0x46 => self.ports[0x46] = value & 0xFE,
            // GDMA control: writing bit 7 starts a synchronous burst transfer,
            // exactly as on hardware where OUT to this port stalls the CPU for
            // the duration of the copy. Executing it here — rather than deferring
            // to a per-scanline tick — is required for correctness: a game that
            // triggers several GDMAs within one scanline's CPU budget (e.g. Lode
            // Runner updating its tilemap row by row) would otherwise have all but
            // the last transfer silently dropped, since each arm overwrites the
            // shared GDMA register file (0x40–0x48).
            0x48 => {
                self.ports[0x48] = value;
                self.pending_wait_cycles += self.tick_gdma();
            }
            0x49 => {}
            // SDMA source segment: bits 4-7 ignored
            0x4C => self.ports[0x4C] = value & 0x0F,
            0x4D => {}
            // SDMA counter segment: bits 4-7 ignored
            0x50 => self.ports[0x50] = value & 0x0F,
            0x51 => {}
            0x52 => self.ports[0x52] = value,
            0x53..=0x5F => {}
            0x61 | 0x63 => {}
            // HW_FLAGS / system control. The real BIOS finishes by running a
            // tiny WRAM trampoline that reads 0xA0, increments it, writes it
            // back, and jumps to the cartridge reset vector. Treat writes with
            // bit 7 set as the internal boot-ROM disable latch.
            0xA0 => {
                self.ports[0xA0] = value;
                if value & 0x80 != 0 {
                    self.boot_rom = None;
                }
            }
            // HyperVoice (WonderSwan Color only): signed 16-bit direct output
            // words (0x64-0x67), 8-bit PCM data latch (0x69), control (0x6A),
            // and channel routing (0x6B). On mono hardware these registers do
            // not exist, so writes are dropped (the same open-bus-on-mono
            // treatment as the 8d upper-RAM window). The APU mix is also gated
            // by port 0x60 bit 7, because HyperVoice is unavailable when WSC
            // color mode is disabled.
            0x64..=0x67 if self.model.is_color() => self.write_hypervoice_direct(port, value),
            0x68 if self.model.is_color() => self.ports[0x68] = value,
            0x69 if self.model.is_color() => self.write_hypervoice_data_latch(value),
            0x6A if self.model.is_color() => self.ports[0x6A] = value,
            0x6B if self.model.is_color() => self.ports[0x6B] = value & 0x6F,
            0x64..=0x7F => {} // mono/unused HyperVoice-adjacent holes
            0x81 | 0x83 | 0x85 | 0x87 => self.ports[port as usize] = value & 0x07,
            0x8D => self.ports[0x8D] = value & 0x1F,
            0x8E => {
                self.ports[0x8E] = value & 0x1F;
                if self.ports[0x8E] & 0x08 != 0 {
                    self.apu.reset_noise_lfsr(&mut self.ports);
                }
            }
            0x90 => self.ports[0x90] = value & 0xEF,
            0x91 => self.ports[0x91] = value & 0x8F,
            0x92 | 0x93 => {}
            0x94 => self.ports[0x94] = value & 0x0F,
            0x9F => {}
            0xA1 => {}
            0xA2 | 0xA3 => self.ports[port as usize] = value & 0x0F,
            // Built-in speaker main volume. WSdev documents only the low two
            // bits as meaningful; the APU applies it to the speaker path only.
            0x9E => self.ports[0x9E] = value & 0x03,
            // Voice (channel-2 PCM) data latch. In voice mode every write feeds
            // the APU's reconstruction filter, so it sees the full PCM stream
            // (games write it faster than the audio rate); outside voice mode the
            // register is just channel-2's volume.
            0x89 => self.write_voice_data_latch(value),
            // HBlank timer period: writing also resets the counter
            0xA4 => {
                self.ports[0xA4] = value;
                self.ports[0xA8] = value;
            }
            0xA5 => {
                self.ports[0xA5] = value;
                self.ports[0xA9] = value;
            }
            // VBlank timer period: writing also resets the counter
            0xA6 => {
                self.ports[0xA6] = value;
                self.ports[0xAA] = value;
            }
            0xA7 => {
                self.ports[0xA7] = value;
                self.ports[0xAB] = value;
            }
            // Timer counters are read-only
            0xA8..=0xAB => {}
            0xAD..=0xAF => {}
            0xB0 => self.ports[0xB0] = value & 0xF8,
            0xB1 => {}
            0xB2 => {
                self.ports[0xB2] = value;
                self.refresh_serial_tx_irq();
            }
            0xB3 => {
                self.ports[0xB3] = value & 0xC4;
                self.refresh_serial_tx_irq();
            }
            // INT_CAUSE is read-only (clear via INT_CAUSE_CLEAR)
            0xB4 => {}
            // KEYPAD: only the group-selector nibble is writable
            0xB5 => self.ports[0xB5] = value & 0x70,
            // INT_CAUSE_CLEAR: writing 1 clears the corresponding INT_CAUSE bits
            0xB6 => {
                self.ports[0xB6] = value;
                self.ports[0xB4] &= !value;
            }
            // Internal EEPROM: 0xBA/0xBB data latch, 0xBC/0xBD command latch,
            // 0xBE control/status. Port 0xBF is retained as a low-bit shadow;
            // the protected byte range itself is hardware-enforced.
            0xBA..=0xBD => self.ports[port as usize] = value,
            0xBE => {
                let command = u16::from_le_bytes([self.ports[0xBC], self.ports[0xBD]]);
                match value {
                    0x10 => {
                        self.execute_ieeprom(command);
                        self.ieeprom_status.start_read();
                    }
                    0x20 => {
                        let data = u16::from_le_bytes([self.ports[0xBA], self.ports[0xBB]]);
                        self.ieeprom.write_data(data);
                        if command_byte_addr(command) >= 0x60 {
                            self.ieeprom_status.protect_fault = true;
                            self.ieeprom_status.ready_done();
                            return;
                        }
                        self.execute_ieeprom(command);
                        self.ieeprom_status.ready();
                    }
                    0x40 => {
                        self.execute_ieeprom(command);
                        self.ieeprom_status.ready_done();
                    }
                    _ => self.ieeprom_status.ready(),
                }
            }
            0xBF => {
                self.ports[0xBF] = value & 0x01;
            }
            // Cartridge bank registers (write also updates the cart struct)
            0xC0 => {
                self.cart.linear_off = value & 0x3F;
                self.ports[0xC0] = value & 0x3F;
            }
            0xC1 => {
                self.cart.ram_bank = value;
                self.ports[0xC1] = value;
            }
            0xC2 => {
                self.cart.rom_bank0 = value;
                self.ports[0xC2] = value;
            }
            0xC3 => {
                self.cart.rom_bank1 = value;
                self.ports[0xC3] = value;
            }
            // Cartridge serial-EEPROM data/command latches
            0xC4..=0xC7 if self.cart.has_eeprom() => self.ports[port as usize] = value,
            // EEPROM control: high nibble selects the operation; the data latch
            // (0xC4/0xC5) is refreshed with the result of a READ.
            0xC8 if self.cart.has_eeprom() => {
                let operation = value >> 4;
                let data = u16::from_le_bytes([self.ports[0xC4], self.ports[0xC5]]);
                let comm = u16::from_le_bytes([self.ports[0xC6], self.ports[0xC7]]);
                let new_data = self.cart.eeprom_control(operation, data, comm);
                [self.ports[0xC4], self.ports[0xC5]] = new_data.to_le_bytes();
                let done = match operation {
                    0x04 => 0x00,        // Short command: DONE remains clear.
                    0x01 | 0x08 => 0x01, // Bandai 2001 DONE-bit errata.
                    _ => 0x00,
                };
                self.ports[0xC8] = (value & 0xF0) | 0x02 | done;
            }
            0xC4..=0xC9 => {} // EEPROM absent: ignore
            // Bandai 2003 high-byte bank registers (ignored on 2001)
            0xD0..=0xD5 if self.cart.mapper() == Mapper::Bandai2003 => match port {
                0xD0 => self.cart.ram_bank = value,
                0xD1 => self.cart.ram_bank_hi = value,
                0xD2 => self.cart.rom_bank0 = value,
                0xD3 => self.cart.rom_bank0_hi = value,
                0xD4 => self.cart.rom_bank1 = value,
                _ => self.cart.rom_bank1_hi = value,
            },
            0xD0..=0xD5 => {}
            // Cartridge RTC command/status and data ports (only when present)
            0xCA if self.cart.has_rtc() => {
                if let Some(r) = self.cart.rtc_mut() {
                    r.write_command(value);
                }
            }
            0xCB if self.cart.has_rtc() => {
                if let Some(r) = self.cart.rtc_mut() {
                    r.write_data(value);
                }
            }
            0xCA..=0xCB => {}
            // Default: raw write
            p => self.ports[p as usize] = value,
        }
    }

    fn take_wait_cycles(&mut self) -> u32 {
        let cycles = self.pending_wait_cycles;
        self.pending_wait_cycles = 0;
        cycles
    }
}
