use super::Cpu;

impl Cpu {
    /// Evaluates a Jcc condition code (the low nibble of opcodes 0x70-0x7F),
    /// per the standard 8086 condition table.
    pub(super) fn condition(&self, cc: u8) -> bool {
        let f = &self.flags;
        match cc & 0xF {
            0x0 => f.overflow,
            0x1 => !f.overflow,
            0x2 => f.carry,
            0x3 => !f.carry,
            0x4 => f.zero,
            0x5 => !f.zero,
            0x6 => f.carry || f.zero,
            0x7 => !f.carry && !f.zero,
            0x8 => f.sign,
            0x9 => !f.sign,
            0xA => f.parity,
            0xB => !f.parity,
            0xC => f.sign != f.overflow,
            0xD => f.sign == f.overflow,
            0xE => (f.sign != f.overflow) || f.zero,
            0xF => (f.sign == f.overflow) && !f.zero,
            _ => unreachable!(),
        }
    }
}
