/// General-purpose and segment registers of the NEC V30MZ.
///
/// 8-bit sub-register indices follow the 8086 ModRM encoding order:
/// 0=AL,1=CL,2=DL,3=BL,4=AH,5=CH,6=DH,7=BH.
/// 16-bit register indices follow the same encoding order:
/// 0=AX,1=CX,2=DX,3=BX,4=SP,5=BP,6=SI,7=DI.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub ax: u16,
    pub cx: u16,
    pub dx: u16,
    pub bx: u16,
    pub sp: u16,
    pub bp: u16,
    pub si: u16,
    pub di: u16,

    pub cs: u16,
    pub ds: u16,
    pub ss: u16,
    pub es: u16,

    pub ip: u16,
}

impl Registers {
    pub fn get_reg16(&self, index: u8) -> u16 {
        match index & 0b111 {
            0 => self.ax,
            1 => self.cx,
            2 => self.dx,
            3 => self.bx,
            4 => self.sp,
            5 => self.bp,
            6 => self.si,
            7 => self.di,
            _ => unreachable!(),
        }
    }

    pub fn set_reg16(&mut self, index: u8, value: u16) {
        match index & 0b111 {
            0 => self.ax = value,
            1 => self.cx = value,
            2 => self.dx = value,
            3 => self.bx = value,
            4 => self.sp = value,
            5 => self.bp = value,
            6 => self.si = value,
            7 => self.di = value,
            _ => unreachable!(),
        }
    }

    pub fn get_reg8(&self, index: u8) -> u8 {
        match index & 0b111 {
            0 => self.ax as u8,
            1 => self.cx as u8,
            2 => self.dx as u8,
            3 => self.bx as u8,
            4 => (self.ax >> 8) as u8,
            5 => (self.cx >> 8) as u8,
            6 => (self.dx >> 8) as u8,
            7 => (self.bx >> 8) as u8,
            _ => unreachable!(),
        }
    }

    pub fn set_reg8(&mut self, index: u8, value: u8) {
        match index & 0b111 {
            0 => self.ax = (self.ax & 0xFF00) | value as u16,
            1 => self.cx = (self.cx & 0xFF00) | value as u16,
            2 => self.dx = (self.dx & 0xFF00) | value as u16,
            3 => self.bx = (self.bx & 0xFF00) | value as u16,
            4 => self.ax = (self.ax & 0x00FF) | (value as u16) << 8,
            5 => self.cx = (self.cx & 0x00FF) | (value as u16) << 8,
            6 => self.dx = (self.dx & 0x00FF) | (value as u16) << 8,
            7 => self.bx = (self.bx & 0x00FF) | (value as u16) << 8,
            _ => unreachable!(),
        }
    }
}
