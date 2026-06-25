use super::Cpu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftOp {
    Rol,
    Ror,
    Rcl,
    Rcr,
    Shl,
    Shr,
    Sar,
}

/// Maps the `reg` field of a shift/rotate group (0xD0-0xD3) ModRM byte to its
/// operation. Reg field 6 is the undocumented SAL alias of SHL on real 8086
/// hardware; we map it to `Shl` directly rather than modeling it separately.
pub fn shift_op_from_reg_field(reg: u8) -> ShiftOp {
    match reg & 0b111 {
        0 => ShiftOp::Rol,
        1 => ShiftOp::Ror,
        2 => ShiftOp::Rcl,
        3 => ShiftOp::Rcr,
        4 | 6 => ShiftOp::Shl,
        5 => ShiftOp::Shr,
        7 => ShiftOp::Sar,
        _ => unreachable!(),
    }
}

impl Cpu {
    /// Shifts/rotates an 8-bit value by `count` (already masked/resolved by
    /// the caller). `count == 0` is a no-op that leaves all flags untouched,
    /// per the 8086 spec. OF is only well-defined for count == 1; for the
    /// rotate forms (ROL/ROR/RCL/RCR) we leave OF unchanged as a documented
    /// approximation pending hardware verification (see "リスクと不確実性
    /// への対処方針" in docs/dev/DevelopmentPlan.md).
    pub(super) fn shift_u8(&mut self, op: ShiftOp, value: u8, count: u8) -> u8 {
        if count == 0 {
            return value;
        }
        let original_msb = value & 0x80 != 0;
        let mut result = value;
        for _ in 0..count {
            result = self.shift_step_u8(op, result);
        }
        if matches!(op, ShiftOp::Shl | ShiftOp::Shr | ShiftOp::Sar) {
            self.set_zsp8(result);
        }
        if count == 1 {
            self.flags.overflow = match op {
                ShiftOp::Shl => (result & 0x80 != 0) != self.flags.carry,
                ShiftOp::Shr => original_msb,
                ShiftOp::Sar => false,
                ShiftOp::Rol | ShiftOp::Ror | ShiftOp::Rcl | ShiftOp::Rcr => self.flags.overflow,
            };
        }
        result
    }

    pub(super) fn shift_u16(&mut self, op: ShiftOp, value: u16, count: u8) -> u16 {
        if count == 0 {
            return value;
        }
        let original_msb = value & 0x8000 != 0;
        let mut result = value;
        for _ in 0..count {
            result = self.shift_step_u16(op, result);
        }
        if matches!(op, ShiftOp::Shl | ShiftOp::Shr | ShiftOp::Sar) {
            self.set_zsp16(result);
        }
        if count == 1 {
            self.flags.overflow = match op {
                ShiftOp::Shl => (result & 0x8000 != 0) != self.flags.carry,
                ShiftOp::Shr => original_msb,
                ShiftOp::Sar => false,
                ShiftOp::Rol | ShiftOp::Ror | ShiftOp::Rcl | ShiftOp::Rcr => self.flags.overflow,
            };
        }
        result
    }

    fn shift_step_u8(&mut self, op: ShiftOp, v: u8) -> u8 {
        match op {
            ShiftOp::Rol => {
                let carry_out = v & 0x80 != 0;
                self.flags.carry = carry_out;
                (v << 1) | carry_out as u8
            }
            ShiftOp::Ror => {
                let carry_out = v & 1 != 0;
                self.flags.carry = carry_out;
                (v >> 1) | ((carry_out as u8) << 7)
            }
            ShiftOp::Rcl => {
                let carry_in = self.flags.carry as u8;
                let carry_out = v & 0x80 != 0;
                self.flags.carry = carry_out;
                (v << 1) | carry_in
            }
            ShiftOp::Rcr => {
                let carry_in = self.flags.carry as u8;
                let carry_out = v & 1 != 0;
                self.flags.carry = carry_out;
                (v >> 1) | (carry_in << 7)
            }
            ShiftOp::Shl => {
                self.flags.carry = v & 0x80 != 0;
                v << 1
            }
            ShiftOp::Shr => {
                self.flags.carry = v & 1 != 0;
                v >> 1
            }
            ShiftOp::Sar => {
                self.flags.carry = v & 1 != 0;
                ((v as i8) >> 1) as u8
            }
        }
    }

    fn shift_step_u16(&mut self, op: ShiftOp, v: u16) -> u16 {
        match op {
            ShiftOp::Rol => {
                let carry_out = v & 0x8000 != 0;
                self.flags.carry = carry_out;
                (v << 1) | carry_out as u16
            }
            ShiftOp::Ror => {
                let carry_out = v & 1 != 0;
                self.flags.carry = carry_out;
                (v >> 1) | ((carry_out as u16) << 15)
            }
            ShiftOp::Rcl => {
                let carry_in = self.flags.carry as u16;
                let carry_out = v & 0x8000 != 0;
                self.flags.carry = carry_out;
                (v << 1) | carry_in
            }
            ShiftOp::Rcr => {
                let carry_in = self.flags.carry as u16;
                let carry_out = v & 1 != 0;
                self.flags.carry = carry_out;
                (v >> 1) | (carry_in << 15)
            }
            ShiftOp::Shl => {
                self.flags.carry = v & 0x8000 != 0;
                v << 1
            }
            ShiftOp::Shr => {
                self.flags.carry = v & 1 != 0;
                v >> 1
            }
            ShiftOp::Sar => {
                self.flags.carry = v & 1 != 0;
                ((v as i16) >> 1) as u16
            }
        }
    }
}
