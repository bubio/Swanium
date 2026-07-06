use super::super::MemoryBus;
use super::{run_with, Cpu, FlatMemory};

// ── Segment register PUSH / POP / MOV ────────────────────────────────────────

#[test]
fn push_es_pushes_segment_value() {
    // PUSH ES (0x06). ES = 0x1234.
    let (cpu, _, mem) = run_with(
        |cpu| {
            cpu.regs.es = 0x1234;
        },
        &[0x06],
    );
    // SP was 0xFFFE before PUSH → 0xFFFC after; value at SS:FFFC = 0x1234.
    assert_eq!(cpu.regs.sp, 0xFFFC);
    assert_eq!(mem.read_u16(0xFFFC), 0x1234);
}

#[test]
fn pop_ds_restores_segment_register() {
    // PUSH imm16 then POP DS: use PUSH AX / MOV AX,imm / ... workaround isn't
    // available yet; instead write directly to stack memory and use POP DS (0x1F).
    use super::super::super::cpu::bus::linear_address;
    let mut mem = FlatMemory::new();
    mem.load(0, &[0x1F]); // POP DS
    mem.write_u16(0xFFFC, 0x5678); // pre-load stack slot
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFC;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.ds, 0x5678);
    assert_eq!(cpu.regs.sp, 0xFFFE);
    let _ = linear_address; // suppress unused import lint
}

#[test]
fn mov_sreg_from_r16_updates_segment() {
    // MOV DS, AX (0x8E, modrm=11 011 000 = 0xD8). AX = 0xABCD.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0xABCD, &[0x8E, 0xD8]);
    assert_eq!(cpu.regs.ds, 0xABCD);
}

#[test]
fn mov_r16_from_sreg_reads_segment() {
    // MOV AX, ES (0x8C, modrm=11 000 000 = 0xC0). ES = 0x9900.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.es = 0x9900, &[0x8C, 0xC0]);
    assert_eq!(cpu.regs.ax, 0x9900);
}

// ── Segment override prefix ───────────────────────────────────────────────────

#[test]
fn es_override_redirects_memory_read_to_es_segment() {
    // ES: MOV AX, [0x0010]  →  read from ES:0010 instead of DS:0010
    // Encoding: 0x26 (ES:) 0xA1 (MOV AX,[imm16]) 0x10 0x00
    let mut mem = FlatMemory::new();
    mem.load(0, &[0x26, 0xA1, 0x10, 0x00]);
    mem.write_u16(0x0010, 0x1234); // DS:0010 = 0x1234
    mem.write_u16(0x1010, 0x5678); // ES:0010 = 0x5678 (ES=0x1000 → 0x1000<<4+0x10=0x10010 → wraps to 0x10010 & 0xFFFFF = 0x10010... wait let me recalculate)
                                   // ES=0x1000 → linear = 0x1000<<4 = 0x10000; 0x10000 + 0x10 = 0x10010.
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ds = 0x0000;
    cpu.regs.es = 0x1000;
    mem.write_u16(0x10010, 0x5678); // ES:0010 at physical 0x10010
    cpu.step(&mut mem);
    assert_eq!(
        cpu.regs.ax, 0x5678,
        "ES: override must redirect read to ES segment"
    );
}

#[test]
fn cs_override_on_modrm_memory_access() {
    // CS: MOV AL, [BX] must read from CS:BX, not DS:BX.
    // CS=0 (instruction fetch lives here). DS=0x0100 (distinct segment).
    // BX=0x0050. CS:BX = 0x0000:0x0050 = phys 0x0050.
    //             DS:BX = 0x0100:0x0050 = phys 0x1050.
    // CS cannot be changed while keeping instruction fetch valid; instead
    // differentiate via DS so the override is meaningful.
    let mut mem = FlatMemory::new();
    mem.load(0, &[0x2E, 0x8A, 0x07]); // CS: MOV AL,[BX]
    let mut cpu = Cpu::new();
    cpu.reset(0, 0); // CS=0, IP=0
    cpu.regs.ds = 0x0100;
    cpu.regs.bx = 0x0050;
    mem.write_u8(0x0050, 0xEE); // value at CS:BX (physical 0x0050)
    mem.write_u8(0x1050, 0xFF); // value at DS:BX (physical 0x1050) — wrong result
    cpu.step(&mut mem);
    assert_eq!(
        cpu.regs.get_reg8(0),
        0xEE,
        "CS: override must redirect read to CS segment"
    );
}

