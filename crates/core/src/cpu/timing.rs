//! NEC V30MZ instruction clock-cycle costs.
//!
//! The WonderSwan's CPU is an NEC V30MZ, which executes most instructions in
//! substantially fewer clocks than the Intel 8086 it is source-compatible with
//! (e.g. `MOV` = 1, `INC` = 1, `IRET` = 10 vs. the 8086's 10 / 3 / 32). Using
//! 8086 timings makes interrupt-heavy code run far too slowly relative to the
//! fixed per-frame clock budget, which visibly breaks games that lean on
//! per-scanline line-compare interrupts (e.g. FF4's opening line-scroll effect).
//!
//! Source of truth: the WonderSwan hardware reference "Sacred Tech Scroll"
//! (<http://perfectkiosk.net/stsws.html>), whose per-instruction cycle counts
//! are hardware-verified, cross-checked against the µPD70116 (V30) datasheet
//! and FluBBaOfWard/WSTimingTest where an opt-in public ROM oracle is available.
//!
//! stsws lists register/memory operand instructions as a range `reg-mem`; the
//! low value is the register-operand cost and the high value the memory-operand
//! cost. Branches are listed as `N+`: `N` clocks when not taken, plus a
//! prefetch-queue refill when taken. WSTimingTest page 0 pins the common taken
//! `Jcc` case at 5 clocks, with an additional clock when the target address is
//! odd. These per-instruction totals fold the operand fetch/refill into one
//! number; a future phase may decompose them into per-clock timing.

use super::decode::RegMem;

/// Clocks to acknowledge and dispatch a hardware (maskable) interrupt: push
/// PSW/PS/PC and load the vector. Modelled on the V30 software `INT`
/// (`BRK`/`INT n`) cost (stsws: 9–10); the 8086-era value of ~32/51 does not
/// apply. Charged once per accepted IRQ (see `System::run_cpu_cycles`).
pub(super) const IRQ_ACK: u32 = 10;

/// Pick the register- or memory-operand clock count for an instruction whose
/// cost depends on the ModRM operand kind (stsws `reg`-`mem` range).
pub(super) fn rm(operand: &RegMem, reg: u32, mem: u32) -> u32 {
    match operand {
        RegMem::Reg(_) => reg,
        RegMem::Mem(_) => mem,
    }
}
