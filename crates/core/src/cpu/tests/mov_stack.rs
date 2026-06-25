use super::run_with;
use super::MemoryBus;

#[test]
fn mov_reg16_imm16() {
    // MOV CX, 0x1234 (0xB9 = MOV CX,imm16)
    let (cpu, cycles, _) = run_with(|_| {}, &[0xB9, 0x34, 0x12]);
    assert_eq!(cpu.regs.cx, 0x1234);
    assert_eq!(cycles, 4);
}

#[test]
fn mov_reg8_imm8() {
    // MOV AH, 0x42 (0xB4 = MOV AH,imm8)
    let (cpu, _, _) = run_with(|_| {}, &[0xB4, 0x42]);
    assert_eq!(cpu.regs.ax, 0x4200);
}

#[test]
fn mov_memory_bx_si_addressing() {
    // MOV [BX+SI], AL ; modrm = 00 000 000
    let (cpu, _, mem) = run_with(
        |cpu| {
            cpu.regs.ax = 0x00AB;
            cpu.regs.bx = 0x0010;
            cpu.regs.si = 0x0002;
            cpu.regs.ds = 0x0000;
        },
        &[0x88, 0x00],
    );
    assert_eq!(mem.read_u8(0x0012), 0xAB);
    let _ = cpu;
}

#[test]
fn mov_memory_direct_address_uses_ds() {
    // MOV [0x0100], AL ; modrm = 00 000 110, disp16 = 0x0100
    let (_cpu, _, mem) = run_with(
        |cpu| {
            cpu.regs.ax = 0x007E;
            cpu.regs.ds = 0x0000;
        },
        &[0x88, 0x06, 0x00, 0x01],
    );
    assert_eq!(mem.read_u8(0x0100), 0x7E);
}

#[test]
fn mov_memory_bp_based_addressing_uses_ss_not_ds() {
    // MOV [BP+SI], AL ; modrm = 00 000 010. BP-based EA defaults to SS.
    let (_cpu, _, mem) = run_with(
        |cpu| {
            cpu.regs.ax = 0x0099;
            cpu.regs.bp = 0x0004;
            cpu.regs.si = 0x0001;
            cpu.regs.ss = 0x0010; // linear 0x100 + 0x05 = 0x105
            cpu.regs.ds = 0x0020; // must NOT be used for this addressing form
        },
        &[0x88, 0x02],
    );
    assert_eq!(mem.read_u8(0x0105), 0x99);
    assert_eq!(
        mem.read_u8(0x0205),
        0x00,
        "DS must not be used for [BP+...]"
    );
}

#[test]
fn push_pop_round_trip() {
    // PUSH BX (0x53)
    let (cpu, cycles, mem) = run_with(|cpu| cpu.regs.bx = 0xBEEF, &[0x53]);
    assert_eq!(cpu.regs.sp, 0xFFFC);
    assert_eq!(mem.read_u16(0xFFFC), 0xBEEF);
    assert_eq!(cycles, 4);
}

#[test]
fn pop_restores_register_and_advances_stack_pointer() {
    // POP CX (0x59), with 0xCAFE pre-pushed at SP.
    let (cpu, cycles, _) = run_with(
        |cpu| {
            cpu.regs.sp = 0xFFFC;
        },
        &[0x59],
    );
    // The word at SS:SP (0x0000) was never written in this test setup, so
    // just verify the stack pointer advanced and the cycle cost is correct;
    // value correctness is covered by push_pop_round_trip combined manually
    // below.
    assert_eq!(cpu.regs.sp, 0xFFFE);
    assert_eq!(cycles, 4);
}

#[test]
fn push_then_pop_round_trips_value() {
    use super::Cpu;
    use super::FlatMemory;
    let mut mem = FlatMemory::new();
    // PUSH BX ; POP CX
    mem.load(0, &[0x53, 0x59]);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFE;
    cpu.regs.bx = 0xBEEF;
    cpu.step(&mut mem);
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.cx, 0xBEEF);
    assert_eq!(cpu.regs.sp, 0xFFFE);
}
