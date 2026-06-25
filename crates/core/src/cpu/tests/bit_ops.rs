use super::run_with;

#[test]
fn shl_rm8_by_1_sets_carry_from_vacated_bit() {
    // SHL AL, 1 (0xD0, modrm = 11 100 000 = 0xE0). AL = 0x81 -> 0x02, CF=1.
    let (cpu, cycles, _) = run_with(|cpu| cpu.regs.ax = 0x0081, &[0xD0, 0xE0]);
    assert_eq!(cpu.regs.ax & 0xFF, 0x02);
    assert!(cpu.flags.carry);
    assert_eq!(cycles, 4);
}

#[test]
fn shr_rm16_by_cl_shifts_multiple_bits() {
    // SHR AX, CL (0xD3, modrm = 11 101 000 = 0xE8). AX=0x0010, CL=4 -> 0x0001.
    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x0010;
            cpu.regs.cx = 0x0004;
        },
        &[0xD3, 0xE8],
    );
    assert_eq!(cpu.regs.ax, 0x0001);
}

#[test]
fn shift_by_zero_leaves_flags_untouched() {
    // SHL AX, CL (0xD3, modrm=11 100 000=0xE0) with CL=0: no-op per 8086 spec.
    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x1234;
            cpu.regs.cx = 0x0000;
            cpu.flags.carry = true;
        },
        &[0xD3, 0xE0],
    );
    assert_eq!(cpu.regs.ax, 0x1234);
    assert!(cpu.flags.carry, "count=0 must not touch flags");
}

#[test]
fn rol_rm8_by_1_wraps_msb_into_carry_and_lsb() {
    // ROL AL, 1 (0xD0, modrm=11 000 000=0xC0). AL=0x81 -> 0x03, CF=1.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0x0081, &[0xD0, 0xC0]);
    assert_eq!(cpu.regs.ax & 0xFF, 0x03);
    assert!(cpu.flags.carry);
}

#[test]
fn sar_preserves_sign_bit() {
    // SAR AL, 1 (0xD0, modrm=11 111 000=0xF8). AL=0x80 -> 0xC0 (sign-extended).
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0x0080, &[0xD0, 0xF8]);
    assert_eq!(cpu.regs.ax & 0xFF, 0xC0);
    assert!(!cpu.flags.carry);
}

#[test]
fn xchg_rm16_register_form_swaps_values() {
    // XCHG BX, AX (0x87, modrm=11 000 011=0xC3, reg=AX, rm=BX).
    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x1111;
            cpu.regs.bx = 0x2222;
        },
        &[0x87, 0xC3],
    );
    assert_eq!(cpu.regs.ax, 0x2222);
    assert_eq!(cpu.regs.bx, 0x1111);
}

#[test]
fn xchg_ax_with_register_short_form() {
    // XCHG CX, AX (0x91).
    let (cpu, cycles, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0xAAAA;
            cpu.regs.cx = 0xBBBB;
        },
        &[0x91],
    );
    assert_eq!(cpu.regs.ax, 0xBBBB);
    assert_eq!(cpu.regs.cx, 0xAAAA);
    assert_eq!(cycles, 3);
}

#[test]
fn test_al_imm8_does_not_modify_register() {
    // TEST AL, 0x0F ; AL = 0xF0 -> ZF=1, AL unchanged.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0x00F0, &[0xA8, 0x0F]);
    assert_eq!(cpu.regs.ax & 0xFF, 0xF0);
    assert!(cpu.flags.zero);
}

#[test]
fn not_rm8_complements_bits_without_touching_flags() {
    // NOT AL (0xF6, modrm=11 010 000=0xD0). AL=0x0F -> 0xF0. CF must be untouched.
    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x000F;
            cpu.flags.carry = true;
        },
        &[0xF6, 0xD0],
    );
    assert_eq!(cpu.regs.ax & 0xFF, 0xF0);
    assert!(cpu.flags.carry, "NOT must not modify flags");
}

#[test]
fn neg_rm8_sets_carry_unless_operand_is_zero() {
    // NEG AL (0xF6, modrm=11 011 000=0xD8). AL=0x01 -> 0xFF, CF=1.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0x0001, &[0xF6, 0xD8]);
    assert_eq!(cpu.regs.ax & 0xFF, 0xFF);
    assert!(cpu.flags.carry);
}

