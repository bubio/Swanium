use super::run_with;

#[test]
fn add_al_imm8_sets_carry_and_aux_carry() {
    // ADD AL, 0x05 ; AL = 0xFE -> 0x03 with CF=1, AF=1, OF=0
    let (cpu, cycles, _) = run_with(|cpu| cpu.regs.ax = 0x00FE, &[0x04, 0x05]);
    assert_eq!(cpu.regs.ax & 0xFF, 0x03);
    assert!(cpu.flags.carry);
    assert!(cpu.flags.aux_carry);
    assert!(!cpu.flags.overflow);
    assert!(!cpu.flags.zero);
    assert!(!cpu.flags.sign);
    assert!(cpu.flags.parity); // 0x03 has even popcount
    assert_eq!(cycles, 1);
}

#[test]
fn add_al_imm8_signed_overflow() {
    // ADD AL, 0x01 ; AL = 0x7F -> 0x80, OF=1 (positive + positive = negative)
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0x007F, &[0x04, 0x01]);
    assert_eq!(cpu.regs.ax & 0xFF, 0x80);
    assert!(cpu.flags.overflow);
    assert!(cpu.flags.sign);
    assert!(!cpu.flags.carry);
}

#[test]
fn sub_al_imm8_borrows() {
    // SUB AL, 0x01 ; AL = 0x00 -> 0xFF, CF=1 (borrow), AF=1, OF=0
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0x0000, &[0x2C, 0x01]);
    assert_eq!(cpu.regs.ax & 0xFF, 0xFF);
    assert!(cpu.flags.carry);
    assert!(cpu.flags.aux_carry);
    assert!(!cpu.flags.overflow);
    assert!(cpu.flags.sign);
}

#[test]
fn cmp_al_imm8_does_not_modify_register() {
    // CMP AL, 0x05 ; AL = 0x05 -> ZF=1, AL unchanged
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0x0005, &[0x3C, 0x05]);
    assert_eq!(cpu.regs.ax & 0xFF, 0x05);
    assert!(cpu.flags.zero);
    assert!(!cpu.flags.carry);
}

#[test]
fn and_ax_imm16_clears_carry_and_overflow() {
    // AND AX, 0x0F0F ; AX = 0xFFFF -> 0x0F0F, CF=0, OF=0
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0xFFFF, &[0x25, 0x0F, 0x0F]);
    assert_eq!(cpu.regs.ax, 0x0F0F);
    assert!(!cpu.flags.carry);
    assert!(!cpu.flags.overflow);
}

#[test]
fn or_ax_imm16_sets_zero_flag_when_result_is_zero() {
    // OR AX, 0x0000 ; AX = 0x0000 -> ZF=1
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0x0000, &[0x0D, 0x00, 0x00]);
    assert!(cpu.flags.zero);
}

#[test]
fn xor_ax_imm16_self_clears_register() {
    // XOR AX, 0xFFFF ; AX = 0xFFFF -> 0x0000
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0xFFFF, &[0x35, 0xFF, 0xFF]);
    assert_eq!(cpu.regs.ax, 0x0000);
    assert!(cpu.flags.zero);
}

#[test]
fn inc_ax_sets_overflow_at_signed_boundary_but_not_carry() {
    // INC AX ; AX = 0x7FFF -> 0x8000, OF=1, CF must stay whatever it was (true here)
    let (cpu, cycles, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x7FFF;
            cpu.flags.carry = true;
        },
        &[0x40],
    );
    assert_eq!(cpu.regs.ax, 0x8000);
    assert!(cpu.flags.overflow);
    assert!(cpu.flags.aux_carry);
    assert!(cpu.flags.carry, "INC must not modify CF");
    assert_eq!(cycles, 1);
}

#[test]
fn dec_ax_sets_overflow_at_signed_boundary_but_not_carry() {
    // DEC AX ; AX = 0x8000 -> 0x7FFF, OF=1, CF must stay whatever it was (false here)
    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x8000;
            cpu.flags.carry = false;
        },
        &[0x48],
    );
    assert_eq!(cpu.regs.ax, 0x7FFF);
    assert!(cpu.flags.overflow);
    assert!(!cpu.flags.carry, "DEC must not modify CF");
}

#[test]
fn add_rm16_r16_register_form() {
    // ADD BX, AX ; modrm = 11 000 011 (reg=AX, rm=BX, mod=11)
    let (cpu, cycles, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x0010;
            cpu.regs.bx = 0x0005;
        },
        &[0x01, 0xC3],
    );
    assert_eq!(cpu.regs.bx, 0x0015);
    assert_eq!(cpu.regs.ax, 0x0010, "source register must be unchanged");
    assert_eq!(cycles, 1);
}
