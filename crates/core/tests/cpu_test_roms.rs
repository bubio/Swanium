//! Integration tests: self-built V30MZ machine-code programs executed on the
//! full Bus + Cpu stack.
//!
//! # Harness design
//!
//! Each test places a short byte sequence at ROM bank 0, offset 0 (physical
//! address 0x20000, reached via CS=0x2000, IP=0x0000).  The program writes its
//! result(s) to DS:0x0000 (physical 0x00000 = WRAM) and terminates with HLT.
//! The harness reads WRAM back after execution to verify the result.
//!
//! # ROM bank 0 setup
//!
//! `Cartridge` initialises `rom_bank0 = 0xFF`.  Writing 0x00 to I/O port 0xC2
//! sets `rom_bank0 = 0`, so the bus maps ROM[0] to physical 0x20000.
//!
//! # Assembly mnemonics
//!
//! Comments inside each test use Intel/NASM-style mnemonics so the byte
//! sequences can be verified against an 8086/V30MZ reference assembler.

use swanium_core::bus::Bus;
use swanium_core::cpu::{Cpu, MemoryBus};

// ── Harness ──────────────────────────────────────────────────────────────────

/// Loads `code` into a 64 KiB ROM starting at offset 0, maps it to physical
/// address 0x20000 (via ROM bank 0 = 0), resets the CPU to CS=0x2000 /
/// IP=0x0000, sets SP=0x3FFE (stack near top of WRAM) and runs until HLT or
/// `max_cycles` elapses.
fn run_code(code: &[u8], max_cycles: u32) -> (Cpu, Bus) {
    assert!(
        code.len() <= 0x10000,
        "code ({} bytes) exceeds one 64 KiB ROM bank",
        code.len()
    );
    let mut rom = vec![0u8; 0x10000];
    rom[..code.len()].copy_from_slice(code);

    let mut bus = Bus::new(rom);
    bus.write_io(0xC2, 0x00); // rom_bank0 = 0 → ROM[0] visible at physical 0x20000

    let mut cpu = Cpu::new();
    cpu.reset(0x2000, 0x0000); // CS=0x2000, IP=0x0000 → physical 0x20000
    cpu.regs.sp = 0x3FFE; // stack near top of accessible WRAM (0x0000–0x3FFF)

    let mut cycles = 0u32;
    while !cpu.halted && cycles < max_cycles {
        cycles += cpu.step(&mut bus);
    }
    (cpu, bus)
}

// ── Arithmetic ───────────────────────────────────────────────────────────────

#[test]
fn add_stores_sum_to_wram() {
    #[rustfmt::skip]
    let code = [
        0xB8, 0x05, 0x00, // MOV AX, 5
        0xBB, 0x03, 0x00, // MOV BX, 3
        0x01, 0xD8,       // ADD AX, BX
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 8);
}

#[test]
fn sub_of_equal_values_produces_zero_in_wram() {
    #[rustfmt::skip]
    let code = [
        0xB8, 0x34, 0x12, // MOV AX, 0x1234
        0x2D, 0x34, 0x12, // SUB AX, 0x1234
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 0);
}

#[test]
fn imul_word_stores_product_to_wram() {
    #[rustfmt::skip]
    let code = [
        0xB8, 0x06, 0x00, // MOV AX, 6
        0xBB, 0x07, 0x00, // MOV BX, 7
        0xF7, 0xEB,       // IMUL BX  (AX = 6 * 7 = 42; DX:AX, DX=0 for small values)
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 42);
}

// ── Control flow ─────────────────────────────────────────────────────────────

#[test]
fn loop_instruction_executes_body_cx_times() {
    // MOV CX, 5 ; XOR AX, AX ; [loop:] INC AX ; LOOP loop ; MOV [0], AX ; HLT
    // After 5 iterations: AX = 5.
    //
    // IP layout (from ROM offset 0):
    //   0: B9 05 00   MOV CX, 5
    //   3: 31 C0      XOR AX, AX
    //   5: 40         INC AX        ← loop target (next IP after fetch = 6)
    //   6: E2 FD      LOOP -3       (next IP=8; target=8-3=5) ✓
    //   8: A3 00 00   MOV [0x0000], AX
    //  11: F4         HLT
    #[rustfmt::skip]
    let code = [
        0xB9, 0x05, 0x00, // MOV CX, 5
        0x31, 0xC0,       // XOR AX, AX
        0x40,             // INC AX
        0xE2, 0xFD,       // LOOP -3
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 10_000);
    assert_eq!(bus.read_u16(0x00000), 5);
}

