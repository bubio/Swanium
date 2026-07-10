//! Serial EEPROM device (93Cxx / Microwire-style) used for cartridge saves.
//!
//! WonderSwan cartridges that don't carry SRAM may instead carry a small serial
//! EEPROM. The CPU drives it through a 16-bit data port, a 16-bit command port,
//! and an 8-bit control port (cartridge EEPROM: ports 0xC4–0xC8). Each access is
//! a 16-bit word; the command word selects an operation and a word address:
//!
//! ```text
//! bit:  [ addr_bits+2 ]  [ addr_bits+1 .. addr_bits ]  [ addr_bits-1 .. 0 ]
//!         start (1)        2-bit opcode                  word address
//! ```
//!
//! `addr_bits` depends on the EEPROM capacity (more words ⇒ wider address). When
//! the opcode is 0 the operation is an *extended* command whose 2-bit sub-opcode
//! sits just below the opcode field.

/// A 93Cxx-style serial EEPROM holding 16-bit words.
pub struct Eeprom {
    /// EEPROM contents, little-endian 16-bit words. Length is a multiple of 2.
    contents: Vec<u8>,
    /// Last value written to the data port (input latch).
    input: u16,
    /// Value presented on the data port after a READ (output latch).
    output: u16,
    /// Number of address bits in a command word (derived from capacity).
    address_bits: u8,
    /// Whether write/erase operations are currently permitted.
    write_enabled: bool,
}

/// Primary opcodes (the 2 bits above the address field).
const OP_EXTENDED: u16 = 0;
const OP_WRITE: u16 = 1;
const OP_READ: u16 = 2;
const OP_ERASE: u16 = 3;

/// Extended sub-opcodes (used when the primary opcode is [`OP_EXTENDED`]).
const SUB_WRITE_DISABLE: u16 = 0;
const SUB_WRITE_ALL: u16 = 1;
const SUB_ERASE_ALL: u16 = 2;
const SUB_WRITE_ENABLE: u16 = 3;

/// Erased EEPROM cells read back as all-ones.
const ERASED_BYTE: u8 = 0xFF;

impl Eeprom {
    /// Create an EEPROM with the given contents and command address width.
    ///
    /// Writes are enabled for internal EEPROM use and direct device tests;
    /// cartridge EEPROMs should use [`Eeprom::new_locked`].
    pub fn new(contents: Vec<u8>, address_bits: u8) -> Self {
        Self {
            contents,
            input: 0,
            output: 0xFFFF,
            address_bits,
            write_enabled: true,
        }
    }

    /// Create an EEPROM whose write/erase commands are locked at power-on.
    ///
    /// Cartridge EEPROMs power up write-disabled; games must issue EWEN before
    /// writes or erases take effect.
    pub fn new_locked(contents: Vec<u8>, address_bits: u8) -> Self {
        Self {
            write_enabled: false,
            ..Self::new(contents, address_bits)
        }
    }

    /// The number of command address bits for an EEPROM of `size` bytes.
    ///
    /// Returns `None` for sizes that don't correspond to a real WonderSwan
    /// cartridge EEPROM.
    pub fn address_bits_for(size: usize) -> Option<u8> {
        match size {
            128 => Some(6),
            1024 => Some(9),
            2048 => Some(10),
            _ => None,
        }
    }

    /// The full EEPROM contents (for save-data serialisation).
    pub fn contents(&self) -> &[u8] {
        &self.contents
    }

    /// Overwrite the EEPROM contents from previously serialised save data.
    ///
    /// Copies up to the device capacity; a shorter slice leaves the trailing
    /// cells untouched, a longer slice is truncated.
    pub fn load_contents(&mut self, data: &[u8]) {
        let n = data.len().min(self.contents.len());
        self.contents[..n].copy_from_slice(&data[..n]);
    }

    /// The value currently latched on the data/output port.
    pub fn read_data(&self) -> u16 {
        self.output
    }

    /// Latch a value into the input port (consumed by a subsequent WRITE).
    pub fn write_data(&mut self, data: u16) {
        self.input = data;
    }

    /// Execute the command encoded in `comm`.
    ///
    /// A command with its start bit clear, or addressing a word beyond the
    /// device, is ignored. READ updates the output latch (observable via
    /// [`Eeprom::read_data`]); WRITE consumes the input latch.
    pub fn execute(&mut self, comm: u16) {
        // Anything above the start bit must be zero for a valid command.
        if comm >> (self.address_bits + 3) != 0 {
            return;
        }
        let start_bit = (comm >> (self.address_bits + 2)) & 1;
        if start_bit == 0 {
            return;
        }
        let opcode = (comm >> self.address_bits) & 3;
        if opcode == OP_EXTENDED {
            let sub_opcode = (comm >> (self.address_bits - 2)) & 3;
            self.execute_extended(sub_opcode);
            return;
        }
        let word_addr = comm & ((1 << self.address_bits) - 1);
        let byte_addr = (word_addr as usize) * 2;
        if byte_addr + 1 >= self.contents.len() {
            return;
        }
        self.execute_addressed(byte_addr, opcode);
    }

    /// Execute a command using a temporary address width.
    ///
    /// Internal console EEPROM commands use the mono-width protocol when the
    /// system is in mono-compatible mode and the wider Color protocol in Color
    /// mode, while the backing storage remains shared.
    pub fn execute_with_address_bits(&mut self, comm: u16, address_bits: u8) {
        let saved = self.address_bits;
        self.address_bits = address_bits;
        self.execute(comm);
        self.address_bits = saved;
    }

    /// Execute a word-addressed command (WRITE / READ / ERASE).
    fn execute_addressed(&mut self, byte_addr: usize, opcode: u16) {
        match opcode {
            OP_WRITE if self.write_enabled => {
                let [lo, hi] = self.input.to_le_bytes();
                self.contents[byte_addr] = lo;
                self.contents[byte_addr + 1] = hi;
            }
            OP_READ => {
                self.output =
                    u16::from_le_bytes([self.contents[byte_addr], self.contents[byte_addr + 1]]);
            }
            OP_ERASE if self.write_enabled => {
                self.contents[byte_addr] = ERASED_BYTE;
                self.contents[byte_addr + 1] = ERASED_BYTE;
            }
            _ => {}
        }
    }

    /// Execute an extended command (write-enable/disable, write-all, erase-all).
    fn execute_extended(&mut self, sub_opcode: u16) {
        match sub_opcode {
            SUB_WRITE_DISABLE => self.write_enabled = false,
            SUB_WRITE_ENABLE => self.write_enabled = true,
            SUB_WRITE_ALL if self.write_enabled => {
                let [lo, hi] = self.input.to_le_bytes();
                for (i, cell) in self.contents.iter_mut().enumerate() {
                    *cell = if i % 2 == 0 { lo } else { hi };
                }
            }
            SUB_ERASE_ALL if self.write_enabled => self.contents.fill(ERASED_BYTE),
            _ => {}
        }
    }
}
