//! 80186 / V30 instruction-set additions beyond the 8086 base set:
//! PUSHA/POPA, BOUND, PUSH/IMUL with immediates, immediate-count shifts,
//! POP r/m16, and the INS/OUTS string-I/O instructions.

use super::super::MemoryBus;
use super::{run_with, Cpu, FlatMemory};

/// Memory plus a 256-entry I/O space, recording the last `OUT`, for INS/OUTS.
struct IoMemory {
    data: Vec<u8>,
    io: [u8; 256],
    last_out: Option<(u8, u8)>,
}

impl IoMemory {
    fn new(code: &[u8]) -> Self {
        let mut data = vec![0u8; 0x10_0000];
        data[..code.len()].copy_from_slice(code);
        Self {
            data,
            io: [0; 256],
            last_out: None,
        }
    }
}

impl MemoryBus for IoMemory {
    fn read_u8(&self, addr: u32) -> u8 {
        self.data[addr as usize]
    }

    fn write_u8(&mut self, addr: u32, value: u8) {
        self.data[addr as usize] = value;
    }

    fn read_io(&mut self, port: u8) -> u8 {
        self.io[port as usize]
    }

    fn write_io(&mut self, port: u8, value: u8) {
        self.io[port as usize] = value;
        self.last_out = Some((port, value));
    }
}

/// Build a CPU + [`IoMemory`] at CS:IP = 0, apply `setup`, run one instruction.
fn run_io(setup: impl FnOnce(&mut Cpu, &mut IoMemory), code: &[u8]) -> (Cpu, IoMemory) {
    let mut mem = IoMemory::new(code);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFE;
    setup(&mut cpu, &mut mem);
    cpu.step(&mut mem);
    (cpu, mem)
}

// ── PUSHA (0x60) ─────────────────────────────────────────────────────────────

#[test]
fn pusha_decrements_sp_by_16() {
    let (cpu, _, _) = run_with(|_| {}, &[0x60]);
    assert_eq!(cpu.regs.sp, 0xFFEE);
}

#[test]
fn pusha_pushes_ax_first() {
    let (_, _, mem) = run_with(|cpu| cpu.regs.ax = 0x1111, &[0x60]);
    assert_eq!(mem.read_u16(0xFFFC), 0x1111);
}

#[test]
fn pusha_pushes_di_last() {
    let (_, _, mem) = run_with(|cpu| cpu.regs.di = 0x8888, &[0x60]);
    assert_eq!(mem.read_u16(0xFFEE), 0x8888);
}

#[test]
fn pusha_pushes_original_sp() {
    let (_, _, mem) = run_with(|_| {}, &[0x60]);
    // The SP slot (5th push) holds the value SP had before PUSHA (0xFFFE).
    assert_eq!(mem.read_u16(0xFFF4), 0xFFFE);
}

// ── POPA (0x61) ──────────────────────────────────────────────────────────────

/// Run POPA with a pre-loaded stack of eight distinct words at SP = 0xFFEE.
fn run_popa() -> Cpu {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0x61]);
    mem.write_u16(0xFFEE, 0x00D1); // DI
    mem.write_u16(0xFFF0, 0x0051); // SI
    mem.write_u16(0xFFF2, 0x00B0); // BP
    mem.write_u16(0xFFF4, 0x1111); // discarded SP slot
    mem.write_u16(0xFFF6, 0x00B3); // BX
    mem.write_u16(0xFFF8, 0x00D2); // DX
    mem.write_u16(0xFFFA, 0x00C1); // CX
    mem.write_u16(0xFFFC, 0x00A0); // AX
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFEE;
    cpu.step(&mut mem);
    cpu
}

#[test]
fn popa_restores_ax() {
    assert_eq!(run_popa().regs.ax, 0x00A0);
}

#[test]
fn popa_restores_di() {
    assert_eq!(run_popa().regs.di, 0x00D1);
}

#[test]
fn popa_increments_sp_by_16() {
    assert_eq!(run_popa().regs.sp, 0xFFFE);
}

#[test]
fn popa_discards_sp_slot() {
    // Final SP comes from the eight pops, not the (bogus) SP-slot value.
    assert_ne!(run_popa().regs.sp, 0x1111);
}

// ── PUSH imm (0x68 / 0x6A) ───────────────────────────────────────────────────

#[test]
fn push_imm16_stores_value() {
    let (_, _, mem) = run_with(|_| {}, &[0x68, 0x34, 0x12]);
    assert_eq!(mem.read_u16(0xFFFC), 0x1234);
}

#[test]
fn push_imm8_is_sign_extended() {
    let (_, _, mem) = run_with(|_| {}, &[0x6A, 0xFF]);
    assert_eq!(mem.read_u16(0xFFFC), 0xFFFF);
}

// ── IMUL with immediate (0x69 / 0x6B) ────────────────────────────────────────

