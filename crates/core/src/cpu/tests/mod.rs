use super::{Cpu, MemoryBus};

mod alu;
mod bit_ops;
mod ctrl_flow;
mod mov_stack;
mod segment_string;

/// Flat 1 MiB memory used only for Phase 1 CPU unit tests. Phase 2 replaces
/// this with the real WonderSwan memory map (RAM/I/O/cartridge dispatch);
/// see docs/dev/DevelopmentPlan.md.
pub struct FlatMemory {
    data: Vec<u8>,
}

impl FlatMemory {
    pub fn new() -> Self {
        FlatMemory {
            data: vec![0; 0x10_0000],
        }
    }

    pub fn load(&mut self, addr: u32, bytes: &[u8]) {
        self.data[addr as usize..addr as usize + bytes.len()].copy_from_slice(bytes);
    }
}

impl MemoryBus for FlatMemory {
    fn read_u8(&self, addr: u32) -> u8 {
        self.data[addr as usize]
    }

    fn write_u8(&mut self, addr: u32, value: u8) {
        self.data[addr as usize] = value;
    }
}

/// Builds a CPU + memory pair with code loaded at CS:IP = 0x0000:0x0000 and
/// SS:SP = 0x0000:0xFFFE, applies `setup` to the CPU before execution, runs
/// exactly one instruction, and returns the resulting CPU state, the consumed
/// cycle count, and the final memory (for assertions on written bytes).
pub fn run_with(setup: impl FnOnce(&mut Cpu), code: &[u8]) -> (Cpu, u32, FlatMemory) {
    let mut mem = FlatMemory::new();
    mem.load(0, code);
    let mut cpu = Cpu::new();
    cpu.reset(0, 0);
    cpu.regs.ss = 0;
    cpu.regs.sp = 0xFFFE;
    setup(&mut cpu);
    let cycles = cpu.step(&mut mem);
    (cpu, cycles, mem)
}
