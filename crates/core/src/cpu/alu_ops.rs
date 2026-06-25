use super::flags::parity_even;
use super::Cpu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AluOp {
    Add,
    Or,
    Adc,
    Sbb,
    And,
    Sub,
    Xor,
    Cmp,
}

/// Maps the `reg` field of a group-1 (0x80/0x81/0x83) ModRM byte to its ALU
/// operation, per the standard 8086 opcode table.
pub fn alu_op_from_reg_field(reg: u8) -> AluOp {
    match reg & 0b111 {
        0 => AluOp::Add,
        1 => AluOp::Or,
        2 => AluOp::Adc,
        3 => AluOp::Sbb,
        4 => AluOp::And,
        5 => AluOp::Sub,
        6 => AluOp::Xor,
        7 => AluOp::Cmp,
        _ => unreachable!(),
    }
}

impl Cpu {
    pub(super) fn alu_u8(&mut self, op: AluOp, a: u8, b: u8) -> u8 {
        match op {
            AluOp::Add => self.add_u8(a, b, 0),
            AluOp::Adc => self.add_u8(a, b, self.flags.carry as u8),
            AluOp::Or => self.logic_u8(a | b),
            AluOp::And => self.logic_u8(a & b),
            AluOp::Xor => self.logic_u8(a ^ b),
            AluOp::Sub | AluOp::Cmp => self.sub_u8(a, b, 0),
            AluOp::Sbb => self.sub_u8(a, b, self.flags.carry as u8),
        }
    }

    pub(super) fn alu_u16(&mut self, op: AluOp, a: u16, b: u16) -> u16 {
        match op {
            AluOp::Add => self.add_u16(a, b, 0),
            AluOp::Adc => self.add_u16(a, b, self.flags.carry as u16),
            AluOp::Or => self.logic_u16(a | b),
            AluOp::And => self.logic_u16(a & b),
            AluOp::Xor => self.logic_u16(a ^ b),
            AluOp::Sub | AluOp::Cmp => self.sub_u16(a, b, 0),
            AluOp::Sbb => self.sub_u16(a, b, self.flags.carry as u16),
        }
    }

    fn set_zsp8(&mut self, result: u8) {
        self.flags.zero = result == 0;
        self.flags.sign = result & 0x80 != 0;
        self.flags.parity = parity_even(result);
    }

    fn set_zsp16(&mut self, result: u16) {
        self.flags.zero = result == 0;
        self.flags.sign = result & 0x8000 != 0;
        self.flags.parity = parity_even(result as u8);
    }

    pub(super) fn add_u8(&mut self, a: u8, b: u8, carry_in: u8) -> u8 {
        let wide = a as u16 + b as u16 + carry_in as u16;
        let result = wide as u8;
        self.flags.carry = wide > 0xFF;
        self.flags.aux_carry = (a & 0xF) as u16 + (b & 0xF) as u16 + carry_in as u16 > 0xF;
        let signed = a as i8 as i16 + b as i8 as i16 + carry_in as i16;
        self.flags.overflow = !(i8::MIN as i16..=i8::MAX as i16).contains(&signed);
        self.set_zsp8(result);
        result
    }

    pub(super) fn add_u16(&mut self, a: u16, b: u16, carry_in: u16) -> u16 {
        let wide = a as u32 + b as u32 + carry_in as u32;
        let result = wide as u16;
        self.flags.carry = wide > 0xFFFF;
        self.flags.aux_carry = (a & 0xF) as u32 + (b & 0xF) as u32 + carry_in as u32 > 0xF;
        let signed = a as i16 as i32 + b as i16 as i32 + carry_in as i32;
        self.flags.overflow = !(i16::MIN as i32..=i16::MAX as i32).contains(&signed);
        self.set_zsp16(result);
        result
    }

    pub(super) fn sub_u8(&mut self, a: u8, b: u8, borrow_in: u8) -> u8 {
        let wide = a as i32 - b as i32 - borrow_in as i32;
        let result = wide as u8;
        self.flags.carry = wide < 0;
        self.flags.aux_carry = ((a & 0xF) as i32 - (b & 0xF) as i32 - borrow_in as i32) < 0;
        let signed = a as i8 as i32 - b as i8 as i32 - borrow_in as i32;
        self.flags.overflow = !(i8::MIN as i32..=i8::MAX as i32).contains(&signed);
        self.set_zsp8(result);
        result
    }

    pub(super) fn sub_u16(&mut self, a: u16, b: u16, borrow_in: u16) -> u16 {
        let wide = a as i32 - b as i32 - borrow_in as i32;
        let result = wide as u16;
        self.flags.carry = wide < 0;
        self.flags.aux_carry = ((a & 0xF) as i32 - (b & 0xF) as i32 - borrow_in as i32) < 0;
        let signed = a as i16 as i32 - b as i16 as i32 - borrow_in as i32;
        self.flags.overflow = !(i16::MIN as i32..=i16::MAX as i32).contains(&signed);
        self.set_zsp16(result);
        result
    }

    /// OR/AND/XOR always clear CF and OF; AF is left undefined by the 8086
    /// spec (this implementation leaves it unchanged — unverified against
    /// real V30MZ hardware, see docs/dev/DevelopmentPlan.md risk notes).
    fn logic_u8(&mut self, result: u8) -> u8 {
        self.flags.carry = false;
        self.flags.overflow = false;
        self.set_zsp8(result);
        result
    }

    fn logic_u16(&mut self, result: u16) -> u16 {
        self.flags.carry = false;
        self.flags.overflow = false;
        self.set_zsp16(result);
        result
    }

    /// INC/DEC affect OF/SF/ZF/AF/PF but, unlike ADD/SUB, leave CF untouched.
    pub(super) fn inc_u8(&mut self, a: u8) -> u8 {
        let result = a.wrapping_add(1);
        self.flags.aux_carry = (a & 0xF) == 0xF;
        self.flags.overflow = a == 0x7F;
        self.set_zsp8(result);
        result
    }

    pub(super) fn dec_u8(&mut self, a: u8) -> u8 {
        let result = a.wrapping_sub(1);
        self.flags.aux_carry = (a & 0xF) == 0x0;
        self.flags.overflow = a == 0x80;
        self.set_zsp8(result);
        result
    }

    pub(super) fn inc_u16(&mut self, a: u16) -> u16 {
        let result = a.wrapping_add(1);
        self.flags.aux_carry = (a & 0xF) == 0xF;
        self.flags.overflow = a == 0x7FFF;
        self.set_zsp16(result);
        result
    }

    pub(super) fn dec_u16(&mut self, a: u16) -> u16 {
        let result = a.wrapping_sub(1);
        self.flags.aux_carry = (a & 0xF) == 0x0;
        self.flags.overflow = a == 0x8000;
        self.set_zsp16(result);
        result
    }
}