#[test]
fn imul_reg_rm_imm16_computes_product() {
    // IMUL AX, BX, 3 (modrm 0xC3, imm16 = 0x0003); BX = 5 → AX = 15.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.bx = 5, &[0x69, 0xC3, 0x03, 0x00]);
    assert_eq!(cpu.regs.ax, 15);
}

#[test]
fn imul_reg_rm_imm8_sign_extends_immediate() {
    // IMUL AX, BX, -1 (imm8 0xFF); BX = 5 → AX = -5 = 0xFFFB.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.bx = 5, &[0x6B, 0xC3, 0xFF]);
    assert_eq!(cpu.regs.ax, 0xFFFB);
}

#[test]
fn imul_imm_sets_overflow_when_truncated() {
    // 0x4000 * 4 = 0x10000 does not fit in i16 → CF/OF set.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.bx = 0x4000, &[0x6B, 0xC3, 0x04]);
    assert!(cpu.flags.overflow);
}

// ── Immediate-count shift/rotate (0xC0 / 0xC1) ───────────────────────────────

#[test]
fn shl_rm16_by_immediate_count() {
    // SHL AX, 2 (0xC1 /4, modrm 0xE0, imm8 = 2); AX = 1 → 4.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 1, &[0xC1, 0xE0, 0x02]);
    assert_eq!(cpu.regs.ax, 4);
}

#[test]
fn shl_rm8_by_immediate_count() {
    // SHL AL, 2 (0xC0 /4, modrm 0xE0, imm8 = 2); AL = 1 → 4.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 1, &[0xC0, 0xE0, 0x02]);
    assert_eq!(cpu.regs.ax & 0xFF, 4);
}

#[test]
fn wscputest_c0_c1_group_six_zero_destination_without_flags() {
    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x501A;
            cpu.flags.carry = true;
            cpu.flags.zero = true;
        },
        &[0xC1, 0xF0, 0x03],
    );
    assert_eq!(cpu.regs.ax, 0);
    assert!(cpu.flags.carry);
    assert!(cpu.flags.zero);

    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x101A;
            cpu.flags.carry = true;
            cpu.flags.zero = true;
        },
        &[0xC0, 0xF0, 0x02],
    );
    assert_eq!(cpu.regs.ax, 0x1000);
    assert!(cpu.flags.carry);
    assert!(cpu.flags.zero);
}

// ── POP r/m16 (0x8F) ─────────────────────────────────────────────────────────

#[test]
fn pop_rm16_into_register() {
    // POP AX (modrm 0xC0) from a pre-loaded stack slot.
    let mut mem = FlatMemory::new();
    mem.load(0, &[0x8F, 0xC0]);
    mem.write_u16(0xFFFC, 0xBEEF);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFC;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.ax, 0xBEEF);
}

#[test]
fn pop_rm16_increments_sp() {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0x8F, 0xC0]);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFC;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.sp, 0xFFFE);
}

#[test]
fn wscputest_f6_f7_group_one_consumes_immediate_without_side_effects() {
    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0xD01A;
            cpu.flags.zero = true;
            cpu.flags.carry = true;
        },
        &[0xF6, 0xC8, 0x90],
    );
    assert_eq!(cpu.regs.ax, 0xD01A);
    assert_eq!(cpu.regs.ip, 3);
    assert!(cpu.flags.zero);
    assert!(cpu.flags.carry);

    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0xD01A;
            cpu.flags.zero = true;
            cpu.flags.carry = true;
        },
        &[0xF7, 0xC8, 0x90, 0x90],
    );
    assert_eq!(cpu.regs.ax, 0xD01A);
    assert_eq!(cpu.regs.ip, 4);
    assert!(cpu.flags.zero);
    assert!(cpu.flags.carry);
}

#[test]
fn wscputest_fe_extended_groups_match_ff_variants() {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xFE, 0xF0]); // FE /6 acts like PUSH r/m16.
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFE;
    cpu.regs.ax = 0xFE5A;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.sp, 0xFFFC);
    assert_eq!(mem.read_u16(0xFFFC), 0xFE5A);
}

#[test]
fn wscputest_ff_group_seven_is_noop() {
    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x55AA;
            cpu.flags.carry = true;
        },
        &[0xFF, 0xF8],
    );
    assert_eq!(cpu.regs.ax, 0x55AA);
    assert!(cpu.flags.carry);
    assert_eq!(cpu.regs.ip, 2);
    assert!(cpu.fault.is_none());
}

// ── BOUND (0x62) ─────────────────────────────────────────────────────────────

/// Run `BOUND AX, [0x0200]` with the bounds `[lower, upper]` at 0x0200 and a
/// vector at IVT entry 5 pointing to offset 0x0050. Returns the post-step CPU.
fn run_bound(index: u16, lower: u16, upper: u16) -> Cpu {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0x62, 0x06, 0x00, 0x02]); // BOUND AX, [0x0200]
    mem.write_u16(0x0200, lower);
    mem.write_u16(0x0202, upper);
    mem.write_u16(0x0014, 0x0050); // IVT[5] offset
    mem.write_u16(0x0016, 0x0000); // IVT[5] segment
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFE;
    cpu.regs.ax = index;
    cpu.step(&mut mem);
    cpu
}