#[test]
fn jz_taken_when_zero_flag_set() {
    // XOR AX, AX sets ZF=1; JZ +2 skips MOV AX, 0xFF.
    // Expected: AX = 0 stored to WRAM (the MOV was skipped).
    //
    //   0: 31 C0      XOR AX, AX        ZF=1
    //   2: 74 03      JZ +3             → IP = 2+2+3 = 7 (skip next MOV)
    //   4: B8 FF 00   MOV AX, 0x00FF    (skipped)
    //   7: A3 00 00   MOV [0x0000], AX
    //  10: F4         HLT
    #[rustfmt::skip]
    let code = [
        0x31, 0xC0,       // XOR AX, AX
        0x74, 0x03,       // JZ +3
        0xB8, 0xFF, 0x00, // MOV AX, 0x00FF  (skipped)
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 0);
}

#[test]
fn jnz_not_taken_when_zero_flag_set() {
    // XOR AX, AX → ZF=1; JNZ should NOT jump; execution falls through to HLT.
    // MOV [0x0000], AX writes 0.
    //
    //   0: 31 C0      XOR AX, AX
    //   2: 75 03      JNZ +3            not taken (ZF=1)
    //   4: A3 00 00   MOV [0x0000], AX
    //   7: F4         HLT
    //   8: B8 FF 00   (unreachable)
    #[rustfmt::skip]
    let code = [
        0x31, 0xC0,       // XOR AX, AX
        0x75, 0x03,       // JNZ +3  (not taken)
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
        0xB8, 0xFF, 0x00, // MOV AX, 0xFF (unreachable)
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 0);
}

// ── Stack ─────────────────────────────────────────────────────────────────────

#[test]
fn push_pop_round_trips_value_through_wram_stack() {
    // Stack is at SS:SP = 0x0000:0x3FFE (set by run_code harness).
    //
    //   0: B8 AD DE   MOV AX, 0xDEAD
    //   3: 50         PUSH AX
    //   4: 31 C0      XOR AX, AX
    //   6: 58         POP AX
    //   7: A3 00 00   MOV [0x0000], AX
    //  10: F4         HLT
    #[rustfmt::skip]
    let code = [
        0xB8, 0xAD, 0xDE, // MOV AX, 0xDEAD
        0x50,             // PUSH AX
        0x31, 0xC0,       // XOR AX, AX
        0x58,             // POP AX
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 0xDEAD);
}

// ── String instructions ───────────────────────────────────────────────────────

#[test]
fn rep_stosb_fills_four_bytes_in_wram() {
    // ES = DS = 0x0000 (default); DI = 0 → ES:DI = physical 0x00000 = WRAM.
    // After REP STOSB with CX=4, AL=0xAB: WRAM[0..3] = 0xAB.
    //
    //   0: B9 04 00   MOV CX, 4
    //   3: B0 AB      MOV AL, 0xAB
    //   5: 31 FF      XOR DI, DI
    //   7: F3 AA      REP STOSB
    //   9: F4         HLT
    #[rustfmt::skip]
    let code = [
        0xB9, 0x04, 0x00, // MOV CX, 4
        0xB0, 0xAB,       // MOV AL, 0xAB
        0x31, 0xFF,       // XOR DI, DI
        0xF3, 0xAA,       // REP STOSB
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 10_000);
    // Check all four filled bytes and that the fifth is untouched.
    for i in 0u32..4 {
        assert_eq!(bus.read_u8(i), 0xAB, "WRAM[{i}] should be 0xAB");
    }
    assert_eq!(bus.read_u8(4), 0x00);
}

#[test]
fn rep_movsb_copies_bytes_within_wram() {
    // Copy 3 bytes from DS:0x0010 to ES:0x0020 (both in WRAM).
    // Source bytes are seeded with MOV [imm], AL before the copy.
    //
    //   0: B0 AA      MOV AL, 0xAA
    //   2: A2 10 00   MOV [0x0010], AL
    //   5: B0 BB      MOV AL, 0xBB
    //   7: A2 11 00   MOV [0x0011], AL
    //  10: B0 CC      MOV AL, 0xCC
    //  12: A2 12 00   MOV [0x0012], AL
    //  15: B9 03 00   MOV CX, 3
    //  18: BE 10 00   MOV SI, 0x0010
    //  21: BF 20 00   MOV DI, 0x0020
    //  24: F3 A4      REP MOVSB
    //  26: F4         HLT
    #[rustfmt::skip]
    let code = [
        0xB0, 0xAA,       // MOV AL, 0xAA
        0xA2, 0x10, 0x00, // MOV [0x0010], AL
        0xB0, 0xBB,       // MOV AL, 0xBB
        0xA2, 0x11, 0x00, // MOV [0x0011], AL
        0xB0, 0xCC,       // MOV AL, 0xCC
        0xA2, 0x12, 0x00, // MOV [0x0012], AL
        0xB9, 0x03, 0x00, // MOV CX, 3
        0xBE, 0x10, 0x00, // MOV SI, 0x0010
        0xBF, 0x20, 0x00, // MOV DI, 0x0020
        0xF3, 0xA4,       // REP MOVSB
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 10_000);
    assert_eq!(bus.read_u8(0x00020), 0xAA);
    assert_eq!(bus.read_u8(0x00021), 0xBB);
    assert_eq!(bus.read_u8(0x00022), 0xCC);
    assert_eq!(bus.read_u8(0x00023), 0x00); // not overwritten
}

