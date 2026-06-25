/// Abstraction over the 20-bit (1 MiB) WonderSwan address space.
///
/// Phase 2 (see docs/dev/DevelopmentPlan.md) replaces test-only flat-memory
/// implementations with the real memory map (RAM, I/O ports, cartridge ROM).
pub trait MemoryBus {
    fn read_u8(&self, addr: u32) -> u8;
    fn write_u8(&mut self, addr: u32, value: u8);

    fn read_u16(&self, addr: u32) -> u16 {
        let lo = self.read_u8(addr) as u16;
        let hi = self.read_u8(addr.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }

    fn write_u16(&mut self, addr: u32, value: u16) {
        self.write_u8(addr, value as u8);
        self.write_u8(addr.wrapping_add(1), (value >> 8) as u8);
    }
}

/// Resolves a real-mode segment:offset pair to a 20-bit physical address.
pub fn linear_address(segment: u16, offset: u16) -> u32 {
    (((segment as u32) << 4).wrapping_add(offset as u32)) & 0xF_FFFF
}