#[test]
fn mul_rm8_sets_carry_and_overflow_when_result_overflows_al() {
    // MUL AL, with AL preset and rm8 = CL (0xF6, modrm=11 100 001=0xE1).
    let (cpu, cycles, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x0010; // AL = 0x10
            cpu.regs.cx = 0x0010; // CL = 0x10
        },
        &[0xF6, 0xE1],
    );
    assert_eq!(cpu.regs.ax, 0x0100);
    assert!(cpu.flags.carry);
    assert!(cpu.flags.overflow);
    assert_eq!(cycles, 72);
}

#[test]
fn div_rm8_computes_quotient_and_remainder() {
    // DIV CL (0xF6, modrm=11 110 001=0xF1). AX=0x000A / CL=0x03 -> AL=3,AH=1.
    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x000A;
            cpu.regs.cx = 0x0003;
        },
        &[0xF6, 0xF1],
    );
    assert_eq!(cpu.regs.ax, 0x0103);
}

#[test]
#[should_panic(expected = "INT0")]
fn div_rm8_by_zero_panics_pending_int0_support() {
    let _ = run_with(
        |cpu| {
            cpu.regs.ax = 0x000A;
            cpu.regs.cx = 0x0000;
        },
        &[0xF6, 0xF1],
    );
}

#[test]
fn loop_decrements_cx_and_branches_while_nonzero() {
    // LOOP -0x02 (0xE2 0xFE): branch back to the start of this instruction.
    let (cpu, cycles, _) = run_with(|cpu| cpu.regs.cx = 0x0002, &[0xE2, 0xFE]);
    assert_eq!(cpu.regs.cx, 1);
    assert_eq!(cpu.regs.ip, 0);
    assert_eq!(cycles, 17);
}

#[test]
fn loop_does_not_branch_when_cx_reaches_zero() {
    let (cpu, cycles, _) = run_with(|cpu| cpu.regs.cx = 0x0001, &[0xE2, 0xFE]);
    assert_eq!(cpu.regs.cx, 0);
    assert_eq!(cpu.regs.ip, 2);
    assert_eq!(cycles, 5);
}

#[test]
fn jcxz_branches_when_cx_is_zero() {
    let (cpu, _, _) = run_with(|cpu| cpu.regs.cx = 0x0000, &[0xE3, 0x04]);
    assert_eq!(cpu.regs.ip, 6);
}

#[test]
fn cbw_sign_extends_negative_al_into_ah() {
    // CBW (0x98). AL = 0x80 -> AX = 0xFF80.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0x0080, &[0x98]);
    assert_eq!(cpu.regs.ax, 0xFF80);
}

#[test]
fn cwd_sign_extends_negative_ax_into_dx() {
    // CWD (0x99). AX = 0x8000 -> DX = 0xFFFF.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0x8000, &[0x99]);
    assert_eq!(cpu.regs.dx, 0xFFFF);
}

#[test]
fn lahf_then_sahf_round_trips_flag_byte() {
    use super::Cpu;
    let mut mem = super::FlatMemory::new();
    mem.load(0, &[0x9F, 0x9E]); // LAHF ; SAHF
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.flags.carry = true;
    cpu.flags.zero = true;
    cpu.step(&mut mem); // LAHF
    cpu.flags.carry = false;
    cpu.flags.zero = false;
    cpu.step(&mut mem); // SAHF restores from AH
    assert!(cpu.flags.carry);
    assert!(cpu.flags.zero);
}

#[test]
fn pushf_then_popf_round_trips_flags() {
    use super::Cpu;
    let mut mem = super::FlatMemory::new();
    mem.load(0, &[0x9C, 0x9D]); // PUSHF ; POPF
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFE;
    cpu.flags.overflow = true;
    cpu.flags.sign = true;
    cpu.step(&mut mem); // PUSHF
    cpu.flags.overflow = false;
    cpu.flags.sign = false;
    cpu.step(&mut mem); // POPF
    assert!(cpu.flags.overflow);
    assert!(cpu.flags.sign);
    assert_eq!(cpu.regs.sp, 0xFFFE);
}

#[test]
fn xlat_reads_byte_at_ds_bx_plus_al() {
    use super::Cpu;
    let mut mem = super::FlatMemory::new();
    mem.load(0, &[0xD7]); // XLAT
    mem.load(0x0103, &[0x42]);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ds = 0;
    cpu.regs.bx = 0x0100;
    cpu.regs.ax = 0x0003;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.ax & 0xFF, 0x42);
}