// ── MOV AL/AX memory-direct (0xA0–0xA3) ─────────────────────────────────────

#[test]
fn mov_al_mem_direct_reads_from_ds() {
    // MOV AL, [0x0050] (0xA0, 0x50, 0x00)
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xA0, 0x50, 0x00]);
    mem.write_u8(0x0050, 0x42);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ds = 0;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.get_reg8(0), 0x42);
}

#[test]
fn mov_mem_direct_ax_writes_to_ds() {
    // MOV [0x0060], AX (0xA3, 0x60, 0x00)
    let (_, _, mem) = run_with(|cpu| cpu.regs.ax = 0xBEEF, &[0xA3, 0x60, 0x00]);
    assert_eq!(mem.read_u16(0x0060), 0xBEEF);
}

// ── LEA ──────────────────────────────────────────────────────────────────────

#[test]
fn lea_loads_effective_address_not_memory_value() {
    // LEA BX, [BX+SI] (0x8D, modrm=00 011 000 = 0x18). BX=0x100, SI=0x050.
    // EA = BX + SI = 0x150. Memory at DS:0x150 could be anything; LEA must
    // NOT read from memory — it loads the address itself into BX.
    let mut mem = FlatMemory::new();
    mem.load(0, &[0x8D, 0x18]); // LEA BX,[BX+SI]
    mem.write_u16(0x0150, 0xDEAD); // would be read if this were a MOV
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.bx = 0x0100;
    cpu.regs.si = 0x0050;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.bx, 0x0150);
}

#[test]
fn lea_with_displacement() {
    // LEA AX, [BX+0x12] (0x8D, modrm=01 000 111=0x47, disp8=0x12).
    let (cpu, _, _) = run_with(|cpu| cpu.regs.bx = 0x0100, &[0x8D, 0x47, 0x12]);
    assert_eq!(cpu.regs.ax, 0x0112);
}

#[test]
fn wscputest_lea_register_mode_uses_extended_address_table() {
    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x1234;
            cpu.regs.bx = 0x5678;
        },
        &[0x8D, 0xC8], // LEA CX,[BX+AX]
    );
    assert_eq!(cpu.regs.cx, 0x68AC);

    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.bp = 0x1234;
            cpu.regs.dx = 0x5678;
        },
        &[0x8D, 0xCA], // LEA CX,[BP+DX]
    );
    assert_eq!(cpu.regs.cx, 0x68AC);
}

// ── LES / LDS ────────────────────────────────────────────────────────────────

#[test]
fn les_loads_offset_and_es_from_memory() {
    // LES BX, [0x0080] (0xC4, modrm=00 011 110=0x1E, disp16=0x0080)
    // Memory at DS:0x0080: offset=0x1234, at DS:0x0082: seg=0xABCD.
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xC4, 0x1E, 0x80, 0x00]); // LES BX,[0x0080]
    mem.write_u16(0x0080, 0x1234); // offset
    mem.write_u16(0x0082, 0xABCD); // segment
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ds = 0;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.bx, 0x1234);
    assert_eq!(cpu.regs.es, 0xABCD);
}

#[test]
fn wscputest_les_register_mode_uses_extended_address_table() {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xC4, 0xD8]); // LES BX,[DS:BX+AX]
    mem.write_u16(0x0200, 0xF0AB);
    mem.write_u16(0x0202, 0x570D);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFE;
    cpu.regs.ax = 0x0100;
    cpu.regs.bx = 0x0100;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.bx, 0xF0AB);
    assert_eq!(cpu.regs.es, 0x570D);
}

#[test]
fn lds_loads_offset_and_ds_from_memory() {
    // LDS SI, [0x0040] (0xC5, modrm=00 110 110=0x36, disp16=0x0040)
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xC5, 0x36, 0x40, 0x00]);
    mem.write_u16(0x0040, 0x5678);
    mem.write_u16(0x0042, 0x9ABC);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ds = 0;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.si, 0x5678);
    assert_eq!(cpu.regs.ds, 0x9ABC);
}

#[test]
fn wscputest_lds_register_mode_uses_extended_address_table() {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xC5, 0xDE]); // LDS BX,[SS:BP+SI]
    mem.write_u16(0x0300, 0xF0AB);
    mem.write_u16(0x0302, 0x570D);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFE;
    cpu.regs.bp = 0x0100;
    cpu.regs.si = 0x0200;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.bx, 0xF0AB);
    assert_eq!(cpu.regs.ds, 0x570D);
}

