/// Abstraction over the 20-bit (1 MiB) WonderSwan address space and
/// the 8-bit I/O port space (0x00–0xFF).
///
/// Phase 2 (see docs/dev/DevelopmentPlan.md) provides the real WonderSwan
/// `Bus` implementation. The default `read_io`/`write_io` return open-bus
/// (0xFF) / no-op so that the Phase 1 flat-memory test stub requires no
/// changes.
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

    /// Read from an 8-bit I/O port. May have side effects on real hardware
    /// (e.g. INT_CAUSE clears edge-triggered bits on read). Default: open bus.
    fn read_io(&mut self, port: u8) -> u8 {
        let _ = port;
        0xFF
    }

    /// Write to an 8-bit I/O port. Default: no-op.
    fn write_io(&mut self, port: u8, value: u8) {
        let _ = (port, value);
    }

    /// Return and clear CPU-visible wait cycles caused by recent bus I/O.
    ///
    /// Most test buses have no wait state, so the default is zero. The hardware
    /// bus uses this for synchronous DMA bursts that stall the CPU during an
    /// otherwise ordinary OUT instruction.
    fn take_wait_cycles(&mut self) -> u32 {
        0
    }
}

/// Resolves a real-mode segment:offset pair to a 20-bit physical address.
pub fn linear_address(segment: u16, offset: u16) -> u32 {
    (((segment as u32) << 4).wrapping_add(offset as u32)) & 0xF_FFFF
}
