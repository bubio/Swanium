use super::bus::{linear_address, MemoryBus};
use super::Cpu;

/// A decoded ModRM operand: either a register index or an already-resolved
/// 20-bit physical address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegMem {
    Reg(u8),
    Mem(u32),
}

#[derive(Debug, Clone, Copy)]
pub struct ModRm {
    pub reg: u8,
    pub rm: RegMem,
}

/// Decodes a ModRM byte (and any trailing displacement) using the 8086/V30
/// 16-bit addressing modes. If `cpu.seg_override` is set (by a preceding
/// segment-override prefix opcode), that segment is used instead of the
/// default DS/SS. The default segment is SS for BP-based forms and DS
/// otherwise, per the standard 8086 effective address table.
pub fn decode_modrm<B: MemoryBus>(cpu: &mut Cpu, bus: &mut B) -> ModRm {
    let byte = cpu.fetch_u8(bus);
    let md = byte >> 6;
    let reg = (byte >> 3) & 0b111;
    let rm = byte & 0b111;

    if md == 0b11 {
        return ModRm {
            reg,
            rm: RegMem::Reg(rm),
        };
    }

    if md == 0b00 && rm == 0b110 {
        // Direct address: disp16, no base register.
        let disp = cpu.fetch_u16(bus);
        let seg = cpu.seg_override.unwrap_or(cpu.regs.ds);
        return ModRm {
            reg,
            rm: RegMem::Mem(linear_address(seg, disp)),
        };
    }

    let (base, uses_bp) = match rm {
        0b000 => (cpu.regs.bx.wrapping_add(cpu.regs.si), false),
        0b001 => (cpu.regs.bx.wrapping_add(cpu.regs.di), false),
        0b010 => (cpu.regs.bp.wrapping_add(cpu.regs.si), true),
        0b011 => (cpu.regs.bp.wrapping_add(cpu.regs.di), true),
        0b100 => (cpu.regs.si, false),
        0b101 => (cpu.regs.di, false),
        0b110 => (cpu.regs.bp, true),
        0b111 => (cpu.regs.bx, false),
        _ => unreachable!(),
    };

    let disp: u16 = match md {
        0b00 => 0,
        0b01 => cpu.fetch_u8(bus) as i8 as i16 as u16,
        0b10 => cpu.fetch_u16(bus),
        _ => unreachable!(),
    };

    let offset = base.wrapping_add(disp);
    let default_seg = if uses_bp { cpu.regs.ss } else { cpu.regs.ds };
    let segment = cpu.seg_override.unwrap_or(default_seg);
    ModRm {
        reg,
        rm: RegMem::Mem(linear_address(segment, offset)),
    }
}
