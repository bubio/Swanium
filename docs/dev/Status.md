# Implementation status

Last updated: 2026-07-02. Update this file (not AGENTS.md) when implementation progress changes.

Phase 1 of `docs/dev/DevelopmentPlan.md` is substantially complete (80+ unit tests). `crates/core/src/cpu` implements the V30MZ register file, flags, ModRM decoding, and a near-complete 8086-compatible instruction set against a `MemoryBus` trait, using a test-only flat-memory implementation:

- Data movement: MOV (all forms incl. segment registers and memory-direct 0xA0–0xA3), XCHG, PUSH/POP (incl. segment register forms), LAHF/SAHF/PUSHF/POPF, XLAT, CBW/CWD, LEA, LES, LDS.
- Arithmetic/logic: ADD/OR/ADC/SBB/AND/SUB/XOR/CMP/TEST and their immediate/group forms, INC/DEC, NOT/NEG, MUL/IMUL/DIV/IDIV (group F6/F7), shift/rotate group (D0-D3), BCD instructions DAA/DAS/AAA/AAS/AAM/AAD.
- Control flow: JMP (near/far)/Jcc/CALL (near/far)/RET (near/far), LOOP/LOOPE/LOOPNE/JCXZ, flag instructions, NOP/HLT, ENTER/LEAVE, indirect CALL/JMP/PUSH via Group FF.
- String instructions: MOVS/CMPS/SCAS/LODS/STOS (byte and word, 0xA4–0xAF) with REP/REPE/REPNE (0xF2/0xF3).
- 80186/V30 additions (the V30MZ is 80186-class): PUSHA/POPA (0x60/0x61), BOUND (0x62, INT 5 on range error), PUSH imm16/imm8 (0x68/0x6A), IMUL r16,r/m16,imm16/imm8 (0x69/0x6B), immediate-count shift/rotate (0xC0/0xC1), POP r/m16 (0x8F). Added when a commercial ROM (Lode Runner) exercised them; see `crates/core/src/cpu/tests/v30_extensions.rs`.
- Prefixes: segment override (0x26 ES:, 0x2E CS:, 0x36 SS:, 0x3E DS:) stored in `Cpu::seg_override`; REP stored in `Cpu::rep_prefix`.

Still deferred (panics via `unimplemented!`): IN/OUT port I/O (needs Phase 2 I/O bus), INT/IRET (needs interrupt controller — DIV/IDIV/AAM by zero also defer here), ENTER with nesting level > 0. Memory map, interrupt controller, timers, DMA, PPU, APU, and cartridge logic are not yet implemented — see `docs/dev/DevelopmentPlan.md` for the phase-by-phase roadmap.

Frontend progress (Phase 7): cpal audio, gilrs gamepad input, TOML config persistence, in-app ROM picker (rfd), menu bar, status bar are implemented. Next: startup-pause, settings UI, key-binding screen; then Phase 8 (WonderSwan Color).
