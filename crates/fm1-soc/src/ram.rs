//! Plain little-endian RAM region at address 0.

pub struct Ram {
    data: Vec<u8>,
    /// Per 4-byte word: holds code the CPU has built blocks from (`mark_code`). The wasm JIT's
    /// inline stores read it too (`code_ptr`): a store to such a word takes the bus instead.
    /// (Words, not pages: the stock firmware's cpu1 keeps data next to its RAM code, and with
    /// 256-byte pages its stores all took the bus and left the compiled code: 2.7x slower.)
    code: Vec<u8>,
    /// Writes to code words so far: while it stays the same, no block built from RAM can have
    /// changed (the CPU skips comparing their bytes with memory).
    pub code_writes: u64,
    /// The stores' check of the marks is required (never goes back; `Bus::code_watch`).
    pub watched: bool,
}

impl Ram {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0; size],
            code: vec![0; size / 4 + 1],
            code_writes: 0,
            watched: false,
        }
    }

    /// Bytes lo..hi hold code: their words are watched from now on. (From lo - 3: a store that
    /// reaches into a code word starts at most 3 bytes before it, and stores check the word they
    /// start in.)
    pub fn mark_code(&mut self, lo: u32, hi: u32) {
        let last = (hi.max(lo + 1) - 1) as usize >> 2;
        for w in (lo.saturating_sub(3) as usize >> 2)..=last.min(self.code.len() - 1) {
            self.code[w] = 1;
        }
    }

    /// Host pointer to the code-word flags (the wasm JIT's inline stores).
    pub fn code_ptr(&self) -> *const u8 {
        self.code.as_ptr()
    }

    #[inline(always)]
    fn wrote(&mut self, a: usize) {
        if self.code[a >> 2] != 0 {
            self.code_writes += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Host pointer to the bytes (the JIT's inline loads and stores).
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn read8(&self, addr: u32) -> u8 {
        self.data[addr as usize]
    }

    pub fn read16(&self, addr: u32) -> u16 {
        let a = addr as usize;
        u16::from_le_bytes([self.data[a], self.data[a + 1]])
    }

    pub fn read32(&self, addr: u32) -> u32 {
        let a = addr as usize;
        u32::from_le_bytes([
            self.data[a],
            self.data[a + 1],
            self.data[a + 2],
            self.data[a + 3],
        ])
    }

    pub fn write8(&mut self, addr: u32, value: u8) {
        self.wrote(addr as usize);
        self.data[addr as usize] = value;
    }

    pub fn write16(&mut self, addr: u32, value: u16) {
        let a = addr as usize;
        self.wrote(a);
        self.data[a..a + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub fn write32(&mut self, addr: u32, value: u32) {
        let a = addr as usize;
        self.wrote(a);
        self.data[a..a + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// Load raw bytes at an offset (firmware loading).
    pub fn load(&mut self, offset: u32, bytes: &[u8]) {
        let o = offset as usize;
        self.code_writes += 1;
        self.data[o..o + bytes.len()].copy_from_slice(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_widths() {
        let mut ram = Ram::new(64);
        ram.write32(0, 0xDEAD_BEEF);
        assert_eq!(ram.read32(0), 0xDEAD_BEEF);
        assert_eq!(ram.read16(0), 0xBEEF);
        assert_eq!(ram.read8(1), 0xBE);
        ram.write16(8, 0x1234);
        assert_eq!(ram.read8(8), 0x34);
        assert_eq!(ram.read8(9), 0x12);
    }
}
