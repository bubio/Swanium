/// V30MZ status flags, laid out like the 8086 FLAGS register.
///
/// Assumption (unverified against real hardware, see
/// "リスクと不確実性への対処方針" in docs/dev/DevelopmentPlan.md): the reserved
/// bits are modeled after the 8086 (bit 1 always reads as 1, others as 0).
/// If real-hardware testing later shows the V30MZ differs, update
/// `to_u16`/`from_u16` and note the correction here.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    pub carry: bool,
    pub parity: bool,
    pub aux_carry: bool,
    pub zero: bool,
    pub sign: bool,
    pub trap: bool,
    pub interrupt: bool,
    pub direction: bool,
    pub overflow: bool,
}

const CF: u16 = 1 << 0;
const RESERVED_BIT1: u16 = 1 << 1;
const PF: u16 = 1 << 2;
const AF: u16 = 1 << 4;
const ZF: u16 = 1 << 6;
const SF: u16 = 1 << 7;
const TF: u16 = 1 << 8;
const IF: u16 = 1 << 9;
const DF: u16 = 1 << 10;
const OF: u16 = 1 << 11;

impl Flags {
    pub fn to_u16(self) -> u16 {
        let mut v = RESERVED_BIT1;
        v |= if self.carry { CF } else { 0 };
        v |= if self.parity { PF } else { 0 };
        v |= if self.aux_carry { AF } else { 0 };
        v |= if self.zero { ZF } else { 0 };
        v |= if self.sign { SF } else { 0 };
        v |= if self.trap { TF } else { 0 };
        v |= if self.interrupt { IF } else { 0 };
        v |= if self.direction { DF } else { 0 };
        v |= if self.overflow { OF } else { 0 };
        v
    }

    pub fn from_u16(v: u16) -> Self {
        Flags {
            carry: v & CF != 0,
            parity: v & PF != 0,
            aux_carry: v & AF != 0,
            zero: v & ZF != 0,
            sign: v & SF != 0,
            trap: v & TF != 0,
            interrupt: v & IF != 0,
            direction: v & DF != 0,
            overflow: v & OF != 0,
        }
    }
}

/// Parity flag is defined over the least-significant byte of the ALU result,
/// even for 16-bit operations — this is an 8086 quirk that V30MZ inherits.
pub fn parity_even(value: u8) -> bool {
    value.count_ones().is_multiple_of(2)
}