#[test]
fn bound_in_range_does_not_vector() {
    // Index 5 within [0, 10]: execution falls through (IP past the 4-byte insn).
    assert_eq!(run_bound(5, 0, 10).regs.ip, 4);
}

#[test]
fn bound_below_lower_raises_int5() {
    // Index 0xFFFF (-1) below 0 → INT 5 → IP taken from IVT[5].
    assert_eq!(run_bound(0xFFFF, 0, 10).regs.ip, 0x0050);
}

#[test]
fn bound_above_upper_raises_int5() {
    assert_eq!(run_bound(20, 0, 10).regs.ip, 0x0050);
}

// ── INS / OUTS string I/O (0x6C–0x6F) ────────────────────────────────────────

#[test]
fn insb_reads_port_into_memory() {
    // INSB: port DX=0x10 → ES:DI=0x0500. Port latched with 0xAB.
    let (_, mem) = run_io(
        |cpu, mem| {
            cpu.regs.dx = 0x10;
            cpu.regs.es = 0;
            cpu.regs.di = 0x0500;
            mem.io[0x10] = 0xAB;
        },
        &[0x6C],
    );
    assert_eq!(mem.read_u8(0x0500), 0xAB);
}

#[test]
fn insb_increments_di() {
    let (cpu, _) = run_io(
        |cpu, _| {
            cpu.regs.es = 0;
            cpu.regs.di = 0x0500;
        },
        &[0x6C],
    );
    assert_eq!(cpu.regs.di, 0x0501);
}

#[test]
fn insw_reads_word_from_two_ports() {
    // INSW: ports 0x10 (lo) and 0x11 (hi) → word at ES:DI.
    let (_, mem) = run_io(
        |cpu, mem| {
            cpu.regs.dx = 0x10;
            cpu.regs.es = 0;
            cpu.regs.di = 0x0500;
            mem.io[0x10] = 0xCD;
            mem.io[0x11] = 0xAB;
        },
        &[0x6D],
    );
    assert_eq!(mem.read_u16(0x0500), 0xABCD);
}

#[test]
fn outsb_writes_memory_to_port() {
    // OUTSB: DS:SI=0x0400 → port DX=0x20.
    let (_, mem) = run_io(
        |cpu, mem| {
            cpu.regs.dx = 0x20;
            cpu.regs.ds = 0;
            cpu.regs.si = 0x0400;
            mem.data[0x0400] = 0xCD;
        },
        &[0x6E],
    );
    assert_eq!(mem.last_out, Some((0x20, 0xCD)));
}

#[test]
fn outsb_increments_si() {
    let (cpu, _) = run_io(
        |cpu, _| {
            cpu.regs.ds = 0;
            cpu.regs.si = 0x0400;
        },
        &[0x6E],
    );
    assert_eq!(cpu.regs.si, 0x0401);
}

#[test]
fn outsw_writes_high_byte_to_next_port() {
    // OUTSW: word 0xABCD at DS:SI → ports 0x20 (lo) then 0x21 (hi); last is hi.
    let (_, mem) = run_io(
        |cpu, mem| {
            cpu.regs.dx = 0x20;
            cpu.regs.ds = 0;
            cpu.regs.si = 0x0400;
            mem.data[0x0400] = 0xCD;
            mem.data[0x0401] = 0xAB;
        },
        &[0x6F],
    );
    assert_eq!(mem.last_out, Some((0x21, 0xAB)));
}

#[test]
fn outsb_honours_direction_flag() {
    // With DF set, SI decrements.
    let (cpu, _) = run_io(
        |cpu, _| {
            cpu.regs.ds = 0;
            cpu.regs.si = 0x0400;
            cpu.flags.direction = true;
        },
        &[0x6E],
    );
    assert_eq!(cpu.regs.si, 0x03FF);
}

#[test]
fn rep_outsb_consumes_cx() {
    // REP OUTSB (0xF3 0x6E) with CX=3 transfers three bytes, leaving CX=0.
    let (cpu, _) = run_io(
        |cpu, _| {
            cpu.regs.cx = 3;
            cpu.regs.dx = 0x20;
            cpu.regs.ds = 0;
            cpu.regs.si = 0x0400;
        },
        &[0xF3, 0x6E],
    );
    assert_eq!(cpu.regs.cx, 0);
}

#[test]
fn rep_insb_advances_di_per_byte() {
    // REP INSB with CX=4 advances DI by four.
    let (cpu, _) = run_io(
        |cpu, _| {
            cpu.regs.cx = 4;
            cpu.regs.es = 0;
            cpu.regs.di = 0x0500;
        },
        &[0xF3, 0x6C],
    );
    assert_eq!(cpu.regs.di, 0x0504);
}