// ── ENTER / LEAVE ─────────────────────────────────────────────────────────────

#[test]
fn enter_creates_stack_frame_and_leave_tears_it_down() {
    let mut mem = FlatMemory::new();
    // ENTER 8, 0 (0xC8 0x08 0x00 0x00) ; LEAVE (0xC9)
    mem.load(0, &[0xC8, 0x08, 0x00, 0x00, 0xC9]);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0x0100;
    cpu.regs.bp = 0xBEEF;

    cpu.step(&mut mem); // ENTER 8, 0
                        // BP is saved, BP = old SP - 2, SP -= 8
    let saved_sp = 0x0100u16;
    assert_eq!(cpu.regs.bp, saved_sp.wrapping_sub(2));
    assert_eq!(cpu.regs.sp, saved_sp.wrapping_sub(2).wrapping_sub(8));
    assert_eq!(mem.read_u16((saved_sp.wrapping_sub(2)) as u32), 0xBEEF);

    cpu.step(&mut mem); // LEAVE: sp = bp; pop bp → sp += 2
                        // After LEAVE, sp is restored to the pre-ENTER value (ENTER pushed one
                        // word onto the stack, LEAVE pops it back), and bp is restored.
    assert_eq!(cpu.regs.sp, saved_sp);
    assert_eq!(cpu.regs.bp, 0xBEEF);
}

#[test]
fn enter_with_nesting_pushes_frame_links() {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xC8, 0x04, 0x00, 0x02]); // ENTER 4, 2
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0x0100;
    cpu.regs.bp = 0x0080;
    mem.write_u16(0x007E, 0x1234);

    cpu.step(&mut mem);

    assert_eq!(cpu.regs.bp, 0x00FE);
    assert_eq!(cpu.regs.sp, 0x00F6);
    assert_eq!(mem.read_u16(0x00FE), 0x0080);
    assert_eq!(mem.read_u16(0x00FC), 0x1234);
    assert_eq!(mem.read_u16(0x00FA), 0x00FE);
}

// ── Far CALL / RET / JMP ─────────────────────────────────────────────────────

#[test]
fn jmp_far_updates_cs_and_ip() {
    // JMP FAR 0x1234:0x0010 (0xEA 0x10 0x00 0x34 0x12)
    let (cpu, _, _) = run_with(|_| {}, &[0xEA, 0x10, 0x00, 0x34, 0x12]);
    assert_eq!(cpu.regs.ip, 0x0010);
    assert_eq!(cpu.regs.cs, 0x1234);
}

#[test]
fn call_far_pushes_cs_and_ip_then_jumps() {
    // CALL FAR 0x0000:0x0020 (0x9A 0x20 0x00 0x00 0x00)
    let (cpu, _, mem) = run_with(|_| {}, &[0x9A, 0x20, 0x00, 0x00, 0x00]);
    assert_eq!(cpu.regs.ip, 0x0020);
    assert_eq!(cpu.regs.cs, 0x0000);
    // stack: CS (0) then IP (5) pushed in order CS first, IP second
    assert_eq!(cpu.regs.sp, 0xFFFA);
    // IP pushed last → at top (FFFA)
    assert_eq!(mem.read_u16(0xFFFA), 5); // return IP (after 5-byte CALL far)
    assert_eq!(mem.read_u16(0xFFFC), 0); // return CS
}

#[test]
fn ret_far_pops_ip_then_cs() {
    // Manually set up: push 0x1000 (CS), push 0x0030 (IP), then RET FAR (0xCB).
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xCB]); // RET far
    mem.write_u16(0xFFFA, 0x0030); // IP on stack top
    mem.write_u16(0xFFFC, 0x1000); // CS below IP
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFA;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.ip, 0x0030);
    assert_eq!(cpu.regs.cs, 0x1000);
    assert_eq!(cpu.regs.sp, 0xFFFE);
}

// ── Group FF: CALL/JMP/PUSH indirect ─────────────────────────────────────────

#[test]
fn ff_call_near_indirect_via_register() {
    // CALL [AX indirect via reg]: FF /2, modrm=11 010 000=0xD0. AX=0x0050.
    let (cpu, _, mem) = run_with(|cpu| cpu.regs.ax = 0x0050, &[0xFF, 0xD0]);
    assert_eq!(cpu.regs.ip, 0x0050);
    // Return address = 2 (size of CALL instruction)
    assert_eq!(mem.read_u16(0xFFFC), 2);
}

