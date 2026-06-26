//! WonderSwan hardware bus: memory map, I/O ports, interrupt controller,
//! HBlank/VBlank timers, and GDMA/SDMA.
//!
//! See `docs/dev/DevelopmentPlan.md` Phase 2 for the full design rationale.

mod cart;
#[cfg(test)]
mod tests;

pub use cart::Cartridge;

use crate::cpu::MemoryBus;

/// Open-bus return value for unmapped reads on WonderSwan mono.
const OPEN_BUS: u8 = 0x90;

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
pub struct Bus {
    /// 64 KiB work RAM (only first 16 KiB accessible on WonderSwan mono).
    wram: Box<[u8; 0x10000]>,
    /// Cartridge ROM, SRAM, and bank-switch registers.
    pub cart: Cartridge,
    /// Shadow of all 256 I/O port registers.
    /// Exceptions (side-effect on read, read-only bits, etc.) are handled
    /// explicitly in `read_io` / `write_io`.
    ports: [u8; 0x100],
}

impl Bus {
    /// Create a bus with the given ROM bytes (and no SRAM).
    pub fn new(rom: Vec<u8>) -> Self {
        let mut bus = Self {
            wram: Box::new([0u8; 0x10000]),
            cart: Cartridge::new(rom, Vec::new()),
            ports: [0u8; 0x100],
        };
        // INT_ENABLE: VBLANK (bit 6) is always forced on.
        bus.ports[0xB2] = 1 << IrqSource::VBlank as u8;
        bus
    }

    /// Create a bus with the given ROM and SRAM bytes.
    pub fn with_sram(rom: Vec<u8>, sram: Vec<u8>) -> Self {
        let mut bus = Self {
            wram: Box::new([0u8; 0x10000]),
            cart: Cartridge::new(rom, sram),
            ports: [0u8; 0x100],
        };
        bus.ports[0xB2] = 1 << IrqSource::VBlank as u8;
        bus
    }

    // ── Interrupt controller ──────────────────────────────────────────────

    /// Assert a hardware interrupt request source.
    pub fn request_irq(&mut self, src: IrqSource) {
        self.ports[0xB4] |= 1 << src as u8;
    }

