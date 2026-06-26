/// WonderSwan cartridge: ROM, optional SRAM, and bank-switch registers.
///
/// Memory map (physical address ranges handled by `Bus`):
/// - 0x10000–0x1FFFF: SRAM bank (selected by `ram_bank`)
/// - 0x20000–0x2FFFF: ROM bank 0 (selected by `rom_bank0`)
/// - 0x30000–0x3FFFF: ROM bank 1 (selected by `rom_bank1`)
/// - 0x40000–0xFFFFF: ROM linear range (offset by `linear_off`)
///
/// Bank addressing uses OR semantics: `(bank_reg as u32) << 16 | (addr & 0xFFFF)`,
/// taken modulo `rom.len()`. This allows roms of any power-of-two size to be
/// addressed correctly, and matches WonderCrab's reference implementation.
///
/// At power-on all bank registers initialise to 0xFF, so the ROM linear range
/// maps to the last bytes of the cartridge ROM (the reset vector / header area).
pub struct Cartridge {
    pub rom: Vec<u8>,
    pub sram: Vec<u8>,
    /// I/O port 0xC0: linear ROM address offset (bits 5:0 only; bit-masked on write).
    pub linear_off: u8,
    /// I/O port 0xC1: SRAM bank (full 8 bits).
    pub ram_bank: u8,
    /// I/O port 0xC2: ROM bank 0 (full 8 bits).
    pub rom_bank0: u8,
    /// I/O port 0xC3: ROM bank 1 (full 8 bits).
    pub rom_bank1: u8,
}

impl Cartridge {
    /// Create a cartridge from raw ROM bytes and optional SRAM.
    pub fn new(rom: Vec<u8>, sram: Vec<u8>) -> Self {
        Self {
            rom,
            sram,
            linear_off: 0xFF,
            ram_bank: 0xFF,
            rom_bank0: 0xFF,
            rom_bank1: 0xFF,
        }
    }

    /// Empty 64 KiB ROM / 32 KiB SRAM cartridge for unit tests.
    pub fn for_test() -> Self {
        Self::new(vec![0u8; 0x10000], vec![0u8; 0x8000])
    }

    pub fn read_sram(&self, addr: u32) -> u8 {
        if self.sram.is_empty() {
            return 0x90;
        }
        let bank = self.ram_bank as u32;
        let offset = ((bank << 16) | (addr & 0xFFFF)) as usize % self.sram.len();
        self.sram[offset]
    }

    pub fn write_sram(&mut self, addr: u32, value: u8) {
        if self.sram.is_empty() {
            return;
        }
        let bank = self.ram_bank as u32;
        let offset = ((bank << 16) | (addr & 0xFFFF)) as usize % self.sram.len();
        self.sram[offset] = value;
    }

    /// Read from a switchable ROM bank (bank 0 at 0x20000, bank 1 at 0x30000).
    pub fn read_rom_bank(&self, bank_select: u8, addr: u32) -> u8 {
        if self.rom.is_empty() {
            return 0x90;
        }
        let bank = bank_select as u32;
        let offset = ((bank << 16) | (addr & 0xFFFF)) as usize % self.rom.len();
        self.rom[offset]
    }

    /// Read from the linear ROM range (0x40000–0xFFFFF) using `linear_off`.
    pub fn read_rom_ex(&self, addr: u32) -> u8 {
        if self.rom.is_empty() {
            return 0x90;
        }
        let hi = (self.linear_off as u32) << 20;
        let offset = (hi | (addr & 0xFFFFF)) as usize % self.rom.len();
        self.rom[offset]
    }
}