#[test]
fn ff_jmp_near_indirect_via_register() {
    // JMP [AX indirect via reg]: FF /4, modrm=11 100 000=0xE0. AX=0x0080.
    let (cpu, _, _) = run_with(|cpu| cpu.regs.ax = 0x0080, &[0xFF, 0xE0]);
    assert_eq!(cpu.regs.ip, 0x0080);
}

#[test]
fn ff_push_rm16_memory_form() {
    // PUSH [0x0020] (FF /6, modrm=00 110 110=0x36, disp16=0x0020)
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xFF, 0x36, 0x20, 0x00]);
    mem.write_u16(0x0020, 0xCAFE);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFE;
    cpu.regs.ds = 0;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.sp, 0xFFFC);
    assert_eq!(mem.read_u16(0xFFFC), 0xCAFE);
}

// ── String instructions ───────────────────────────────────────────────────────

#[test]
fn movsb_copies_byte_from_ds_si_to_es_di() {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xA4]); // MOVSB
    mem.write_u8(0x0100, 0xAB); // DS:SI
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ds = 0;
    cpu.regs.es = 0;
    cpu.regs.si = 0x0100;
    cpu.regs.di = 0x0200;
    cpu.flags.direction = false;
    cpu.step(&mut mem);
    assert_eq!(mem.read_u8(0x0200), 0xAB);
    assert_eq!(cpu.regs.si, 0x0101);
    assert_eq!(cpu.regs.di, 0x0201);
}

#[test]
fn movsb_decrements_si_di_when_direction_flag_set() {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xA4]); // MOVSB
    mem.write_u8(0x0100, 0x55);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ds = 0;
    cpu.regs.es = 0;
    cpu.regs.si = 0x0100;
    cpu.regs.di = 0x0200;
    cpu.flags.direction = true;
    cpu.step(&mut mem);
    assert_eq!(mem.read_u8(0x0200), 0x55);
    assert_eq!(cpu.regs.si, 0x00FF);
    assert_eq!(cpu.regs.di, 0x01FF);
}

#[test]
fn stosb_writes_al_to_es_di() {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xAA]); // STOSB
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.es = 0;
    cpu.regs.di = 0x0300;
    cpu.regs.ax = 0x00CC;
    cpu.flags.direction = false;
    cpu.step(&mut mem);
    assert_eq!(mem.read_u8(0x0300), 0xCC);
    assert_eq!(cpu.regs.di, 0x0301);
}

#[test]
fn lodsb_loads_byte_from_ds_si_into_al() {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xAC]); // LODSB
    mem.write_u8(0x0050, 0x77);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ds = 0;
    cpu.regs.si = 0x0050;
    cpu.flags.direction = false;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.get_reg8(0), 0x77);
    assert_eq!(cpu.regs.si, 0x0051);
}

#[test]
fn scasb_compares_al_with_es_di_and_sets_zero_on_match() {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xAE]); // SCASB
    mem.write_u8(0x0400, 0x42);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.es = 0;
    cpu.regs.di = 0x0400;
    cpu.regs.ax = 0x0042; // AL = 0x42
    cpu.flags.direction = false;
    cpu.step(&mut mem);
    assert!(cpu.flags.zero, "SCASB match must set ZF");
    assert_eq!(cpu.regs.di, 0x0401);
}

#[test]
fn cmpsb_sets_zero_when_bytes_are_equal() {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xA6]); // CMPSB
    mem.write_u8(0x0100, 0x33); // DS:SI
    mem.write_u8(0x0200, 0x33); // ES:DI
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ds = 0;
    cpu.regs.es = 0;
    cpu.regs.si = 0x0100;
    cpu.regs.di = 0x0200;
    cpu.flags.direction = false;
    cpu.step(&mut mem);
    assert!(cpu.flags.zero);
}

// ── REP prefix ───────────────────────────────────────────────────────────────

#[test]
fn rep_stosb_fills_region_with_al() {
    // REP STOSB (0xF3 0xAA): fill CX bytes at ES:DI with AL.
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xF3, 0xAA]); // REP STOSB
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.es = 0;
    cpu.regs.di = 0x0500;
    cpu.regs.cx = 4;
    cpu.regs.ax = 0x00FF;
    cpu.flags.direction = false;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.cx, 0);
    assert_eq!(cpu.regs.di, 0x0504);
    for i in 0u32..4 {
        assert_eq!(mem.read_u8(0x0500 + i), 0xFF, "byte {i} must be 0xFF");
    }
}