    /// Returns the interrupt vector number for the highest-priority pending
    /// and enabled interrupt, or `None` if there is nothing to service.
    ///
    /// The vector = `INT_BASE` (port 0xB0) + IRQ priority bit position.
    /// The caller must check `cpu.flags.interrupt` for maskable IRQs before
    /// calling `cpu.handle_irq(bus, vector)`.
    pub fn pending_irq(&self) -> Option<u8> {
        let pending = self.ports[0xB4] & self.ports[0xB2];
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
        if self.ports[0xA2] & 1 == 0 {
            return; // HBlank timer disabled
        }
        let counter = u16::from_le_bytes([self.ports[0xA8], self.ports[0xA9]]);
        if counter == 0 {
            return;
        }
        if counter == 1 {
            self.ports[0xB4] |= (1 << IrqSource::HBlankTimer as u8) & self.ports[0xB2];
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
        // VBLANK interrupt (bit 6 always enabled)
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
    /// Returns the approximate number of CPU cycles consumed by the transfer
    /// (0 if GDMA was not active). Fires [`IrqSource::GdmaComplete`] on
    /// completion.
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
        let mut cycles = 0u32;

        while len > 0 {
            if (0x10000..=0x1FFFF).contains(&src) {
                break; // SRAM source: abort
            }
            let byte = self.read_u8_phys(src);
            self.wram[(dst & 0xFFFF) as usize] = byte;

            if decrement {
                src = src.wrapping_sub(1);
                dst = dst.wrapping_sub(1);
            } else {
                src = src.wrapping_add(1);
                dst = dst.wrapping_add(1);
            }
            len -= 1;
            cycles += 2;
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
        cycles
    }

    // ── Internal helpers ──────────────────────────────────────────────────

    /// Physical memory read without going through the `MemoryBus` trait
    /// (avoids borrow conflicts in `tick_gdma`).
    fn read_u8_phys(&self, addr: u32) -> u8 {
        match addr & 0xF_FFFF {
            a @ 0x00000..=0x03FFF => self.wram[a as usize],
            0x04000..=0x0FFFF => OPEN_BUS,
            a @ 0x10000..=0x1FFFF => self.cart.read_sram(a),
            a @ 0x20000..=0x2FFFF => self.cart.read_rom_bank(self.cart.rom_bank0, a),
            a @ 0x30000..=0x3FFFF => self.cart.read_rom_bank(self.cart.rom_bank1, a),
            a @ 0x40000..=0xFFFFF => self.cart.read_rom_ex(a),
            _ => OPEN_BUS,
        }
    }
}

impl MemoryBus for Bus {
    fn read_u8(&self, addr: u32) -> u8 {
        match addr & 0xF_FFFF {
            a @ 0x00000..=0x03FFF => self.wram[a as usize],
            0x04000..=0x0FFFF => OPEN_BUS,
            a @ 0x10000..=0x1FFFF => self.cart.read_sram(a),
            a @ 0x20000..=0x2FFFF => self.cart.read_rom_bank(self.cart.rom_bank0, a),
            a @ 0x30000..=0x3FFFF => self.cart.read_rom_bank(self.cart.rom_bank1, a),
            a @ 0x40000..=0xFFFFF => self.cart.read_rom_ex(a),
            _ => OPEN_BUS,
        }
    }

    fn write_u8(&mut self, addr: u32, value: u8) {
        match addr & 0xF_FFFF {
            a @ 0x00000..=0x03FFF => self.wram[a as usize] = value,
            0x04000..=0x0FFFF => {} // open bus on mono
            a @ 0x10000..=0x1FFFF => self.cart.write_sram(a, value),
            _ => {} // ROM is read-only
        }
    }

    fn read_io(&mut self, port: u8) -> u8 {
        match port {
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
            // SDMA source segment: bits 4-7 undefined
            0x4C => self.ports[0x4C] & 0x0F,
            0x4D => 0,
            // SDMA counter segment: bits 4-7 undefined
            0x50 => self.ports[0x50] & 0x0F,
            0x51 => 0,
            // HBlank/VBlank timer counters (read-only)
            0xA8 => self.ports[0xA8],
            0xA9 => self.ports[0xA9],
            0xAA => self.ports[0xAA],
            0xAB => self.ports[0xAB],
            // INT_ENABLE: VBLANK (bit 6) is always set
            0xB2 => self.ports[0xB2] | (1 << IrqSource::VBlank as u8),
            // SERIAL_STATUS stub: TX empty, RX empty
            0xB3 => 0x84,
            // INT_CAUSE: reading clears edge-triggered bits (1, 4, 5, 6, 7)
            0xB4 => {
                let v = self.ports[0xB4];
                self.ports[0xB4] &= !0b1111_0010;
                v
            }
            // INT_CAUSE_CLEAR is write-only
            0xB6 => 0,
            // INT_NMI_CTRL: clears all but bit 4 on read
            0xB7 => {
                let v = self.ports[0xB7] & 0x10;
                self.ports[0xB7] = v;
                v
            }
            // Cartridge bank registers
            0xC0 => self.cart.linear_off,
            0xC1 => self.cart.ram_bank,
            0xC2 => self.cart.rom_bank0,
            0xC3 => self.cart.rom_bank1,
            // Default: return raw shadow value
            p => self.ports[p as usize],
        }
    }

    fn write_io(&mut self, port: u8, value: u8) {
        match port {
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
            // SDMA source segment: bits 4-7 ignored
            0x4C => self.ports[0x4C] = value & 0x0F,
            0x4D => {}
            // SDMA counter segment: bits 4-7 ignored
            0x50 => self.ports[0x50] = value & 0x0F,
            0x51 => {}
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
            // INT_ENABLE: VBLANK (bit 6) always forced on
            0xB2 => self.ports[0xB2] = value | (1 << IrqSource::VBlank as u8),
            // SERIAL_STATUS is read-only
            0xB3 => {}
            // INT_CAUSE is read-only (clear via INT_CAUSE_CLEAR)
            0xB4 => {}
            // INT_CAUSE_CLEAR: writing 1 clears the corresponding INT_CAUSE bits
            0xB6 => {
                self.ports[0xB6] = value;
                self.ports[0xB4] &= !value;
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
            // Default: raw write
            p => self.ports[p as usize] = value,
        }
    }
}