// ── Carry / borrow propagation ────────────────────────────────────────────────

#[test]
fn adc_includes_carry_from_previous_overflow() {
    // ADD 0xFFFF + 1 → AX=0, CF=1; then ADC AX, 2 → AX=0+2+1=3.
    //
    //   0: B8 FF FF   MOV AX, 0xFFFF
    //   3: BB 01 00   MOV BX, 1
    //   6: 01 D8      ADD AX, BX      CF=1, AX=0
    //   8: BB 02 00   MOV BX, 2
    //  11: 11 D8      ADC AX, BX      AX = 0+2+CF(1) = 3
    //  13: A3 00 00   MOV [0x0000], AX
    //  16: F4         HLT
    #[rustfmt::skip]
    let code = [
        0xB8, 0xFF, 0xFF, // MOV AX, 0xFFFF
        0xBB, 0x01, 0x00, // MOV BX, 1
        0x01, 0xD8,       // ADD AX, BX   → AX=0, CF=1
        0xBB, 0x02, 0x00, // MOV BX, 2
        0x11, 0xD8,       // ADC AX, BX   → AX = 0+2+1 = 3
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 3);
}

#[test]
fn sbb_subtracts_borrow_from_previous_underflow() {
    // SUB 0 - 1 → AX=0xFFFF, CF=1; then SBB AX, DX(=0) → AX=0xFFFF-0-CF(1)=0xFFFE.
    // DX is zeroed BEFORE the SUB so that no instruction between SUB and SBB
    // touches CF (XOR would clear CF).
    //
    //   0: 31 C0      XOR AX, AX      AX=0, CF=0
    //   2: BB 01 00   MOV BX, 1
    //   5: 31 D2      XOR DX, DX      DX=0, CF=0  (before SUB sets CF)
    //   7: 29 D8      SUB AX, BX      AX=0xFFFF, CF=1
    //   9: 19 D0      SBB AX, DX      AX = 0xFFFF-0-CF(1) = 0xFFFE
    //  11: A3 00 00   MOV [0x0000], AX
    //  14: F4         HLT
    #[rustfmt::skip]
    let code = [
        0x31, 0xC0,       // XOR AX, AX
        0xBB, 0x01, 0x00, // MOV BX, 1
        0x31, 0xD2,       // XOR DX, DX  (zero DX before SUB sets CF)
        0x29, 0xD8,       // SUB AX, BX  → AX=0xFFFF, CF=1
        0x19, 0xD0,       // SBB AX, DX  → AX=0xFFFF-0-CF(1)=0xFFFE
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 0xFFFE);
}

// ── Bitwise / logical ─────────────────────────────────────────────────────────

#[test]
fn neg_produces_twos_complement() {
    // NEG AX: AX = -5 (two's complement) = 0xFFFB.
    //
    //   0: B8 05 00   MOV AX, 5
    //   3: F7 D8      NEG AX          AX = 0xFFFB
    //   5: A3 00 00   MOV [0x0000], AX
    //   8: F4         HLT
    #[rustfmt::skip]
    let code = [
        0xB8, 0x05, 0x00, // MOV AX, 5
        0xF7, 0xD8,       // NEG AX
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 0xFFFBu16);
}

#[test]
fn and_preserves_common_bits_only() {
    // 0x0FFF & 0xF0F0 = 0x00F0.
    //
    //   0: B8 FF 0F   MOV AX, 0x0FFF
    //   3: BB F0 F0   MOV BX, 0xF0F0
    //   6: 21 D8      AND AX, BX
    //   8: A3 00 00   MOV [0x0000], AX
    //  11: F4         HLT
    #[rustfmt::skip]
    let code = [
        0xB8, 0xFF, 0x0F, // MOV AX, 0x0FFF
        0xBB, 0xF0, 0xF0, // MOV BX, 0xF0F0
        0x21, 0xD8,       // AND AX, BX
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 0x00F0);
}