#[test]
fn rep_movsb_copies_cx_bytes() {
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xF3, 0xA4]); // REP MOVSB
    for i in 0u8..3 {
        mem.write_u8(0x0100 + i as u32, i + 1); // DS:0100..0102 = 1,2,3
    }
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ds = 0;
    cpu.regs.es = 0;
    cpu.regs.si = 0x0100;
    cpu.regs.di = 0x0200;
    cpu.regs.cx = 3;
    cpu.flags.direction = false;
    cpu.step(&mut mem);
    assert_eq!(cpu.regs.cx, 0);
    assert_eq!(mem.read_u8(0x0200), 1);
    assert_eq!(mem.read_u8(0x0201), 2);
    assert_eq!(mem.read_u8(0x0202), 3);
}

#[test]
fn repne_scasb_stops_at_matching_byte() {
    // REPNE SCASB (0xF2 0xAE): scan ES:DI until AL matches or CX==0.
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xF2, 0xAE]); // REPNE SCASB
                                // Buffer: 0x01, 0x02, 0x03, 0xFF, 0x05 — search for 0xFF
    for (i, &b) in [0x01u8, 0x02, 0x03, 0xFF, 0x05].iter().enumerate() {
        mem.write_u8(0x0300 + i as u32, b);
    }
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.es = 0;
    cpu.regs.di = 0x0300;
    cpu.regs.cx = 5;
    cpu.regs.ax = 0x00FF; // AL = 0xFF
    cpu.flags.direction = false;
    cpu.step(&mut mem);
    // Should stop when ZF=1 (match found at offset 3), having consumed 4 iterations
    assert!(cpu.flags.zero, "ZF must be set when match is found");
    assert_eq!(cpu.regs.cx, 1, "1 element should remain after stopping");
    assert_eq!(cpu.regs.di, 0x0304);
}

#[test]
fn rep_with_cx_zero_is_nop() {
    // REP STOSB with CX=0: must not write anything.
    let mut mem = FlatMemory::new();
    mem.load(0, &[0xF3, 0xAA]);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.es = 0;
    cpu.regs.di = 0x0600;
    cpu.regs.cx = 0;
    cpu.regs.ax = 0x00EE;
    cpu.step(&mut mem);
    assert_eq!(
        mem.read_u8(0x0600),
        0x00,
        "no bytes must be written when CX=0"
    );
    assert_eq!(cpu.regs.di, 0x0600, "DI must be unchanged");
}

// ── BCD instructions ──────────────────────────────────────────────────────────

#[test]
fn aaa_adjusts_after_bcd_add() {
    // AAA (0x37): AL = 0x0F (9 + 6 carry scenario), AF = 0.
    // (0x0F & 0xF) = 0xF > 9 → AH++, AL = (0x0F+6) & 0x0F = 0x05, CF=AF=1.
    let (cpu, _, _) = run_with(
        |cpu| {
            cpu.regs.ax = 0x000F;
            cpu.flags.aux_carry = false;
        },
        &[0x37],
    );
    assert_eq!(cpu.regs.ax & 0xFF, 0x05); // AL = 5
    assert_eq!((cpu.regs.ax >> 8) as u8, 1); // AH = 1
    assert!(cpu.flags.carry);
    assert!(cpu.flags.aux_carry);
}

#[test]
fn aam_divides_al_by_base() {
    // AAM 0x0A (0xD4 0x0A): AH = AL / 10; AL = AL % 10. AL=0x1E (30).
    let (cpu, _, _) = run_with(
        |cpu| cpu.regs.ax = 0x001E, // AL = 30
        &[0xD4, 0x0A],
    );
    assert_eq!((cpu.regs.ax >> 8) as u8, 3); // AH = 3
    assert_eq!(cpu.regs.ax as u8, 0); // AL = 0
    assert!(cpu.flags.zero);
}

#[test]
fn aad_combines_ah_al_before_division() {
    // AAD 0x0A (0xD5 0x0A): AL = AH*10 + AL; AH = 0. AH=3, AL=5 → AL=35.
    let (cpu, _, _) = run_with(
        |cpu| cpu.regs.ax = 0x0305, // AH=3, AL=5
        &[0xD5, 0x0A],
    );
    assert_eq!(cpu.regs.ax, 35); // 0x0023
    assert_eq!((cpu.regs.ax >> 8) as u8, 0);
}
