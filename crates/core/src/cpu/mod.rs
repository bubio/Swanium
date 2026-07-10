//! NEC V30MZ CPU core: registers, flags, ModRM decoding, and a representative
//! subset of the 8086/V30-compatible instruction set (see "Phase 1 — CPU実装"
//! in docs/dev/DevelopmentPlan.md for scope and the remaining opcode
//! coverage). Memory access goes through the `MemoryBus` trait so that
//! Phase 2 can swap in the real WonderSwan memory map without touching the
//! instruction decoder/executor.

mod alu_ops;
mod bit_ops;
mod bus;
mod ctrl_ops;
mod decode;
mod flags;
mod registers;
mod timing;

#[cfg(test)]
mod tests;

pub use bus::MemoryBus;
pub use flags::Flags;
pub use registers::Registers;

use alu_ops::alu_op_from_reg_field;
use bit_ops::shift_op_from_reg_field;
use bus::linear_address;
use decode::{decode_modrm, RegMem};

const LONG_REP_INTERRUPT_RETURN_CYCLE_THRESHOLD: u32 = 256;

/// CPU core state. Cycle costs returned by `step` are per-instruction clock
/// counts for the NEC V30MZ, sourced from the WonderSwan hardware reference
/// "Sacred Tech Scroll" (perfectkiosk.net/stsws) and cross-checked against the
/// µPD70116 (V30) datasheet — see the [`timing`] module. The V30 executes most
/// instructions in far fewer clocks than the 8086 (e.g. MOV = 1, IRET = 10),
/// which materially affects interrupt-heavy games; per-clock (prefetch-queue
/// exact) modelling remains a later-phase refinement (see
/// docs/dev/DevelopmentPlan.md "サイクル精度設計の考慮点").
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Cpu {
    pub regs: Registers,
    pub flags: Flags,
    pub halted: bool,
    pub fault: Option<CpuFault>,
    /// Set by a segment-override prefix opcode (0x26/2E/36/3E); used by
    /// the immediately following instruction's effective-address calculation,
    /// then cleared by `step`.
    pub seg_override: Option<u16>,
    /// Set by a REP/REPE/REPNE prefix (0xF3/0xF2); consumed by the
    /// immediately following string instruction, then cleared by `step`.
    pub rep_prefix: Option<u8>,
    /// Maskable IRQ delivery is inhibited for this many upcoming instruction
    /// boundaries after STI/POPF/IRET enabling IF and after POP/MOV SS.
    pub interrupt_inhibit: u8,
    /// Trap delivery is inhibited for this many upcoming instruction
    /// boundaries after POPF/IRET enabling TF.
    pub trap_inhibit: u8,
    /// IP of the first prefix/opcode byte for the instruction currently being
    /// executed. This is needed for REP-string IRQ restart semantics.
    instruction_start_ip: u16,
    /// Saved-IP override for the next hardware IRQ after a long REP string
    /// instruction crosses a scanline boundary in the scanline-framed driver.
    interrupt_return_override_ip: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuFault {
    pub opcode: u8,
    pub cs: u16,
    pub ip: u16,
}

impl Cpu {
    /// Returns a CPU in its power-on state (all registers zero, not halted).
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets all CPU state and positions the instruction pointer at `cs:ip`.
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

    fn fault_unsupported(&mut self, opcode: u8) -> u32 {
        self.fault = Some(CpuFault {
            opcode,
            cs: self.regs.cs,
            ip: self.regs.ip.wrapping_sub(1),
        });
        self.halted = true;
        1
    }

    /// V30MZ clock cost for a ModRM instruction, choosing the register- or
    /// memory-operand count (see [`timing::rm`] and the [`timing`] module).
    fn cycles_for(rm: &RegMem, reg: u32, mem: u32) -> u32 {
        timing::rm(rm, reg, mem)
    }

    /// Dispatch a hardware (or software-triggered) interrupt with `vector`.
    ///
    /// Saves FLAGS / CS / IP to the stack, clears IF and TF, then jumps to
    /// the far pointer stored in the interrupt vector table at physical
    /// address `vector * 4` (standard 8086 IVT layout in WRAM).
    ///
    /// The caller is responsible for checking `self.flags.interrupt` before
    /// calling this for maskable hardware IRQs.  This method always executes
    /// (used for NMI, software INT, and divide-by-zero as well).
    /// Returns the acknowledge/dispatch clock cost ([`timing::IRQ_ACK`]). For a
    /// hardware IRQ the caller adds this to its cycle budget; for a software
    /// `INT`/`INTO`/exception the wrapping opcode already reports the full cost,
    /// so it ignores this return value.
    pub fn handle_irq<B: MemoryBus>(&mut self, bus: &mut B, vector: u8) -> u32 {
        self.handle_irq_with_saved_ip(bus, vector, self.regs.ip)
    }

    /// Dispatch an interrupt while saving `ip` as the interrupted IP.
    pub fn handle_irq_at_ip<B: MemoryBus>(&mut self, bus: &mut B, vector: u8, ip: u16) -> u32 {
        self.handle_irq_with_saved_ip(bus, vector, ip)
    }

    fn handle_irq_with_saved_ip<B: MemoryBus>(&mut self, bus: &mut B, vector: u8, ip: u16) -> u32 {
        let flags = self.flags.to_u16();
        self.push16(bus, flags);
        let cs = self.regs.cs;
        self.push16(bus, cs);
        self.push16(bus, ip);
        self.flags.interrupt = false;
        self.flags.trap = false;
        let vec_addr = (vector as u32) * 4;
        self.regs.ip = bus.read_u16(vec_addr);
        self.regs.cs = bus.read_u16(vec_addr + 2);
        self.halted = false;
        timing::IRQ_ACK
    }

    /// Consume the pending saved-IP override produced by a long REP string
    /// instruction, if any.
    pub fn take_interrupt_return_override_ip(&mut self) -> Option<u16> {
        self.interrupt_return_override_ip.take()
    }

    /// Return the pending saved-IP override without consuming it.
    pub fn interrupt_return_override_ip(&self) -> Option<u16> {
        self.interrupt_return_override_ip
    }

    /// Executes a single instruction and returns the number of clock cycles
    /// it consumed. Phase 1 models cycle cost per *instruction*; a future
    /// phase may decompose this into a true per-clock `step_cycle()` once
    /// PPU/APU/timer/DMA synchronization requires it.
    pub fn step<B: MemoryBus>(&mut self, bus: &mut B) -> u32 {
        if self.halted {
            return 1;
        }
        self.interrupt_return_override_ip = None;
        self.instruction_start_ip = self.regs.ip;
        let opcode = self.fetch_u8(bus);
        let cycles = self.execute(opcode, bus);
        // Prefix fields are consumed within the instruction; clear any residual
        // state so a stale override cannot bleed into the next instruction.
        self.seg_override = None;
        self.rep_prefix = None;
        cycles
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
            0x80 | 0x82 => {
                let m = decode_modrm(self, bus);
                let imm = self.fetch_u8(bus);
                let op = alu_op_from_reg_field(m.reg);
                let a = self.read_rm8(bus, &m.rm);
                let r = self.alu_u8(op, a, imm);
                if op != alu_ops::AluOp::Cmp {
                    self.write_rm8(bus, &m.rm, r);
                }
                Self::cycles_for(&m.rm, 1, if op == alu_ops::AluOp::Cmp { 2 } else { 3 })
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
                Self::cycles_for(&m.rm, 1, if op == alu_ops::AluOp::Cmp { 2 } else { 3 })
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
                Self::cycles_for(&m.rm, 1, if op == alu_ops::AluOp::Cmp { 2 } else { 3 })
            }

            // MOV
            0x88 => {
                let m = decode_modrm(self, bus);
                let v = self.regs.get_reg8(m.reg);
                self.write_rm8(bus, &m.rm, v);
                Self::cycles_for(&m.rm, 1, 1)
            }
            0x89 => {
                let m = decode_modrm(self, bus);
                let v = self.regs.get_reg16(m.reg);
                self.write_rm16(bus, &m.rm, v);
                Self::cycles_for(&m.rm, 1, 1)
            }
            0x8A => {
                let m = decode_modrm(self, bus);
                let v = self.read_rm8(bus, &m.rm);
                self.regs.set_reg8(m.reg, v);
                Self::cycles_for(&m.rm, 1, 1)
            }
            0x8B => {
                let m = decode_modrm(self, bus);
                let v = self.read_rm16(bus, &m.rm);
                self.regs.set_reg16(m.reg, v);
                Self::cycles_for(&m.rm, 1, 1)
            }
            0xC6 => {
                let m = decode_modrm(self, bus);
                let imm = self.fetch_u8(bus);
                self.write_rm8(bus, &m.rm, imm);
                Self::cycles_for(&m.rm, 1, 1)
            }
            0xC7 => {
                let m = decode_modrm(self, bus);
                let imm = self.fetch_u16(bus);
                self.write_rm16(bus, &m.rm, imm);
                Self::cycles_for(&m.rm, 1, 1)
            }
            0xB0..=0xB7 => {
                let imm = self.fetch_u8(bus);
                self.regs.set_reg8(opcode & 0x07, imm);
                1
            }
            0xB8..=0xBF => {
                let imm = self.fetch_u16(bus);
                self.regs.set_reg16(opcode & 0x07, imm);
                1
            }

            // INC/DEC (register form; CF is left untouched per the 8086
            // spec, unlike ADD/SUB).
            0x40..=0x47 => {
                let i = opcode & 0x07;
                let v = self.inc_u16(self.regs.get_reg16(i));
                self.regs.set_reg16(i, v);
                1
            }
            0x48..=0x4F => {
                let i = opcode & 0x07;
                let v = self.dec_u16(self.regs.get_reg16(i));
                self.regs.set_reg16(i, v);
                1
            }

            // Stack
            0x50..=0x57 => {
                let v = self.regs.get_reg16(opcode & 0x07);
                self.push16(bus, v);
                1
            }
            0x58..=0x5F => {
                let v = self.pop16(bus);
                self.regs.set_reg16(opcode & 0x07, v);
                1
            }

            // Control flow
            0xE9 => {
                let rel = self.fetch_u16(bus) as i16;
                self.regs.ip = self.regs.ip.wrapping_add(rel as u16);
                4
            }
            0xEB => {
                let rel = self.fetch_u8(bus) as i8;
                self.regs.ip = self.regs.ip.wrapping_add(rel as u16);
                4
            }
            0x70..=0x7F => {
                let rel = self.fetch_u8(bus) as i8;
                if self.condition(opcode) {
                    let target = self.regs.ip.wrapping_add(rel as u16);
                    self.regs.ip = target;
                    5 + u32::from(target & 1 != 0)
                } else {
                    1
                }
            }
            0xE8 => {
                let rel = self.fetch_u16(bus) as i16;
                let return_ip = self.regs.ip;
                self.push16(bus, return_ip);
                self.regs.ip = self.regs.ip.wrapping_add(rel as u16);
                5
            }
            0xC3 => {
                self.regs.ip = self.pop16(bus);
                6
            }
            0xC2 => {
                let extra = self.fetch_u16(bus);
                self.regs.ip = self.pop16(bus);
                self.regs.sp = self.regs.sp.wrapping_add(extra);
                6
            }
            0xE0 => {
                // LOOPNE/LOOPNZ
                let rel = self.fetch_u8(bus) as i8;
                self.regs.cx = self.regs.cx.wrapping_sub(1);
                if self.regs.cx != 0 && !self.flags.zero {
                    self.regs.ip = self.regs.ip.wrapping_add(rel as u16);
                    7
                } else {
                    3
                }
            }
            0xE1 => {
                // LOOPE/LOOPZ
                let rel = self.fetch_u8(bus) as i8;
                self.regs.cx = self.regs.cx.wrapping_sub(1);
                if self.regs.cx != 0 && self.flags.zero {
                    self.regs.ip = self.regs.ip.wrapping_add(rel as u16);
                    7
                } else {
                    3
                }
            }
            0xE2 => {
                // LOOP
                let rel = self.fetch_u8(bus) as i8;
                self.regs.cx = self.regs.cx.wrapping_sub(1);
                if self.regs.cx != 0 {
                    self.regs.ip = self.regs.ip.wrapping_add(rel as u16);
                    6
                } else {
                    2
                }
            }
            0xE3 => {
                // JCXZ
                let rel = self.fetch_u8(bus) as i8;
                if self.regs.cx == 0 {
                    self.regs.ip = self.regs.ip.wrapping_add(rel as u16);
                    4
                } else {
                    1
                }
            }

            // XCHG
            0x86 => {
                let m = decode_modrm(self, bus);
                let a = self.read_rm8(bus, &m.rm);
                let b = self.regs.get_reg8(m.reg);
                self.write_rm8(bus, &m.rm, b);
                self.regs.set_reg8(m.reg, a);
                Self::cycles_for(&m.rm, 3, 5)
            }
            0x87 => {
                let m = decode_modrm(self, bus);
                let a = self.read_rm16(bus, &m.rm);
                let b = self.regs.get_reg16(m.reg);
                self.write_rm16(bus, &m.rm, b);
                self.regs.set_reg16(m.reg, a);
                Self::cycles_for(&m.rm, 3, 5)
            }
            0x91..=0x97 => {
                let i = opcode & 0x07;
                let a = self.regs.ax;
                let b = self.regs.get_reg16(i);
                self.regs.ax = b;
                self.regs.set_reg16(i, a);
                3
            }

            // TEST
            0x84 => {
                let m = decode_modrm(self, bus);
                let a = self.read_rm8(bus, &m.rm);
                let b = self.regs.get_reg8(m.reg);
                self.test_u8(a, b);
                Self::cycles_for(&m.rm, 1, 2)
            }
            0x85 => {
                let m = decode_modrm(self, bus);
                let a = self.read_rm16(bus, &m.rm);
                let b = self.regs.get_reg16(m.reg);
                self.test_u16(a, b);
                Self::cycles_for(&m.rm, 1, 2)
            }
            0xA8 => {
                let imm = self.fetch_u8(bus);
                let a = self.regs.get_reg8(0);
                self.test_u8(a, imm);
                1
            }
            0xA9 => {
                let imm = self.fetch_u16(bus);
                self.test_u16(self.regs.ax, imm);
                1
            }

            // Sign extension / flags transfer
            0x98 => {
                // CBW
                self.regs.ax = (self.regs.ax as u8 as i8 as i16) as u16;
                1
            }
            0x99 => {
                // CWD
                self.regs.dx = if self.regs.ax & 0x8000 != 0 {
                    0xFFFF
                } else {
                    0
                };
                1
            }
            0x9C => {
                // PUSHF
                let v = self.flags.to_u16();
                self.push16(bus, v);
                2
            }
            0x9B => {
                // WAIT/FWAIT. No external coprocessor is modelled.
                3
            }
            0x9D => {
                // POPF
                let v = self.pop16(bus);
                let old_interrupt = self.flags.interrupt;
                let old_trap = self.flags.trap;
                self.flags = Flags::from_u16(v);
                if !old_interrupt && self.flags.interrupt {
                    self.interrupt_inhibit = 1;
                }
                if !old_trap && self.flags.trap {
                    self.trap_inhibit = 1;
                }
                3
            }
            0x9E => {
                // SAHF
                let ah = self.regs.get_reg8(4);
                let high = self.flags.to_u16() & 0xFF00;
                self.flags = Flags::from_u16(high | ah as u16);
                4
            }
            0x9F => {
                // LAHF
                self.regs.set_reg8(4, (self.flags.to_u16() & 0xFF) as u8);
                2
            }

            // XLAT: AL = [DS:BX+AL]
            0xD7 => {
                let offset = self.regs.bx.wrapping_add(self.regs.get_reg8(0) as u16);
                let addr = linear_address(self.regs.ds, offset);
                let v = bus.read_u8(addr);
                self.regs.set_reg8(0, v);
                5
            }

            // Shift/rotate group
            0xD0 => {
                let m = decode_modrm(self, bus);
                let op = shift_op_from_reg_field(m.reg);
                let a = self.read_rm8(bus, &m.rm);
                let r = self.shift_u8(op, a, 1);
                self.write_rm8(bus, &m.rm, r);
                Self::cycles_for(&m.rm, 1, 3)
            }
            0xD1 => {
                let m = decode_modrm(self, bus);
                let op = shift_op_from_reg_field(m.reg);
                let a = self.read_rm16(bus, &m.rm);
                let r = self.shift_u16(op, a, 1);
                self.write_rm16(bus, &m.rm, r);
                Self::cycles_for(&m.rm, 1, 3)
            }
            0xD2 => {
                let m = decode_modrm(self, bus);
                let op = shift_op_from_reg_field(m.reg);
                let count = self.regs.get_reg8(1);
                let a = self.read_rm8(bus, &m.rm);
                let r = self.shift_u8(op, a, count);
                self.write_rm8(bus, &m.rm, r);
                Self::cycles_for(&m.rm, 3, 5)
            }
            0xD3 => {
                let m = decode_modrm(self, bus);
                let op = shift_op_from_reg_field(m.reg);
                let count = self.regs.get_reg8(1);
                let a = self.read_rm16(bus, &m.rm);
                let r = self.shift_u16(op, a, count);
                self.write_rm16(bus, &m.rm, r);
                Self::cycles_for(&m.rm, 3, 5)
            }

            // Group F6/F7: TEST/NOT/NEG/MUL/IMUL/DIV/IDIV
            0xF6 => {
                let m = decode_modrm(self, bus);
                match m.reg & 0b111 {
                    0 => {
                        let imm = self.fetch_u8(bus);
                        let a = self.read_rm8(bus, &m.rm);
                        self.test_u8(a, imm);
                        Self::cycles_for(&m.rm, 1, 2)
                    }
                    1 => {
                        let _ = self.fetch_u8(bus);
                        1
                    }
                    2 => {
                        let a = self.read_rm8(bus, &m.rm);
                        self.write_rm8(bus, &m.rm, !a);
                        Self::cycles_for(&m.rm, 1, 3)
                    }
                    3 => {
                        let a = self.read_rm8(bus, &m.rm);
                        let r = self.sub_u8(0, a, 0);
                        self.write_rm8(bus, &m.rm, r);
                        Self::cycles_for(&m.rm, 1, 3)
                    }
                    4 => {
                        let a = self.read_rm8(bus, &m.rm);
                        let al = self.regs.get_reg8(0);
                        self.regs.ax = self.mul_u8(al, a);
                        Self::cycles_for(&m.rm, 3, 4)
                    }
                    5 => {
                        let a = self.read_rm8(bus, &m.rm);
                        let al = self.regs.get_reg8(0);
                        self.regs.ax = self.imul_u8(al, a);
                        Self::cycles_for(&m.rm, 3, 4)
                    }
                    6 => {
                        let divisor = self.read_rm8(bus, &m.rm);
                        let dividend = self.regs.ax;
                        let cycles = Self::cycles_for(&m.rm, 15, 24);
                        let Some((quotient, remainder)) = Cpu::div_u8(dividend, divisor) else {
                            self.handle_irq(bus, 0); // INT 0: divide overflow
                            return cycles;
                        };
                        self.regs.set_reg8(0, quotient);
                        self.regs.set_reg8(4, remainder);
                        cycles
                    }
                    7 => {
                        let divisor = self.read_rm8(bus, &m.rm) as i8;
                        let dividend = self.regs.ax as i16;
                        let cycles = Self::cycles_for(&m.rm, 17, 25);
                        let Some((quotient, remainder)) = Cpu::idiv_u8(dividend, divisor) else {
                            self.handle_irq(bus, 0); // INT 0: divide overflow
                            return cycles;
                        };
                        self.regs.set_reg8(0, quotient as u8);
                        self.regs.set_reg8(4, remainder as u8);
                        cycles
                    }
                    _ => unreachable!(),
                }
            }
            0xF7 => {
                let m = decode_modrm(self, bus);
                match m.reg & 0b111 {
                    0 => {
                        let imm = self.fetch_u16(bus);
                        let a = self.read_rm16(bus, &m.rm);
                        self.test_u16(a, imm);
                        Self::cycles_for(&m.rm, 1, 2)
                    }
                    1 => {
                        let _ = self.fetch_u16(bus);
                        1
                    }
                    2 => {
                        let a = self.read_rm16(bus, &m.rm);
                        self.write_rm16(bus, &m.rm, !a);
                        Self::cycles_for(&m.rm, 1, 3)
                    }
                    3 => {
                        let a = self.read_rm16(bus, &m.rm);
                        let r = self.sub_u16(0, a, 0);
                        self.write_rm16(bus, &m.rm, r);
                        Self::cycles_for(&m.rm, 1, 3)
                    }
                    4 => {
                        let a = self.read_rm16(bus, &m.rm);
                        let ax = self.regs.ax;
                        let product = self.mul_u16(ax, a);
                        self.regs.ax = product as u16;
                        self.regs.dx = (product >> 16) as u16;
                        Self::cycles_for(&m.rm, 3, 4)
                    }
                    5 => {
                        let a = self.read_rm16(bus, &m.rm);
                        let ax = self.regs.ax;
                        let product = self.imul_u16(ax, a);
                        self.regs.ax = product as u16;
                        self.regs.dx = (product >> 16) as u16;
                        Self::cycles_for(&m.rm, 3, 4)
                    }
                    6 => {
                        let divisor = self.read_rm16(bus, &m.rm);
                        let dividend = ((self.regs.dx as u32) << 16) | self.regs.ax as u32;
                        let cycles = Self::cycles_for(&m.rm, 15, 24);
                        let Some((quotient, remainder)) = Cpu::div_u16(dividend, divisor) else {
                            self.handle_irq(bus, 0); // INT 0: divide overflow
                            return cycles;
                        };
                        self.regs.ax = quotient;
                        self.regs.dx = remainder;
                        cycles
                    }
                    7 => {
                        let divisor = self.read_rm16(bus, &m.rm) as i16;
                        let dividend = (((self.regs.dx as u32) << 16) | self.regs.ax as u32) as i32;
                        let cycles = Self::cycles_for(&m.rm, 17, 25);
                        let Some((quotient, remainder)) = Cpu::idiv_u16(dividend, divisor) else {
                            self.handle_irq(bus, 0); // INT 0: divide overflow
                            return cycles;
                        };
                        self.regs.ax = quotient as u16;
                        self.regs.dx = remainder as u16;
                        cycles
                    }
                    _ => unreachable!(),
                }
            }

            // Misc / flag instructions
            0x90 => 1,
            0xF4 => {
                self.halted = true;
                9
            }
            0xF5 => {
                self.flags.carry = !self.flags.carry;
                4
            }
            0xF8 => {
                self.flags.carry = false;
                4
            }
            0xF9 => {
                self.flags.carry = true;
                4
            }
            0xFA => {
                self.flags.interrupt = false;
                4
            }
            0xFB => {
                let was_enabled = self.flags.interrupt;
                self.flags.interrupt = true;
                if !was_enabled {
                    self.interrupt_inhibit = 1;
                }
                4
            }
            0xFC => {
                self.flags.direction = false;
                4
            }
            0xFD => {
                self.flags.direction = true;
                4
            }

            // ── Segment-override prefixes ─────────────────────────────────
            // Set seg_override, then decode and execute the next opcode so
            // that decode_modrm (and string instructions) use the override.
            0x26 => {
                self.seg_override = Some(self.regs.es);
                let op = self.fetch_u8(bus);
                self.execute(op, bus)
            }
            0x2E => {
                self.seg_override = Some(self.regs.cs);
                let op = self.fetch_u8(bus);
                self.execute(op, bus)
            }
            0x36 => {
                self.seg_override = Some(self.regs.ss);
                let op = self.fetch_u8(bus);
                self.execute(op, bus)
            }
            0x3E => {
                self.seg_override = Some(self.regs.ds);
                let op = self.fetch_u8(bus);
                self.execute(op, bus)
            }

            // ── REP / REPNE prefixes ──────────────────────────────────────
            0xF2 | 0xF3 => {
                self.rep_prefix = Some(opcode);
                let op = self.fetch_u8(bus);
                self.execute(op, bus)
            }

            // ── Segment register PUSH / POP ───────────────────────────────
            0x06 => {
                let v = self.regs.es;
                self.push16(bus, v);
                2
            }
            0x07 => {
                self.regs.es = self.pop16(bus);
                3
            }
            0x0E => {
                let v = self.regs.cs;
                self.push16(bus, v);
                2
            }
            0x0F => 1,
            0x16 => {
                let v = self.regs.ss;
                self.push16(bus, v);
                2
            }
            0x17 => {
                self.regs.ss = self.pop16(bus);
                self.interrupt_inhibit = 1;
                3
            }
            0x1E => {
                let v = self.regs.ds;
                self.push16(bus, v);
                2
            }
            0x1F => {
                self.regs.ds = self.pop16(bus);
                3
            }

            // ── BCD adjustment ────────────────────────────────────────────
            // V30MZ vs 8086 differences in AAA/AAS/AAM/AAD are a documented
            // risk area; see "リスクと不確実性への対処方針" in
            // docs/dev/DevelopmentPlan.md. The 8086 behaviour is implemented
            // here as a starting point and will be corrected when hardware
            // test results become available.
            0x27 => {
                // DAA: Decimal Adjust AL after Addition
                let al = self.regs.ax as u8;
                let mut result = al;
                let mut cf = false;
                if (al & 0xF) > 9 || self.flags.aux_carry {
                    result = result.wrapping_add(6);
                    self.flags.aux_carry = true;
                } else {
                    self.flags.aux_carry = false;
                }
                if al > 0x99 || self.flags.carry {
                    result = result.wrapping_add(0x60);
                    cf = true;
                }
                self.flags.carry = cf;
                self.regs.ax = (self.regs.ax & 0xFF00) | result as u16;
                self.set_zsp8(result);
                10
            }
            0x2F => {
                // DAS: Decimal Adjust AL after Subtraction
                let al = self.regs.ax as u8;
                let mut result = al;
                let mut cf = false;
                if (al & 0xF) > 9 || self.flags.aux_carry {
                    result = result.wrapping_sub(6);
                    self.flags.aux_carry = true;
                } else {
                    self.flags.aux_carry = false;
                }
                if al > 0x99 || self.flags.carry {
                    result = result.wrapping_sub(0x60);
                    cf = true;
                }
                self.flags.carry = cf;
                self.regs.ax = (self.regs.ax & 0xFF00) | result as u16;
                self.set_zsp8(result);
                10
            }
            0x37 => {
                // AAA: ASCII Adjust after Addition
                let al = self.regs.ax as u8;
                let ah = (self.regs.ax >> 8) as u8;
                if (al & 0xF) > 9 || self.flags.aux_carry {
                    let new_al = al.wrapping_add(6) & 0x0F;
                    let new_ah = ah.wrapping_add(1);
                    self.regs.ax = (new_ah as u16) << 8 | new_al as u16;
                    self.flags.aux_carry = true;
                    self.flags.carry = true;
                } else {
                    self.regs.ax = (ah as u16) << 8 | (al as u16 & 0x0F);
                    self.flags.aux_carry = false;
                    self.flags.carry = false;
                }
                9
            }
            0x3F => {
                // AAS: ASCII Adjust after Subtraction
                let al = self.regs.ax as u8;
                let ah = (self.regs.ax >> 8) as u8;
                if (al & 0xF) > 9 || self.flags.aux_carry {
                    let new_al = al.wrapping_sub(6) & 0x0F;
                    let new_ah = ah.wrapping_sub(1);
                    self.regs.ax = (new_ah as u16) << 8 | new_al as u16;
                    self.flags.aux_carry = true;
                    self.flags.carry = true;
                } else {
                    self.regs.ax = (ah as u16) << 8 | (al as u16 & 0x0F);
                    self.flags.aux_carry = false;
                    self.flags.carry = false;
                }
                9
            }
            0xD4 => {
                // AAM imm8: AH = AL / imm8; AL = AL mod imm8
                let base = self.fetch_u8(bus);
                if base == 0 {
                    self.handle_irq(bus, 0); // INT 0: divide overflow
                    return 17;
                }
                let al = self.regs.ax as u8;
                let new_ah = al / base;
                let new_al = al % base;
                self.regs.ax = (new_ah as u16) << 8 | new_al as u16;
                self.set_zsp8(new_al);
                17
            }
            0xD5 => {
                // AAD imm8: AL = AH * imm8 + AL; AH = 0
                let base = self.fetch_u8(bus);
                let al = self.regs.ax as u8;
                let ah = (self.regs.ax >> 8) as u8;
                let result = ah.wrapping_mul(base).wrapping_add(al);
                self.regs.ax = result as u16;
                self.set_zsp8(result);
                6
            }
            0xD6 => {
                // SALC/SETALC: undocumented on 8086, but harmless and useful
                // for compatibility. AL becomes 0xFF when CF=1, else 0x00.
                self.regs
                    .set_reg8(0, if self.flags.carry { 0xFF } else { 0 });
                3
            }

            // ── MOV: memory direct (0xA0–0xA3) ───────────────────────────
            0xA0 => {
                let offset = self.fetch_u16(bus);
                let seg = self.seg_override.unwrap_or(self.regs.ds);
                let v = bus.read_u8(linear_address(seg, offset));
                self.regs.set_reg8(0, v);
                1
            }
            0xA1 => {
                let offset = self.fetch_u16(bus);
                let seg = self.seg_override.unwrap_or(self.regs.ds);
                let v = bus.read_u16(linear_address(seg, offset));
                self.regs.ax = v;
                1
            }
            0xA2 => {
                let offset = self.fetch_u16(bus);
                let seg = self.seg_override.unwrap_or(self.regs.ds);
                let v = self.regs.get_reg8(0);
                bus.write_u8(linear_address(seg, offset), v);
                1
            }
            0xA3 => {
                let offset = self.fetch_u16(bus);
                let seg = self.seg_override.unwrap_or(self.regs.ds);
                let v = self.regs.ax;
                bus.write_u16(linear_address(seg, offset), v);
                1
            }

            // ── Segment register MOV ──────────────────────────────────────
            0x8C => {
                // MOV r/m16, Sreg
                let m = decode_modrm(self, bus);
                let v = self.regs.get_sreg(m.reg);
                self.write_rm16(bus, &m.rm, v);
                Self::cycles_for(&m.rm, 1, 3)
            }
            0x8E => {
                // MOV Sreg, r/m16
                let m = decode_modrm(self, bus);
                let v = self.read_rm16(bus, &m.rm);
                self.regs.set_sreg(m.reg, v);
                if m.reg == 2 {
                    self.interrupt_inhibit = 1;
                }
                Self::cycles_for(&m.rm, 1, 3)
            }

            // ── LEA ───────────────────────────────────────────────────────
            0x8D => {
                // LEA reg16, m: load 16-bit effective address (no memory
                // access, no segment applied). decode_modrm applies the
                // segment and returns a 20-bit physical address, which is
                // wrong for LEA — use the dedicated lea_decode helper instead.
                let (reg, ea) = self.lea_decode(bus);
                self.regs.set_reg16(reg, ea);
                1
            }

            // ── LES / LDS ─────────────────────────────────────────────────
            0xC4 => {
                // LES reg16, m32
                let m = decode_modrm(self, bus);
                let addr = match m.rm {
                    RegMem::Mem(a) => a,
                    RegMem::Reg(rm) => self.ws_reg_mode_effective_address(rm),
                };
                let off = bus.read_u16(addr);
                let seg = bus.read_u16(addr.wrapping_add(2));
                self.regs.set_reg16(m.reg, off);
                self.regs.es = seg;
                6
            }
            0xC5 => {
                // LDS reg16, m32
                let m = decode_modrm(self, bus);
                let addr = match m.rm {
                    RegMem::Mem(a) => a,
                    RegMem::Reg(rm) => self.ws_reg_mode_effective_address(rm),
                };
                let off = bus.read_u16(addr);
                let seg = bus.read_u16(addr.wrapping_add(2));
                self.regs.set_reg16(m.reg, off);
                self.regs.ds = seg;
                6
            }

            // ── ENTER / LEAVE ─────────────────────────────────────────────
            0xC8 => {
                // ENTER size, level.
                let size = self.fetch_u16(bus);
                let level = self.fetch_u8(bus) & 0x1F;
                let old_bp = self.regs.bp;
                self.push16(bus, old_bp);
                let frame_temp = self.regs.sp;
                if level > 0 {
                    for _ in 1..level {
                        self.regs.bp = self.regs.bp.wrapping_sub(2);
                        let addr = linear_address(self.regs.ss, self.regs.bp);
                        let v = bus.read_u16(addr);
                        self.push16(bus, v);
                    }
                    self.push16(bus, frame_temp);
                }
                self.regs.bp = frame_temp;
                self.regs.sp = self.regs.sp.wrapping_sub(size);
                8
            }
            0xC9 => {
                // LEAVE: SP = BP; POP BP
                self.regs.sp = self.regs.bp;
                self.regs.bp = self.pop16(bus);
                2
            }

            // ── Far CALL / JMP / RET ──────────────────────────────────────
            0x9A => {
                // CALL far ptr16:16 — push CS:IP, jump to new_cs:new_ip
                let new_ip = self.fetch_u16(bus);
                let new_cs = self.fetch_u16(bus);
                let ret_cs = self.regs.cs;
                let ret_ip = self.regs.ip;
                self.push16(bus, ret_cs);
                self.push16(bus, ret_ip);
                self.regs.ip = new_ip;
                self.regs.cs = new_cs;
                10
            }
            0xCA => {
                // RET far imm16
                let extra = self.fetch_u16(bus);
                self.regs.ip = self.pop16(bus);
                self.regs.cs = self.pop16(bus);
                self.regs.sp = self.regs.sp.wrapping_add(extra);
                9
            }
            0xCB => {
                // RET far
                self.regs.ip = self.pop16(bus);
                self.regs.cs = self.pop16(bus);
                8
            }
            0xEA => {
                // JMP far ptr16:16
                let new_ip = self.fetch_u16(bus);
                let new_cs = self.fetch_u16(bus);
                self.regs.ip = new_ip;
                self.regs.cs = new_cs;
                7
            }

            // ── String instructions (MOVS/CMPS/STOS/LODS/SCAS, INS/OUTS) ──
            0x6C | 0x6D | 0x6E | 0x6F | 0xA4 | 0xA5 | 0xA6 | 0xA7 | 0xAA | 0xAB | 0xAC | 0xAD
            | 0xAE | 0xAF => self.exec_string_op(bus, opcode),

            // ── INC/DEC r/m (group FE/FF) ─────────────────────────────────
            0xFE => {
                let m = decode_modrm(self, bus);
                match m.reg & 0b111 {
                    0 => {
                        let a = self.read_rm8(bus, &m.rm);
                        let r = self.inc_u8(a);
                        self.write_rm8(bus, &m.rm, r);
                        Self::cycles_for(&m.rm, 1, 3)
                    }
                    1 => {
                        let a = self.read_rm8(bus, &m.rm);
                        let r = self.dec_u8(a);
                        self.write_rm8(bus, &m.rm, r);
                        Self::cycles_for(&m.rm, 1, 3)
                    }
                    2 => {
                        let target = self.read_rm16(bus, &m.rm);
                        let ret_ip = self.regs.ip;
                        self.push16(bus, ret_ip);
                        self.regs.ip = target;
                        Self::cycles_for(&m.rm, 5, 6)
                    }
                    3 => {
                        let addr = match m.rm {
                            RegMem::Mem(a) => a,
                            RegMem::Reg(rm) => self.ws_reg_mode_effective_address(rm),
                        };
                        let new_ip = bus.read_u16(addr);
                        let new_cs = bus.read_u16(addr.wrapping_add(2));
                        let ret_cs = self.regs.cs;
                        let ret_ip = self.regs.ip;
                        self.push16(bus, ret_cs);
                        self.push16(bus, ret_ip);
                        self.regs.ip = new_ip;
                        self.regs.cs = new_cs;
                        12
                    }
                    4 => {
                        let target = self.read_rm16(bus, &m.rm);
                        self.regs.ip = target;
                        Self::cycles_for(&m.rm, 4, 5)
                    }
                    5 => {
                        let addr = match m.rm {
                            RegMem::Mem(a) => a,
                            RegMem::Reg(rm) => self.ws_reg_mode_effective_address(rm),
                        };
                        let new_ip = bus.read_u16(addr);
                        let new_cs = bus.read_u16(addr.wrapping_add(2));
                        self.regs.ip = new_ip;
                        self.regs.cs = new_cs;
                        9
                    }
                    6 => {
                        let v = self.read_rm16(bus, &m.rm);
                        self.push16(bus, v);
                        Self::cycles_for(&m.rm, 1, 2)
                    }
                    _ => 1,
                }
            }
            0xFF => {
                let m = decode_modrm(self, bus);
                match m.reg & 0b111 {
                    0 => {
                        let a = self.read_rm16(bus, &m.rm);
                        let r = self.inc_u16(a);
                        self.write_rm16(bus, &m.rm, r);
                        Self::cycles_for(&m.rm, 1, 3)
                    }
                    1 => {
                        let a = self.read_rm16(bus, &m.rm);
                        let r = self.dec_u16(a);
                        self.write_rm16(bus, &m.rm, r);
                        Self::cycles_for(&m.rm, 1, 3)
                    }
                    2 => {
                        // CALL near indirect
                        let target = self.read_rm16(bus, &m.rm);
                        let ret_ip = self.regs.ip;
                        self.push16(bus, ret_ip);
                        self.regs.ip = target;
                        Self::cycles_for(&m.rm, 5, 6)
                    }
                    3 => {
                        // CALL far indirect: [rm] = ip, [rm+2] = cs
                        let addr = match m.rm {
                            RegMem::Mem(a) => a,
                            RegMem::Reg(_) => return self.fault_unsupported(opcode),
                        };
                        let new_ip = bus.read_u16(addr);
                        let new_cs = bus.read_u16(addr.wrapping_add(2));
                        let ret_cs = self.regs.cs;
                        let ret_ip = self.regs.ip;
                        self.push16(bus, ret_cs);
                        self.push16(bus, ret_ip);
                        self.regs.ip = new_ip;
                        self.regs.cs = new_cs;
                        12
                    }
                    4 => {
                        // JMP near indirect
                        let target = self.read_rm16(bus, &m.rm);
                        self.regs.ip = target;
                        Self::cycles_for(&m.rm, 4, 5)
                    }
                    5 => {
                        // JMP far indirect: [rm] = ip, [rm+2] = cs
                        let addr = match m.rm {
                            RegMem::Mem(a) => a,
                            RegMem::Reg(_) => return self.fault_unsupported(opcode),
                        };
                        let new_ip = bus.read_u16(addr);
                        let new_cs = bus.read_u16(addr.wrapping_add(2));
                        self.regs.ip = new_ip;
                        self.regs.cs = new_cs;
                        9
                    }
                    6 => {
                        // PUSH r/m16
                        let v = self.read_rm16(bus, &m.rm);
                        self.push16(bus, v);
                        Self::cycles_for(&m.rm, 1, 2)
                    }
                    _ => 1,
                }
            }

            // ── INT / IRET / INTO ─────────────────────────────────────────
            0xCC => {
                // INT3: one-byte breakpoint interrupt.
                self.handle_irq(bus, 3);
                10
            }
            0xCD => {
                // INT n: software interrupt
                let n = self.fetch_u8(bus);
                self.handle_irq(bus, n);
                10
            }
            0xCE => {
                // INTO: INT 4 if overflow flag is set
                if self.flags.overflow {
                    self.handle_irq(bus, 4);
                    13
                } else {
                    6
                }
            }
            0xCF => {
                // IRET: restore IP, CS, FLAGS from stack
                self.regs.ip = self.pop16(bus);
                self.regs.cs = self.pop16(bus);
                let flags = self.pop16(bus);
                let old_interrupt = self.flags.interrupt;
                let old_trap = self.flags.trap;
                self.flags = Flags::from_u16(flags);
                if !old_interrupt && self.flags.interrupt {
                    self.interrupt_inhibit = 1;
                }
                if !old_trap && self.flags.trap {
                    self.trap_inhibit = 1;
                }
                10
            }

            // ── IN / OUT port I/O ─────────────────────────────────────────
            0xE4 => {
                // IN AL, imm8
                let port = self.fetch_u8(bus);
                let v = bus.read_io(port);
                self.regs.set_reg8(0, v);
                7
            }
            0xE5 => {
                // IN AX, imm8
                let port = self.fetch_u8(bus);
                let lo = bus.read_io(port) as u16;
                let hi = bus.read_io(port.wrapping_add(1)) as u16;
                self.regs.ax = lo | (hi << 8);
                7
            }
            0xE6 => {
                // OUT imm8, AL
                let port = self.fetch_u8(bus);
                let v = self.regs.get_reg8(0);
                bus.write_io(port, v);
                7 + bus.take_wait_cycles()
            }
            0xE7 => {
                // OUT imm8, AX
                let port = self.fetch_u8(bus);
                let v = self.regs.ax;
                bus.write_io(port, v as u8);
                bus.write_io(port.wrapping_add(1), (v >> 8) as u8);
                7 + bus.take_wait_cycles()
            }
            0xEC => {
                // IN AL, DX
                let port = self.regs.dx as u8;
                let v = bus.read_io(port);
                self.regs.set_reg8(0, v);
                5
            }
            0xED => {
                // IN AX, DX
                let port = self.regs.dx as u8;
                let lo = bus.read_io(port) as u16;
                let hi = bus.read_io(port.wrapping_add(1)) as u16;
                self.regs.ax = lo | (hi << 8);
                5
            }
            0xEE => {
                // OUT DX, AL
                let port = self.regs.dx as u8;
                let v = self.regs.get_reg8(0);
                bus.write_io(port, v);
                5 + bus.take_wait_cycles()
            }
            0xEF => {
                // OUT DX, AX
                let port = self.regs.dx as u8;
                let v = self.regs.ax;
                bus.write_io(port, v as u8);
                bus.write_io(port.wrapping_add(1), (v >> 8) as u8);
                5 + bus.take_wait_cycles()
            }

            // ── 80186 / V30 instruction-set additions ────────────────────────
            0x60 => {
                // PUSHA: push AX, CX, DX, BX, the original SP, BP, SI, DI.
                let sp = self.regs.sp;
                self.push16(bus, self.regs.ax);
                self.push16(bus, self.regs.cx);
                self.push16(bus, self.regs.dx);
                self.push16(bus, self.regs.bx);
                self.push16(bus, sp);
                self.push16(bus, self.regs.bp);
                self.push16(bus, self.regs.si);
                self.push16(bus, self.regs.di);
                9
            }
            0x61 => {
                // POPA: pop DI, SI, BP, (discarded SP slot), BX, DX, CX, AX.
                self.regs.di = self.pop16(bus);
                self.regs.si = self.pop16(bus);
                self.regs.bp = self.pop16(bus);
                let _ = self.pop16(bus);
                self.regs.bx = self.pop16(bus);
                self.regs.dx = self.pop16(bus);
                self.regs.cx = self.pop16(bus);
                self.regs.ax = self.pop16(bus);
                8
            }
            0x62 => {
                // BOUND r16, m16&16: INT 5 if the index is outside [lower, upper].
                let m = decode_modrm(self, bus);
                if let RegMem::Mem(addr) = m.rm {
                    let index = self.regs.get_reg16(m.reg) as i16;
                    let lower = bus.read_u16(addr) as i16;
                    let upper = bus.read_u16(addr.wrapping_add(2)) as i16;
                    if index < lower || index > upper {
                        self.handle_irq(bus, 5); // INT 5: BOUND range exceeded
                    }
                }
                13
            }
            0x63 => 1,
            0x64..=0x67 => 1,
            0x68 => {
                // PUSH imm16
                let v = self.fetch_u16(bus);
                self.push16(bus, v);
                1
            }
            0x69 => {
                // IMUL r16, r/m16, imm16
                let m = decode_modrm(self, bus);
                let src = self.read_rm16(bus, &m.rm);
                let imm = self.fetch_u16(bus);
                let product = self.imul_u16(src, imm);
                self.regs.set_reg16(m.reg, product as u16);
                Self::cycles_for(&m.rm, 3, 4)
            }
            0x6A => {
                // PUSH imm8 (sign-extended to 16 bits)
                let v = self.fetch_u8(bus) as i8 as i16 as u16;
                self.push16(bus, v);
                1
            }
            0x6B => {
                // IMUL r16, r/m16, imm8 (imm sign-extended)
                let m = decode_modrm(self, bus);
                let src = self.read_rm16(bus, &m.rm);
                let imm = self.fetch_u8(bus) as i8 as i16 as u16;
                let product = self.imul_u16(src, imm);
                self.regs.set_reg16(m.reg, product as u16);
                Self::cycles_for(&m.rm, 3, 4)
            }
            0xC0 => {
                // Shift/rotate r/m8, imm8
                let m = decode_modrm(self, bus);
                if m.reg == 6 {
                    let _ = self.fetch_u8(bus);
                    self.write_rm8(bus, &m.rm, 0);
                    return Self::cycles_for(&m.rm, 3, 5);
                }
                let op = shift_op_from_reg_field(m.reg);
                let count = self.fetch_u8(bus);
                let a = self.read_rm8(bus, &m.rm);
                let r = self.shift_u8(op, a, count);
                self.write_rm8(bus, &m.rm, r);
                Self::cycles_for(&m.rm, 3, 5)
            }
            0xC1 => {
                // Shift/rotate r/m16, imm8
                let m = decode_modrm(self, bus);
                if m.reg == 6 {
                    let _ = self.fetch_u8(bus);
                    self.write_rm16(bus, &m.rm, 0);
                    return Self::cycles_for(&m.rm, 3, 5);
                }
                let op = shift_op_from_reg_field(m.reg);
                let count = self.fetch_u8(bus);
                let a = self.read_rm16(bus, &m.rm);
                let r = self.shift_u16(op, a, count);
                self.write_rm16(bus, &m.rm, r);
                Self::cycles_for(&m.rm, 3, 5)
            }
            0x8F => {
                // POP r/m16 (reg field 0)
                let m = decode_modrm(self, bus);
                let v = self.pop16(bus);
                self.write_rm16(bus, &m.rm, v);
                Self::cycles_for(&m.rm, 1, 3)
            }

            // ESC opcodes for an external coprocessor. WonderSwan has no x87,
            // so consume the ModRM/displacement bytes and otherwise ignore.
            0xD8..=0xDF => {
                let _ = self.fetch_u8(bus);
                1
            }

            // LOCK prefix. The bus is single-threaded here, so execute the
            // following instruction normally after consuming the prefix.
            0xF0 => {
                let op = self.fetch_u8(bus);
                self.execute(op, bus)
            }
            0xF1 => {
                // INT1/ICEBP-style one-byte breakpoint.
                self.handle_irq(bus, 1);
                10
            }

            _ => self.fault_unsupported(opcode),
        }
    }

    /// Decodes a ModRM byte and returns `(reg, 16-bit-EA-offset)` without
    /// applying any segment register. This is the correct path for LEA, which
    /// stores the raw offset rather than reading from or writing to memory.
    fn lea_decode<B: MemoryBus>(&mut self, bus: &mut B) -> (u8, u16) {
        let byte = self.fetch_u8(bus);
        let md = byte >> 6;
        let reg = (byte >> 3) & 0b111;
        let rm = byte & 0b111;

        if md == 0b11 {
            return (reg, self.ws_reg_mode_effective_offset(rm));
        }

        if md == 0b00 && rm == 0b110 {
            let disp = self.fetch_u16(bus);
            return (reg, disp);
        }

        let base: u16 = match rm {
            0b000 => self.regs.bx.wrapping_add(self.regs.si),
            0b001 => self.regs.bx.wrapping_add(self.regs.di),
            0b010 => self.regs.bp.wrapping_add(self.regs.si),
            0b011 => self.regs.bp.wrapping_add(self.regs.di),
            0b100 => self.regs.si,
            0b101 => self.regs.di,
            0b110 => self.regs.bp,
            0b111 => self.regs.bx,
            _ => unreachable!(),
        };

        let disp: u16 = match md {
            0b00 => 0,
            0b01 => self.fetch_u8(bus) as i8 as i16 as u16,
            0b10 => self.fetch_u16(bus),
            _ => unreachable!(),
        };

        (reg, base.wrapping_add(disp))
    }

    fn ws_reg_mode_effective_offset(&self, rm: u8) -> u16 {
        match rm & 0b111 {
            0 => self.regs.bx.wrapping_add(self.regs.ax),
            1 => self.regs.bx.wrapping_add(self.regs.cx),
            2 => self.regs.bp.wrapping_add(self.regs.dx),
            3 => self.regs.bp.wrapping_add(self.regs.bx),
            4 => self.regs.si.wrapping_add(self.regs.sp),
            5 => self.regs.di.wrapping_add(self.regs.bp),
            6 => self.regs.bp.wrapping_add(self.regs.si),
            7 => self.regs.bx.wrapping_add(self.regs.di),
            _ => unreachable!(),
        }
    }

    fn ws_reg_mode_effective_address(&self, rm: u8) -> u32 {
        let segment = match rm & 0b111 {
            2 | 3 | 6 => self.regs.ss,
            _ => self.regs.ds,
        };
        linear_address(segment, self.ws_reg_mode_effective_offset(rm))
    }

    /// Executes one string instruction opcode, with REP/REPE/REPNE looping
    /// if `self.rep_prefix` is set. Returns the total cycle count.
    fn exec_string_op<B: MemoryBus>(&mut self, bus: &mut B, op: u8) -> u32 {
        let src_seg = self.seg_override.unwrap_or(self.regs.ds);
        // V30MZ per-execution clock counts (stsws; the REP-repeated per-element
        // cost is approximated by this base, see the `timing` module).
        let base_cycles: u32 = match op {
            0x6C | 0x6D => 6, // INS  (INM)
            0x6E | 0x6F => 7, // OUTS (OUTM)
            0xA4 | 0xA5 => 5, // MOVS (MOVBK)
            0xA6 | 0xA7 => 6, // CMPS (CMPBK)
            0xAA | 0xAB => 3, // STOS (STM)
            0xAC | 0xAD => 3, // LODS (LDM)
            0xAE | 0xAF => 4, // SCAS (CMPM)
            _ => unreachable!(),
        };

        let Some(rep) = self.rep_prefix else {
            self.string_step(bus, op, src_seg);
            return base_cycles;
        };

        // REP loop: execute while CX != 0, with optional ZF check for
        // REPE (0xF3) and REPNE (0xF2) on CMPS/SCAS.
        let mut count = self.regs.cx;
        let mut total = 0u32;
        while count != 0 {
            self.string_step(bus, op, src_seg);
            count -= 1;
            self.regs.cx = count;
            total += base_cycles;
            let check_zf = matches!(op, 0xA6 | 0xA7 | 0xAE | 0xAF);
            if check_zf {
                if rep == 0xF3 && !self.flags.zero {
                    break;
                }
                if rep == 0xF2 && self.flags.zero {
                    break;
                }
            }
        }
        if total >= LONG_REP_INTERRUPT_RETURN_CYCLE_THRESHOLD {
            self.interrupt_return_override_ip = Some(self.instruction_start_ip);
        }
        total
    }

    /// Performs one iteration of a string instruction, updating SI/DI
    /// according to the direction flag.
    fn string_step<B: MemoryBus>(&mut self, bus: &mut B, op: u8, src_seg: u16) {
        let wide = op & 1 != 0;
        let step: u16 = if wide { 2 } else { 1 };
        let delta: u16 = if self.flags.direction {
            step.wrapping_neg()
        } else {
            step
        };

        match op {
            0x6C => {
                // INSB: port DX → ES:DI
                let port = self.regs.dx as u8;
                let v = bus.read_io(port);
                bus.write_u8(linear_address(self.regs.es, self.regs.di), v);
                self.regs.di = self.regs.di.wrapping_add(delta);
            }
            0x6D => {
                // INSW: port DX (and DX+1) → ES:DI
                let port = self.regs.dx as u8;
                let lo = bus.read_io(port);
                let hi = bus.read_io(port.wrapping_add(1));
                let v = u16::from_le_bytes([lo, hi]);
                bus.write_u16(linear_address(self.regs.es, self.regs.di), v);
                self.regs.di = self.regs.di.wrapping_add(delta);
            }
            0x6E => {
                // OUTSB: DS:SI → port DX
                let port = self.regs.dx as u8;
                let v = bus.read_u8(linear_address(src_seg, self.regs.si));
                bus.write_io(port, v);
                self.regs.si = self.regs.si.wrapping_add(delta);
            }
            0x6F => {
                // OUTSW: DS:SI → port DX (and DX+1)
                let port = self.regs.dx as u8;
                let v = bus.read_u16(linear_address(src_seg, self.regs.si));
                let [lo, hi] = v.to_le_bytes();
                bus.write_io(port, lo);
                bus.write_io(port.wrapping_add(1), hi);
                self.regs.si = self.regs.si.wrapping_add(delta);
            }
            0xA4 => {
                let v = bus.read_u8(linear_address(src_seg, self.regs.si));
                bus.write_u8(linear_address(self.regs.es, self.regs.di), v);
                self.regs.si = self.regs.si.wrapping_add(delta);
                self.regs.di = self.regs.di.wrapping_add(delta);
            }
            0xA5 => {
                let v = bus.read_u16(linear_address(src_seg, self.regs.si));
                bus.write_u16(linear_address(self.regs.es, self.regs.di), v);
                self.regs.si = self.regs.si.wrapping_add(delta);
                self.regs.di = self.regs.di.wrapping_add(delta);
            }
            0xA6 => {
                let a = bus.read_u8(linear_address(src_seg, self.regs.si));
                let b = bus.read_u8(linear_address(self.regs.es, self.regs.di));
                self.sub_u8(a, b, 0);
                self.regs.si = self.regs.si.wrapping_add(delta);
                self.regs.di = self.regs.di.wrapping_add(delta);
            }
            0xA7 => {
                let a = bus.read_u16(linear_address(src_seg, self.regs.si));
                let b = bus.read_u16(linear_address(self.regs.es, self.regs.di));
                self.sub_u16(a, b, 0);
                self.regs.si = self.regs.si.wrapping_add(delta);
                self.regs.di = self.regs.di.wrapping_add(delta);
            }
            0xAA => {
                let v = self.regs.get_reg8(0);
                bus.write_u8(linear_address(self.regs.es, self.regs.di), v);
                self.regs.di = self.regs.di.wrapping_add(delta);
            }
            0xAB => {
                let v = self.regs.ax;
                bus.write_u16(linear_address(self.regs.es, self.regs.di), v);
                self.regs.di = self.regs.di.wrapping_add(delta);
            }
            0xAC => {
                let v = bus.read_u8(linear_address(src_seg, self.regs.si));
                self.regs.set_reg8(0, v);
                self.regs.si = self.regs.si.wrapping_add(delta);
            }
            0xAD => {
                let v = bus.read_u16(linear_address(src_seg, self.regs.si));
                self.regs.ax = v;
                self.regs.si = self.regs.si.wrapping_add(delta);
            }
            0xAE => {
                let a = self.regs.get_reg8(0);
                let b = bus.read_u8(linear_address(self.regs.es, self.regs.di));
                self.sub_u8(a, b, 0);
                self.regs.di = self.regs.di.wrapping_add(delta);
            }
            0xAF => {
                let a = self.regs.ax;
                let b = bus.read_u16(linear_address(self.regs.es, self.regs.di));
                self.sub_u16(a, b, 0);
                self.regs.di = self.regs.di.wrapping_add(delta);
            }
            _ => unreachable!(),
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
                Self::cycles_for(&m.rm, 1, if op == AluOp::Cmp { 2 } else { 3 })
            }
            1 => {
                let m = decode_modrm(self, bus);
                let a = self.read_rm16(bus, &m.rm);
                let b = self.regs.get_reg16(m.reg);
                let r = self.alu_u16(op, a, b);
                if op != AluOp::Cmp {
                    self.write_rm16(bus, &m.rm, r);
                }
                Self::cycles_for(&m.rm, 1, if op == AluOp::Cmp { 2 } else { 3 })
            }
            2 => {
                let m = decode_modrm(self, bus);
                let a = self.regs.get_reg8(m.reg);
                let b = self.read_rm8(bus, &m.rm);
                let r = self.alu_u8(op, a, b);
                if op != AluOp::Cmp {
                    self.regs.set_reg8(m.reg, r);
                }
                Self::cycles_for(&m.rm, 1, 2)
            }
            3 => {
                let m = decode_modrm(self, bus);
                let a = self.regs.get_reg16(m.reg);
                let b = self.read_rm16(bus, &m.rm);
                let r = self.alu_u16(op, a, b);
                if op != AluOp::Cmp {
                    self.regs.set_reg16(m.reg, r);
                }
                Self::cycles_for(&m.rm, 1, 2)
            }
            4 => {
                let imm = self.fetch_u8(bus);
                let a = self.regs.get_reg8(0);
                let r = self.alu_u8(op, a, imm);
                if op != AluOp::Cmp {
                    self.regs.set_reg8(0, r);
                }
                1
            }
            5 => {
                let imm = self.fetch_u16(bus);
                let a = self.regs.ax;
                let r = self.alu_u16(op, a, imm);
                if op != AluOp::Cmp {
                    self.regs.ax = r;
                }
                1
            }
            _ => unreachable!(),
        }
    }
}