// ── CMP ───────────────────────────────────────────────────────────────────────

#[test]
fn cmp_does_not_modify_destination_register() {
    // CMP AX, BX flags the subtraction but must leave AX unchanged.
    //
    //   0: B8 2A 00   MOV AX, 42
    //   3: BB 2A 00   MOV BX, 42
    //   6: 3B C3      CMP AX, BX      flags set; AX unchanged
    //   8: A3 00 00   MOV [0x0000], AX
    //  11: F4         HLT
    #[rustfmt::skip]
    let code = [
        0xB8, 0x2A, 0x00, // MOV AX, 42
        0xBB, 0x2A, 0x00, // MOV BX, 42
        0x3B, 0xC3,       // CMP AX, BX
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 42);
}

// ── CALL / RET ────────────────────────────────────────────────────────────────

#[test]
fn call_near_jumps_to_subroutine_and_ret_returns_to_caller() {
    // CALL rel16 pushes the return address and jumps to the subroutine.
    // The subroutine loads AX=0xCDAB and returns.
    //
    // ROM layout (IP from offset 0):
    //   0: E8 04 00   CALL +4  → push 3, jump to 7
    //   3: A3 00 00   MOV [0x0000], AX   (executed after RET)
    //   6: F4         HLT
    //   7: B8 AB CD   MOV AX, 0xCDAB
    //  10: C3         RET  → pop 3, jump to 3
    #[rustfmt::skip]
    let code = [
        0xE8, 0x04, 0x00, // CALL +4   (next IP=3, target=3+4=7)
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
        0xB8, 0xAB, 0xCD, // MOV AX, 0xCDAB
        0xC3,             // RET
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 0xCDAB);
}

// ── Segment registers ─────────────────────────────────────────────────────────

#[test]
fn mov_sreg_round_trips_value() {
    // MOV DS, AX loads DS; MOV AX, DS reads it back.
    // DS is restored to 0 before the final write so MOV [0x0000], AX uses DS=0.
    //
    //   0: B8 AB CD   MOV AX, 0xCDAB
    //   3: 8E D8      MOV DS, AX          DS = 0xCDAB
    //   5: 8C D8      MOV AX, DS          AX = DS = 0xCDAB
    //   7: 31 D2      XOR DX, DX          DX = 0
    //   9: 8E DA      MOV DS, DX          DS = 0  (restore for write)
    //  11: A3 00 00   MOV [0x0000], AX
    //  14: F4         HLT
    #[rustfmt::skip]
    let code = [
        0xB8, 0xAB, 0xCD, // MOV AX, 0xCDAB
        0x8E, 0xD8,       // MOV DS, AX
        0x8C, 0xD8,       // MOV AX, DS
        0x31, 0xD2,       // XOR DX, DX
        0x8E, 0xDA,       // MOV DS, DX
        0xA3, 0x00, 0x00, // MOV [0x0000], AX
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u16(0x00000), 0xCDAB);
}

// ── BCD adjustment ────────────────────────────────────────────────────────────

#[test]
fn daa_adjusts_bcd_addition_to_packed_decimal() {
    // BCD addition: 8 + 4 = 0x0C (binary); DAA corrects it to 0x12 (BCD).
    // DAA adds 6 to AL when the lower nibble exceeds 9 (or AF is set).
    //   0x0C + 6 = 0x12, which is BCD for decimal 12. ✓
    //
    //   0: B0 08      MOV AL, 8
    //   2: B3 04      MOV BL, 4
    //   4: 00 D8      ADD AL, BL      AL=0x0C (binary)
    //   6: 27         DAA             AL=0x12 (BCD)
    //   7: A2 00 00   MOV [0x0000], AL
    //  10: F4         HLT
    #[rustfmt::skip]
    let code = [
        0xB0, 0x08,       // MOV AL, 8
        0xB3, 0x04,       // MOV BL, 4
        0x00, 0xD8,       // ADD AL, BL
        0x27,             // DAA
        0xA2, 0x00, 0x00, // MOV [0x0000], AL
        0xF4,             // HLT
    ];
    let (_, bus) = run_code(&code, 1_000);
    assert_eq!(bus.read_u8(0x00000), 0x12);
}

// ── Halt ─────────────────────────────────────────────────────────────────────

#[test]
fn hlt_instruction_stops_execution() {
    let code = [0xF4]; // HLT
    let (cpu, _) = run_code(&code, 1_000);
    assert!(cpu.halted);
}
