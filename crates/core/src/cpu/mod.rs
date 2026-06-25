//! NEC V30MZ CPU core: registers, flags, ModRM decoding, and a representative
//! subset of the 8086/V30-compatible instruction set (see "Phase 1 — CPU実装"
//! in docs/dev/DevelopmentPlan.md for scope and the remaining opcode
//! coverage). Memory access goes through the `MemoryBus` trait so that
//! Phase 2 can swap in the real WonderSwan memory map without touching the
//! instruction decoder/executor.

mod alu_ops;
mod bus;
mod ctrl_ops;
mod decode;
mod flags;
mod registers;

#[cfg(test)]
mod tests;

pub use bus::MemoryBus;
pub use flags::Flags;
pub use registers::Registers;

use alu_ops::alu_op_from_reg_field;
use bus::linear_address;
use decode::{decode_modrm, RegMem};

/// CPU core state. Cycle costs returned by `step` are provisional
/// per-instruction approximations (see docs/dev/DevelopmentPlan.md "サイクル
/// 精度設計の考慮点"); they are not yet validated against real V30MZ timing
/// and will be refined once cycle-accurate reference data is available.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Cpu {
    pub regs: Registers,
    pub flags: Flags,
    pub halted: bool,
}

impl Cpu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self, cs: u16, ip: u16) {
        *self = Cpu::default();
        self.regs.cs = cs;
        self.regs.ip = ip;
    }

    fn fetch_u8<B: MemoryBus>(&mut self, bus: &mut B) -> u8 {
        let addr = linear_address(self.regs.cs, self.regs.ip);
        let value = bus.read_u8(addr);
        self.regs.ip = self.regs.ip.wrapping_add(1);
        value
    }

    fn fetch_u16<B: MemoryBus>(&mut self, bus: &mut B) -> u16 {
        let lo = self.fetch_u8(bus) as u16;
        let hi = self.fetch_u8(bus) as u16;
        lo | (hi << 8)
    }

    fn read_rm8<B: MemoryBus>(&self, bus: &B, rm: &RegMem) -> u8 {
        match *rm {
            RegMem::Reg(i) => self.regs.get_reg8(i),
            RegMem::Mem(addr) => bus.read_u8(addr),
        }
    }

    fn write_rm8<B: MemoryBus>(&mut self, bus: &mut B, rm: &RegMem, value: u8) {
        match *rm {
            RegMem::Reg(i) => self.regs.set_reg8(i, value),
            RegMem::Mem(addr) => bus.write_u8(addr, value),
        }
    }

    fn read_rm16<B: MemoryBus>(&self, bus: &B, rm: &RegMem) -> u16 {
        match *rm {
            RegMem::Reg(i) => self.regs.get_reg16(i),
            RegMem::Mem(addr) => bus.read_u16(addr),
        }
    }

    fn write_rm16<B: MemoryBus>(&mut self, bus: &mut B, rm: &RegMem, value: u16) {
        match *rm {
            RegMem::Reg(i) => self.regs.set_reg16(i, value),
            RegMem::Mem(addr) => bus.write_u16(addr, value),
        }
    }

    fn push16<B: MemoryBus>(&mut self, bus: &mut B, value: u16) {
        self.regs.sp = self.regs.sp.wrapping_sub(2);
        let addr = linear_address(self.regs.ss, self.regs.sp);
        bus.write_u16(addr, value);
    }

    fn pop16<B: MemoryBus>(&mut self, bus: &mut B) -> u16 {
        let addr = linear_address(self.regs.ss, self.regs.sp);
        let value = bus.read_u16(addr);
        self.regs.sp = self.regs.sp.wrapping_add(2);
        value
    }

    /// Placeholder cycle cost: register operands are cheaper than memory
    /// operands (extra cycles approximate effective-address calculation).
    /// See the module doc comment regarding provisional timing.
    fn cycles_for(rm: &RegMem, base: u32) -> u32 {
        match rm {
            RegMem::Reg(_) => base + 2,
            RegMem::Mem(_) => base + 7,
        }
    }

    /// Executes a single instruction and returns the number of clock cycles
    /// it consumed. Phase 1 models cycle cost per *instruction*; a future
    /// phase may decompose this into a true per-clock `step_cycle()` once
    /// PPU/APU/timer/DMA synchronization requires it.
    pub fn step<B: MemoryBus>(&mut self, bus: &mut B) -> u32 {
        if self.halted {
            return 1;
        }
        let opcode = self.fetch_u8(bus);
        self.execute(opcode, bus)
    }

    fn execute<B: MemoryBus>(&mut self, opcode: u8, bus: &mut B) -> u32 {
        // ADD/OR/ADC/SBB/AND/SUB/XOR/CMP, register/memory forms: opcodes
        // 0x00-0x3D laid out in 8-byte groups (offsets 6,7 are the
        // ES/CS/SS/DS PUSH/POP opcodes interspersed in this range, not yet
        // implemented).
        let base = opcode & 0xF8;
        let form = opcode & 0x07;
        if form <= 5 && matches!(base, 0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38) {
            let op = alu_op_from_reg_field((base >> 3) & 0b111);
            return self.exec_alu_form(bus, op, form);
        }

        match opcode {
            0x80 => {
                let m = decode_modrm(self, bus);
                let imm = self.fetch_u8(bus);
                let op = alu_op_from_reg_field(m.reg);
                let a = self.read_rm8(bus, &m.rm);
                let r = self.alu_u8(op, a, imm);
                if op != alu_ops::AluOp::Cmp {
                    self.write_rm8(bus, &m.rm, r);
                }
                Self::cycles_for(&m.rm, 4)
            }
            0x81 => {
                let m = decode_modrm(self, bus);
                let imm = self.fetch_u16(bus);
                let op = alu_op_from_reg_field(m.reg);
                let a = self.read_rm16(bus, &m.rm);
                let r = self.alu_u16(op, a, imm);
                if op != alu_ops::AluOp::Cmp {
                    self.write_rm16(bus, &m.rm, r);
                }
                Self::cycles_for(&m.rm, 4)
            }
            0x83 => {
                let m = decode_modrm(self, bus);
                let imm = self.fetch_u8(bus) as i8 as i16 as u16;
                let op = alu_op_from_reg_field(m.reg);
                let a = self.read_rm16(bus, &m.rm);
                let r = self.alu_u16(op, a, imm);
                if op != alu_ops::AluOp::Cmp {
                    self.write_rm16(bus, &m.rm, r);
                }
                Self::cycles_for(&m.rm, 4)
            }

            // MOV
            0x88 => {
                let m = decode_modrm(self, bus);
                let v = self.regs.get_reg8(m.reg);
                self.write_rm8(bus, &m.rm, v);
                Self::cycles_for(&m.rm, 2)
            }
            0x89 => {
                let m = decode_modrm(self, bus);
                let v = self.regs.get_reg16(m.reg);
                self.write_rm16(bus, &m.rm, v);
                Self::cycles_for(&m.rm, 2)
            }
            0x8A => {
                let m = decode_modrm(self, bus);
                let v = self.read_rm8(bus, &m.rm);
                self.regs.set_reg8(m.reg, v);
                Self::cycles_for(&m.rm, 2)
            }
            0x8B => {
                let m = decode_modrm(self, bus);
                let v = self.read_rm16(bus, &m.rm);
                self.regs.set_reg16(m.reg, v);
                Self::cycles_for(&m.rm, 2)
            }
            0xC6 => {
                let m = decode_modrm(self, bus);
                let imm = self.fetch_u8(bus);
                self.write_rm8(bus, &m.rm, imm);
                Self::cycles_for(&m.rm, 3)
            }
            0xC7 => {
                let m = decode_modrm(self, bus);
                let imm = self.fetch_u16(bus);
                self.write_rm16(bus, &m.rm, imm);
                Self::cycles_for(&m.rm, 3)
            }
            0xB0..=0xB7 => {
                let imm = self.fetch_u8(bus);
                self.regs.set_reg8(opcode & 0x07, imm);
                4
            }
            0xB8..=0xBF => {
                let imm = self.fetch_u16(bus);
                self.regs.set_reg16(opcode & 0x07, imm);
                4
            }

            // INC/DEC (register form; CF is left untouched per the 8086
            // spec, unlike ADD/SUB).
            0x40..=0x47 => {
                let i = opcode & 0x07;
                let v = self.inc_u16(self.regs.get_reg16(i));
                self.regs.set_reg16(i, v);
                2
            }
            0x48..=0x4F => {
                let i = opcode & 0x07;
                let v = self.dec_u16(self.regs.get_reg16(i));
                self.regs.set_reg16(i, v);
                2
            }

            // Stack
            0x50..=0x57 => {
                let v = self.regs.get_reg16(opcode & 0x07);
                self.push16(bus, v);
                4
            }
            0x58..=0x5F => {
                let v = self.pop16(bus);
                self.regs.set_reg16(opcode & 0x07, v);
                4
            }

            // Control flow
            0xE9 => {
                let rel = self.fetch_u16(bus) as i16;
                self.regs.ip = self.regs.ip.wrapping_add(rel as u16);
                11
            }
            0xEB => {
                let rel = self.fetch_u8(bus) as i8;
                self.regs.ip = self.regs.ip.wrapping_add(rel as u16);
                8
            }
            0x70..=0x7F => {
                let rel = self.fetch_u8(bus) as i8;
                if self.condition(opcode) {
                    self.regs.ip = self.regs.ip.wrapping_add(rel as u16);
                    16
                } else {
                    4
                }
            }
            0xE8 => {
                let rel = self.fetch_u16(bus) as i16;
                let return_ip = self.regs.ip;
                self.push16(bus, return_ip);
                self.regs.ip = self.regs.ip.wrapping_add(rel as u16);
                16
            }
            0xC3 => {
                self.regs.ip = self.pop16(bus);
                16
            }
            0xC2 => {
                let extra = self.fetch_u16(bus);
                self.regs.ip = self.pop16(bus);
                self.regs.sp = self.regs.sp.wrapping_add(extra);
                17
            }

            // Misc / flag instructions
            0x90 => 3,
            0xF4 => {
                self.halted = true;
                2
            }
            0xF5 => {
                self.flags.carry = !self.flags.carry;
                2
            }
            0xF8 => {
                self.flags.carry = false;
                2
            }
            0xF9 => {
                self.flags.carry = true;
                2
            }
            0xFA => {
                self.flags.interrupt = false;
                2
            }
            0xFB => {
                self.flags.interrupt = true;
                2
            }
            0xFC => {
                self.flags.direction = false;
                2
            }
            0xFD => {
                self.flags.direction = true;
                2
            }

            // INC/DEC r/m (group FE/FF, sub-forms 0/1 only; the remaining
            // FF sub-forms — CALL/JMP/PUSH r/m — are deferred to a later
            // phase).
            0xFE => {
                let m = decode_modrm(self, bus);
                let a = self.read_rm8(bus, &m.rm);
                let r = match m.reg & 0b111 {
                    0 => self.inc_u8(a),
                    1 => self.dec_u8(a),
                    other => unimplemented!("opcode 0xFE reg field {other} not yet implemented"),
                };
                self.write_rm8(bus, &m.rm, r);
                Self::cycles_for(&m.rm, 1)
            }
            0xFF => {
                let m = decode_modrm(self, bus);
                let a = self.read_rm16(bus, &m.rm);
                let r = match m.reg & 0b111 {
                    0 => self.inc_u16(a),
                    1 => self.dec_u16(a),
                    other => unimplemented!("opcode 0xFF reg field {other} not yet implemented"),
                };
                self.write_rm16(bus, &m.rm, r);
                Self::cycles_for(&m.rm, 1)
            }

            _ => unimplemented!(
                "opcode {:#04X} is not yet implemented (Phase 1 covers a representative subset; see docs/dev/DevelopmentPlan.md)",
                opcode
            ),
        }
    }

    fn exec_alu_form<B: MemoryBus>(&mut self, bus: &mut B, op: alu_ops::AluOp, form: u8) -> u32 {
        use alu_ops::AluOp;
        match form {
            0 => {
                let m = decode_modrm(self, bus);
                let a = self.read_rm8(bus, &m.rm);
                let b = self.regs.get_reg8(m.reg);
                let r = self.alu_u8(op, a, b);
                if op != AluOp::Cmp {
                    self.write_rm8(bus, &m.rm, r);
                }
                Self::cycles_for(&m.rm, 1)
            }
            1 => {
                let m = decode_modrm(self, bus);
                let a = self.read_rm16(bus, &m.rm);
                let b = self.regs.get_reg16(m.reg);
                let r = self.alu_u16(op, a, b);
                if op != AluOp::Cmp {
                    self.write_rm16(bus, &m.rm, r);
                }
                Self::cycles_for(&m.rm, 1)
            }
            2 => {
                let m = decode_modrm(self, bus);
                let a = self.regs.get_reg8(m.reg);
                let b = self.read_rm8(bus, &m.rm);
                let r = self.alu_u8(op, a, b);
                if op != AluOp::Cmp {
                    self.regs.set_reg8(m.reg, r);
                }
                Self::cycles_for(&m.rm, 1)
            }
            3 => {
                let m = decode_modrm(self, bus);
                let a = self.regs.get_reg16(m.reg);
                let b = self.read_rm16(bus, &m.rm);
                let r = self.alu_u16(op, a, b);
                if op != AluOp::Cmp {
                    self.regs.set_reg16(m.reg, r);
                }
                Self::cycles_for(&m.rm, 1)
            }
            4 => {
                let imm = self.fetch_u8(bus);
                let a = self.regs.get_reg8(0);
                let r = self.alu_u8(op, a, imm);
                if op != AluOp::Cmp {
                    self.regs.set_reg8(0, r);
                }
                4
            }
            5 => {
                let imm = self.fetch_u16(bus);
                let a = self.regs.ax;
                let r = self.alu_u16(op, a, imm);
                if op != AluOp::Cmp {
                    self.regs.ax = r;
                }
                4
            }
            _ => unreachable!(),
        }
    }
}
