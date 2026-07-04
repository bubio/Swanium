use super::{run_with, FlatMemory};
use crate::cpu::timing::IRQ_ACK;
use crate::cpu::Cpu;

#[test]
fn jmp_short_advances_ip_by_signed_offset() {
    // JMP short +0x05 (0xEB 0x05). After fetching the 2-byte instruction,
    // IP=2, so the landing address is 2 + 5 = 7.
    let (cpu, cycles, _) = run_with(|_| {}, &[0xEB, 0x05]);
    assert_eq!(cpu.regs.ip, 7);
    assert_eq!(cycles, 4);
}

#[test]
fn jmp_short_negative_offset_wraps_backward() {
    // JMP short -0x02 (0xEB 0xFE). Landing address is 2 + (-2) = 0.
    let (cpu, _, _) = run_with(|_| {}, &[0xEB, 0xFE]);
    assert_eq!(cpu.regs.ip, 0);
}

#[test]
fn jz_taken_when_zero_flag_set() {
    // JZ +0x03 (0x74 0x03), taken.
    let (cpu, cycles, _) = run_with(|cpu| cpu.flags.zero = true, &[0x74, 0x03]);
    assert_eq!(cpu.regs.ip, 5);
    assert_eq!(cycles, 4);
}

#[test]
fn jz_not_taken_when_zero_flag_clear() {
    // JZ +0x03 (0x74 0x03), not taken: IP only advances past the instruction.
    let (cpu, cycles, _) = run_with(|cpu| cpu.flags.zero = false, &[0x74, 0x03]);
    assert_eq!(cpu.regs.ip, 2);
    assert_eq!(cycles, 1);
}

#[test]
fn jl_uses_sign_xor_overflow() {
    // JL +0x03 (0x7C 0x03): taken when SF != OF.
    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.flags.sign = true;
            cpu.flags.overflow = false;
        },
        &[0x7C, 0x03],
    );
    assert_eq!(cpu.regs.ip, 5);
}

#[test]
fn call_pushes_return_address_and_jumps() {
    use super::MemoryBus;
    // CALL +0x10 (0xE8 0x10 0x00). Return address pushed is IP after the
    // 3-byte instruction (3), and the new IP is 3 + 0x10 = 0x13.
    let (cpu, cycles, mem) = run_with(|_| {}, &[0xE8, 0x10, 0x00]);
    assert_eq!(cpu.regs.ip, 0x13);
    assert_eq!(cpu.regs.sp, 0xFFFC);
    assert_eq!(mem.read_u16(0xFFFC), 3);
    assert_eq!(cycles, 5);
}

#[test]
fn ret_pops_return_address() {
    use super::{Cpu, FlatMemory};
    let mut mem = FlatMemory::new();
    // CALL +0x10 ; at the target, RET.
    mem.load(0, &[0xE8, 0x10, 0x00]);
    mem.load(0x13, &[0xC3]);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFE;
    cpu.step(&mut mem); // CALL
    assert_eq!(cpu.regs.ip, 0x13);
    cpu.step(&mut mem); // RET
    assert_eq!(cpu.regs.ip, 3);
    assert_eq!(cpu.regs.sp, 0xFFFE);
}

#[test]
fn hlt_sets_halted_and_subsequent_steps_are_idempotent() {
    let (mut cpu, cycles, _) = run_with(|_| {}, &[0xF4]);
    assert!(cpu.halted);
    assert_eq!(cycles, 9);
    let mut mem = super::FlatMemory::new();
    let again = cpu.step(&mut mem);
    assert_eq!(again, 1);
    assert!(cpu.halted);
}

#[test]
fn flag_instructions_clc_stc_cli_sti_cld_std() {
    let (cpu, _, _) = run_with(|cpu| cpu.flags.carry = true, &[0xF8]); // CLC
    assert!(!cpu.flags.carry);

    let (cpu, _, _) = run_with(|cpu| cpu.flags.carry = false, &[0xF9]); // STC
    assert!(cpu.flags.carry);

    let (cpu, _, _) = run_with(|cpu| cpu.flags.interrupt = true, &[0xFA]); // CLI
    assert!(!cpu.flags.interrupt);

    let (cpu, _, _) = run_with(|cpu| cpu.flags.interrupt = false, &[0xFB]); // STI
    assert!(cpu.flags.interrupt);

    let (cpu, _, _) = run_with(|cpu| cpu.flags.direction = true, &[0xFC]); // CLD
    assert!(!cpu.flags.direction);

    let (cpu, _, _) = run_with(|cpu| cpu.flags.direction = false, &[0xFD]); // STD
    assert!(cpu.flags.direction);
}

#[test]
fn handle_irq_dispatches_and_reports_acknowledge_cost() {
    // Interrupt vector 0x20 → handler at 0x1234:0x5678 (IVT entry at 0x20*4).
    let mut mem = FlatMemory::new();
    mem.load(0x80, &[0x78, 0x56, 0x34, 0x12]);
    let mut cpu = Cpu::new();
    cpu.reset(0x1000, 0x2000);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFE;
    cpu.flags.interrupt = true;

    let cost = cpu.handle_irq(&mut mem, 0x20);

    // The acknowledge/dispatch cost is what `System::run_cpu_cycles` bills a
    // maskable IRQ; the fix for FF4's black-screen relied on this being a small
    // V30 value rather than the 8086's ~32/51.
    assert_eq!(cost, IRQ_ACK);
    // Jumped to the vector, cleared IF, and pushed FLAGS/CS/IP (SP -= 6).
    assert_eq!((cpu.regs.cs, cpu.regs.ip), (0x1234, 0x5678));
    assert!(!cpu.flags.interrupt);
    assert_eq!(cpu.regs.sp, 0xFFF8);
}

#[test]
fn software_int_reports_full_cost_without_double_counting_acknowledge() {
    // INT 0x21 (0xCD 0x21): the opcode reports its own total cost and ignores
    // handle_irq's return, so it is not double-charged the acknowledge cost.
    let (_, cycles, _) = run_with(|_| {}, &[0xCD, 0x21]);
    assert_eq!(cycles, 10);
}
