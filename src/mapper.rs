//! Cartridge mappers: board logic for PRG/CHR banking, mirroring, IRQ.
//!
//! Supported boards: 0 NROM, 1 MMC1, 2 UxROM, 3 CNROM, 4 MMC3/MMC6, 7 AxROM,
//! 9 MMC2 (approx), 11 Color Dreams, 66 MN3065. Unsupported mappers return a
//! clear error at load time instead of silently mis-emulating.

use crate::rom::Rom;

pub trait Mapper {
    /// CPU $8000-$FFFF
    fn prg_read(&mut self, addr: u16) -> u8;
    fn prg_write(&mut self, addr: u16, val: u8);
    /// PPU $0000-$1FFF
    fn chr_read(&mut self, addr: u16) -> u8;
    fn chr_write(&mut self, addr: u16, val: u8);
    /// 0 = horizontal, 1 = vertical, 2 = pass raw (4-screen or mapper-managed)
    fn mirroring(&self) -> u8;
    /// Override which physical VRAM nametable (0..3) the PPU should use for a
    /// raw nametable index; x_right tells whether the fetch is on the right
    /// screen half (used by MMC2/4-style latch boards).
    fn name_table(&self, raw: u8, _x_right: bool) -> u8 {
        raw
    }
    fn sram_enabled(&self) -> bool {
        true
    }
    fn irq_active(&self) -> bool {
        false
    }
    fn irq_ack(&mut self) {}
    /// Called once per visible scanline (0..239)
    fn scanline(&mut self, _line: u16) {}
    /// PPU palette-space access, for MMC2/4 latching
    fn palette_access(&mut self, _addr: u16, _read: bool) {}
}

/// Helper: mirror a bank index into a rom of given size (unused today).
#[allow(dead_code)]
#[inline]
fn mirrored_offset(bank_addr: usize, rom: &[u8]) -> usize {
    if rom.is_empty() {
        0
    } else {
        bank_addr % rom.len()
    }
}

// ============================ NROM (0) =============================

pub struct Nrom {
    pub prg: Vec<u8>,
    pub chr: Vec<u8>, // ROM or 8KB RAM
    pub mir: u8,
}

impl Nrom {
    pub fn new(rom: &Rom) -> Self {
        let mir = if rom.info.mirroring_vertical { 1 } else { 0 };
        let chr = if rom.chr.is_empty() { vec![0u8; 8192] } else { rom.chr.clone() };
        Nrom { prg: rom.prg.clone(), chr, mir }
    }
    pub fn default_nrom() -> Self {
        Nrom { prg: vec![0; 16384], chr: vec![0; 8192], mir: 0 }
    }
}

impl Mapper for Nrom {
    fn prg_read(&mut self, addr: u16) -> u8 {
        let a = (addr - 0x8000) as usize;
        if self.prg.len() == 0x8000 {
            self.prg[a]
        } else {
            self.prg[a % self.prg.len()]
        }
    }
    fn prg_write(&mut self, _a: u16, _v: u8) {}
    fn chr_read(&mut self, addr: u16) -> u8 {
        self.chr[(addr & 0x1FFF) as usize % self.chr.len().max(1)]
    }
    fn chr_write(&mut self, addr: u16, val: u8) {
        let i = (addr & 0x1FFF) as usize;
        if self.chr.len() == 8192 {
            self.chr[i] = val;
        }
    }
    fn mirroring(&self) -> u8 {
        self.mir
    }
}

// ============================ MMC1 (1) =============================

pub struct Mmc1 {
    prg: Vec<u8>,
    chr: Vec<u8>, // rom or 8/16KB ram
    sram: Vec<u8>,
    bit_cnt: u8,
    shift: u8,
    ctrl: u8,
    prg_bank: u8,
    chr_bank0: u8,
    chr_bank1: u8,
}

impl Mmc1 {
    pub fn new(rom: &Rom) -> Self {
        Mmc1 {
            prg: rom.prg.clone(),
            chr: if rom.chr.is_empty() { vec![0u8; 8192] } else { rom.chr.clone() },
            sram: vec![0u8; 8192],
            bit_cnt: 0,
            shift: 0,
            ctrl: 0x0C, // SRAM enabled, default mirror
            prg_bank: 0,
            chr_bank0: 0,
            chr_bank1: 0,
        }
    }
    fn load_register(&mut self, addr: u16, val: u8) {
        match addr {
            0x8000..=0x9FFF => {
                self.ctrl = val;
                if val & 0x10 != 0 {
                    // reset: disable for 6 cpu cycles; emulate with bit reset
                    self.bit_cnt = 0;
                    self.shift = 0;
                    self.ctrl = val | 0x0C;
                }
            }
            0xA000..=0xBFFF => self.chr_bank0 = val & 0x1F,
            0xC000..=0xDFFF => self.chr_bank1 = val & 0x1F,
            0xE000..=0xFFFF => self.prg_bank = val & 0x0F,
            _ => {}
        }
    }
    fn prg_mode(&self) -> u8 {
        (self.ctrl >> 2) & 3
    }
}

impl Mapper for Mmc1 {
    fn prg_read(&mut self, addr: u16) -> u8 {
        let a = (addr - 0x8000) as usize;
        let prg_len = self.prg.len();
        if prg_len == 0 {
            return 0;
        }
        let off = match self.prg_mode() {
            // 32K switching
            0 => {
                let bank = (self.prg_bank as usize & 0xFE) * 0x4000;
                bank + a
            }
            // 8K fixed-last: first slot switches
            1 => {
                if a < 0x4000 {
                    self.prg_bank as usize * 0x4000 + a
                } else {
                    (prg_len - 0x4000) + (a - 0x4000)
                }
            }
            // 8K fixed-first: second slot switches
            2 => {
                if a < 0x4000 {
                    a
                } else {
                    self.prg_bank as usize * 0x4000 + (a - 0x4000)
                }
            }
            // 8K fixed-second-to-last: second switches
            _ => {
                if a < 0x4000 {
                    (prg_len - 0x8000) + a
                } else {
                    self.prg_bank as usize * 0x4000 + (a - 0x4000)
                }
            }
        };
        self.prg[off % prg_len]
    }
    fn prg_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            if val & 0x80 != 0 {
                self.bit_cnt = 0;
                self.shift = 0;
                // reset keeps previous ctrl but forces 2-screen? nesdev: control register reset
                self.load_register(addr & 0xE000 | 0x8000, self.shift);
                return;
            }
            self.shift = (self.shift >> 1) | ((val & 1) << 4);
            self.bit_cnt += 1;
            if self.bit_cnt == 5 {
                let v = self.shift;
                self.bit_cnt = 0;
                self.shift = 0;
                self.load_register(addr, v);
            }
        } else if addr >= 0x6000 && self.sram_enabled() {
            let i = (addr - 0x6000) as usize % self.sram.len();
            self.sram[i] = val;
        }
    }
    fn chr_read(&mut self, addr: u16) -> u8 {
        let a = (addr & 0x1FFF) as usize;
        if self.chr.len() <= 8192 {
            return self.chr[a % self.chr.len().max(1)];
        }
        if self.ctrl & 0x10 == 0 {
            // 8K mode
            let bank = (self.chr_bank0 as usize & 0xFE) * 0x2000;
            self.chr[bank % self.chr.len() + a % 0x2000]
        } else {
            // 4K mode
            if a < 0x1000 {
                let bank = self.chr_bank0 as usize * 0x1000;
                self.chr[bank % self.chr.len() + a]
            } else {
                let bank = self.chr_bank1 as usize * 0x1000;
                self.chr[bank % self.chr.len() + (a - 0x1000)]
            }
        }
    }
    fn chr_write(&mut self, addr: u16, val: u8) {
        let i = (addr & 0x1FFF) as usize;
        if self.chr.len() == 8192 {
            self.chr[i] = val;
        }
    }
    fn mirroring(&self) -> u8 {
        match self.ctrl & 3 {
            0 | 1 => 2, // single-screen, handled via name_table
            2 => 1,     // vertical
            _ => 0,     // horizontal
        }
    }
    fn name_table(&self, raw: u8, _x_right: bool) -> u8 {
        match self.ctrl & 3 {
            0 => 0, // one-screen lower
            1 => 1, // one-screen upper
            2 => raw >> 1,
            _ => raw & 1,
        }
    }
    fn sram_enabled(&self) -> bool {
        self.ctrl & 0x08 != 0
    }
}

// ============================ UxROM (2) =============================

pub struct Uxrom {
    prg: Vec<u8>,
    chr: Vec<u8>, // usually 8KB CHR RAM
    bank: u8,
}

impl Uxrom {
    pub fn new(rom: &Rom) -> Self {
        Uxrom {
            prg: rom.prg.clone(),
            chr: if rom.chr.is_empty() { vec![0u8; 8192] } else { rom.chr.clone() },
            bank: 0,
        }
    }
}

impl Mapper for Uxrom {
    fn prg_read(&mut self, addr: u16) -> u8 {
        let a = (addr - 0x8000) as usize;
        if a < 0x4000 {
            let off = self.bank as usize * 0x4000 + a;
            self.prg[off % self.prg.len()]
        } else {
            self.prg[(self.prg.len() - 0x4000) + (a - 0x4000)]
        }
    }
    fn prg_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.bank = val & 0x1F;
        }
    }
    fn chr_read(&mut self, addr: u16) -> u8 {
        self.chr[(addr & 0x1FFF) as usize % self.chr.len().max(1)]
    }
    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.len() == 8192 {
            self.chr[(addr & 0x1FFF) as usize] = val;
        }
    }
    fn mirroring(&self) -> u8 {
        if self.bank & 0x80 != 0 {
            0
        } else {
            1
        }
    }
}

// ============================ CNROM (3) =============================

pub struct Cnrom {
    prg: Vec<u8>,
    chr: Vec<u8>,
    bank: u8,
}

impl Cnrom {
    pub fn new(rom: &Rom) -> Self {
        Cnrom {
            prg: rom.prg.clone(),
            chr: if rom.chr.is_empty() { vec![0u8; 8192] } else { rom.chr.clone() },
            bank: 0,
        }
    }
}

impl Mapper for Cnrom {
    fn prg_read(&mut self, addr: u16) -> u8 {
        let a = (addr - 0x8000) as usize;
        let off = if self.prg.len() == 0x8000 { a } else { a % self.prg.len() };
        self.prg[off % self.prg.len()]
    }
    fn prg_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.bank = val & 0x03;
        }
    }
    fn chr_read(&mut self, addr: u16) -> u8 {
        let a = (addr & 0x1FFF) as usize;
        if self.chr.len() == 8192 {
            self.chr[a]
        } else {
            let bank_addr = self.bank as usize * 0x2000;
            self.chr[(bank_addr + a) % self.chr.len()]
        }
    }
    fn chr_write(&mut self, _a: u16, _v: u8) {}
    fn mirroring(&self) -> u8 {
        0
    }
}

// ============================ MMC3 / MMC6 (4) =============================

pub struct Mmc3 {
    prg: Vec<u8>,
    chr: Vec<u8>,
    regs: [u8; 8],
    cmd: u8,
    chr_invert: bool,
    prg_swap: bool,
    prg_protect: bool,
    mir: u8,
    // IRQ
    counter: u8,
    latch: u8,
    enable: bool,
    irq_request: bool,
    is_mmc6: bool,
}

impl Mmc3 {
    pub fn new(rom: &Rom) -> Self {
        Mmc3 {
            prg: rom.prg.clone(),
            chr: if rom.chr.is_empty() { vec![0u8; 8192] } else { rom.chr.clone() },
            regs: [0; 8],
            cmd: 0,
            chr_invert: false,
            prg_swap: false,
            prg_protect: false,
            mir: if rom.info.mirroring_vertical { 1 } else { 0 },
            counter: 0,
            latch: 0,
            enable: false,
            irq_request: false,
            is_mmc6: rom.info.mapper == 121 || rom.info.submapper == 1 && rom.info.mapper == 4,
        }
    }
    fn prg_at(&self, bank8k: usize) -> usize {
        let b = bank8k;
        b.wrapping_mul(0x2000)
    }
    fn read_prg_slot(&self, slot: usize, offset: usize) -> u8 {
        // slot: 0=$8000, 1=$A000, 2=$C000, 3=$E000
        let n = self.prg.len() / 0x2000;
        if n == 0 {
            return 0;
        }
        let bank = match slot {
            0 => {
                if self.prg_swap {
                    n - 2
                } else {
                    self.regs[6] as usize
                }
            }
            1 => self.regs[7] as usize,
            2 => {
                if self.prg_swap {
                    self.regs[6] as usize
                } else {
                    n - 2
                }
            }
            _ => n - 1,
        };
        let bank = bank % n;
        self.prg[bank * 0x2000 + offset % 0x2000]
    }
    fn chr_bank_addr(&self, addr: u16) -> usize {
        let a = (addr & 0x1FFF) as usize;
        let r = &self.regs;
        if self.chr.len() <= 8192 {
            return a;
        }
        // slot layout depends on the A12 invert bit
        let base = if !self.chr_invert {
            match a {
                0x0000..=0x07FF => (r[0] as usize & !1) * 0x400 + a,
                0x0800..=0x0FFF => (r[1] as usize & !1) * 0x400 + (a - 0x800),
                0x1000..=0x13FF => r[2] as usize * 0x400 + (a - 0x1000),
                0x1400..=0x17FF => r[3] as usize * 0x400 + (a - 0x1400),
                0x1800..=0x1BFF => r[4] as usize * 0x400 + (a - 0x1800),
                0x1C00..=0x1FFF => r[5] as usize * 0x400 + (a - 0x1C00),
                _ => r[0] as usize * 0x400 + a,
            }
        } else {
            match a {
                0x0000..=0x03FF => r[2] as usize * 0x400 + a,
                0x0400..=0x07FF => r[3] as usize * 0x400 + (a - 0x400),
                0x0800..=0x0BFF => r[4] as usize * 0x400 + (a - 0x800),
                0x0C00..=0x0FFF => r[5] as usize * 0x400 + (a - 0xC00),
                0x1000..=0x17FF => (r[0] as usize & !1) * 0x400 + (a - 0x1000),
                _ => (r[1] as usize & !1) * 0x400 + (a - 0x1800),
            }
        };
        base
    }
}

impl Mapper for Mmc3 {
    fn prg_read(&mut self, addr: u16) -> u8 {
        let slot = ((addr - 0x8000) / 0x2000) as usize;
        let off = ((addr - 0x8000) % 0x2000) as usize;
        self.read_prg_slot(slot, off)
    }
    fn prg_write(&mut self, addr: u16, val: u8) {
        // MMC3 hardware decode:
        //   $8000 / $E000 (even): bank command
        //   $8001 / $A001 / $E001 (odd): bank data -> regs[cmd & 7]
        //   $A000 (even): PRG protect
        //   $C000-$C003: IRQ control (any write acknowledges IRQ)
        match addr {
            0x8000 | 0xE000 => {
                self.cmd = val & 0x3F;
                self.chr_invert = val & 0x80 != 0;
                self.prg_swap = val & 0x40 != 0;
            }
            0x8001 | 0xA001 | 0xE001 => {
                self.regs[(self.cmd & 7) as usize] = val;
            }
            0xA000 => self.prg_protect = val & 0x40 != 0,
            0xC000 => {
                self.latch = val;
                self.irq_request = false;
            }
            0xC001 => {
                self.enable = false;
                self.irq_request = false;
            }
            0xC002 => {
                self.counter = 0;
                self.enable = true;
                self.irq_request = false;
            }
            0xC003 => {
                self.enable = true;
                self.irq_request = false;
            }
            _ => {}
        }
    }
    fn chr_read(&mut self, addr: u16) -> u8 {
        let off = self.chr_bank_addr(addr);
        if self.chr.len() == 0 {
            return 0;
        }
        self.chr[off % self.chr.len()]
    }
    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.len() == 8192 {
            self.chr[(addr & 0x1FFF) as usize] = val;
        }
    }
    fn mirroring(&self) -> u8 {
        self.mir
    }
    fn irq_active(&self) -> bool {
        self.irq_request
    }
    fn irq_ack(&mut self) {
        // re-arming happens on the next $C002/$C003 write; here just clear
        self.irq_request = false;
    }
    fn scanline(&mut self, _line: u16) {
        if self.enable {
            if self.counter == 0 {
                self.counter = self.latch;
            }
            self.counter = self.counter.wrapping_sub(1);
            if self.counter == 0 {
                self.irq_request = true;
            }
        }
    }
    fn sram_enabled(&self) -> bool {
        true
    }
}

// ============================ AxROM (7) =============================

pub struct Axrom {
    prg: Vec<u8>,
    chr: Vec<u8>,
    reg: u8,
}

impl Axrom {
    pub fn new(rom: &Rom) -> Self {
        Axrom {
            prg: rom.prg.clone(),
            chr: if rom.chr.is_empty() { vec![0u8; 8192] } else { rom.chr.clone() },
            reg: 0,
        }
    }
}

impl Mapper for Axrom {
    fn prg_read(&mut self, addr: u16) -> u8 {
        let n = self.prg.len() / 0x2000;
        if n == 0 {
            return 0;
        }
        let bank = (self.reg & 0x0F) as usize % n;
        let off = (addr - 0x8000) as usize % 0x2000;
        self.prg[bank * 0x2000 + off]
    }
    fn prg_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.reg = val;
        }
    }
    fn chr_read(&mut self, addr: u16) -> u8 {
        self.chr[(addr & 0x1FFF) as usize % self.chr.len().max(1)]
    }
    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.len() == 8192 {
            self.chr[(addr & 0x1FFF) as usize] = val;
        }
    }
    fn mirroring(&self) -> u8 {
        2 // single screen, selected in name_table
    }
    fn name_table(&self, _raw: u8, _x_right: bool) -> u8 {
        if self.reg & 0x08 != 0 {
            1
        } else {
            0
        }
    }
}

// ============================ MMC2 (9, approx) =============================

pub struct Mmc2 {
    prg: Vec<u8>,
    chr: Vec<u8>,
    reg0: u8, // 8K at $A000
    reg1: u8, // 8K at $C000
    l1a: u8,  // selected after $3F08 read
    l1b: u8,  // after $3F18
    l2a: u8,  // after $3F10
    l2b: u8,  // after $3F1B
    sel1: bool, // current first 4K latch: false=l1a
    sel2: bool,
    mode: u8,
    mir: u8,
}

impl Mmc2 {
    pub fn new(rom: &Rom) -> Self {
        Mmc2 {
            prg: rom.prg.clone(),
            chr: rom.chr.clone(),
            reg0: 0,
            reg1: 0,
            l1a: 0,
            l1b: 0,
            l2a: 0,
            l2b: 0,
            sel1: false,
            sel2: false,
            mode: 0,
            mir: 0,
        }
    }
}

impl Mapper for Mmc2 {
    fn prg_read(&mut self, addr: u16) -> u8 {
        let a = (addr - 0x8000) as usize;
        let n = self.prg.len() / 0x2000;
        if n == 0 {
            return 0;
        }
        let bank = match a / 0x2000 {
            0 => 0, // fixed first 16K
            1 => 2 * (self.reg0 as usize % n),
            2 => 2 * (self.reg0 as usize % n) + 1,
            3 => 2 * (self.reg1 as usize % n),
            4 => 2 * (self.reg1 as usize % n) + 1,
            _ => n - 1,
        };
        self.prg[(bank * 0x2000 + a % 0x2000) % self.prg.len()]
    }
    fn prg_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x8000..=0x9FFF => self.reg0 = val,
            0xA000..=0xBFFF => self.reg1 = val,
            0xC000..=0xC7FF => self.l1a = val,
            0xC800..=0xCFFF => self.l1b = val,
            0xD000..=0xD7FF => self.l2a = val,
            0xD800..=0xDFFF => self.l2b = val,
            0xE000..=0xE7FF => self.mode = val,
            0xE800..=0xFFFF => self.mir = val,
            _ => {}
        }
    }
    fn chr_read(&mut self, addr: u16) -> u8 {
        let a = (addr & 0x1FFF) as usize;
        let bank: usize = if a < 0x1000 {
            if self.mode & 1 != 0 {
                if self.sel2 {
                    self.l2b as usize
                } else {
                    self.l2a as usize
                }
            } else if self.sel1 {
                self.l1b as usize
            } else {
                self.l1a as usize
            }
        } else {
            if self.mode & 1 != 0 {
                if self.sel1 {
                    self.l1b as usize
                } else {
                    self.l1a as usize
                }
            } else if self.sel2 {
                self.l2b as usize
            } else {
                self.l2a as usize
            }
        };
        if self.chr.len() < 0x1000 {
            return self.chr.get(a % self.chr.len().max(1)).copied().unwrap_or(0);
        }
        self.chr[(bank * 0x1000 + a % 0x1000) % self.chr.len()]
    }
    fn chr_write(&mut self, _a: u16, _v: u8) {}
    fn mirroring(&self) -> u8 {
        match self.mir & 3 {
            0 => 1,
            1 => 0,
            2 => 2, // one-screen lower: handled by name_table? approximate vertical
            _ => 2,
        }
    }
    fn name_table(&self, raw: u8, x_right: bool) -> u8 {
        match self.mir & 3 {
            2 => 0,
            3 => 1,
            _ => {
                let _ = (raw, x_right);
                raw
            }
        }
    }
    fn palette_access(&mut self, addr: u16, read: bool) {
        if !read {
            return;
        }
        match addr & 0x1F {
            0x08 => self.sel1 = false,
            0x18 => self.sel1 = true,
            0x10 => self.sel2 = false,
            0x1B => self.sel2 = true,
            _ => {}
        }
    }
}

// ============================ Color Dreams (11) =============================

pub struct ColorDreams {
    prg: Vec<u8>,
    chr: Vec<u8>,
    prg_bank: u8,
    chr_bank: u8,
}

impl ColorDreams {
    pub fn new(rom: &Rom) -> Self {
        ColorDreams {
            prg: rom.prg.clone(),
            chr: if rom.chr.is_empty() { vec![0u8; 8192] } else { rom.chr.clone() },
            prg_bank: 0,
            chr_bank: 0,
        }
    }
}

impl Mapper for ColorDreams {
    fn prg_read(&mut self, addr: u16) -> u8 {
        let a = (addr - 0x8000) as usize;
        if self.prg.len() <= 0x8000 {
            return self.prg[a % self.prg.len()];
        }
        let n = self.prg.len() / 0x4000;
        let bank = self.prg_bank as usize % n;
        self.prg[bank * 0x4000 + a % 0x4000]
    }
    fn prg_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.prg_bank = val & 0x03;
            self.chr_bank = (val >> 4) & 0x03;
        }
    }
    fn chr_read(&mut self, addr: u16) -> u8 {
        let a = (addr & 0x1FFF) as usize;
        if self.chr.len() == 8192 {
            self.chr[a]
        } else {
            let n = self.chr.len() / 0x2000;
            self.chr[(self.chr_bank as usize % n) * 0x2000 + a]
        }
    }
    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.len() == 8192 {
            self.chr[(addr & 0x1FFF) as usize] = val;
        }
    }
    fn mirroring(&self) -> u8 {
        0
    }
}

// ============================ MN3065 (66) =============================

pub struct Mn3065 {
    prg: Vec<u8>,
    chr: Vec<u8>,
    prg_bank: u8,
    chr_bank: u8,
    mir: u8,
}

impl Mn3065 {
    pub fn new(rom: &Rom) -> Self {
        Mn3065 {
            prg: rom.prg.clone(),
            chr: if rom.chr.is_empty() { vec![0u8; 8192] } else { rom.chr.clone() },
            prg_bank: 0,
            chr_bank: 0,
            mir: 0,
        }
    }
}

impl Mapper for Mn3065 {
    fn prg_read(&mut self, addr: u16) -> u8 {
        let a = (addr - 0x8000) as usize;
        if a < 0x4000 {
            let n = self.prg.len() / 0x4000;
            self.prg[(self.prg_bank as usize % n.max(1)) * 0x4000 + a]
        } else {
            self.prg[(self.prg.len() - 0x4000) + (a - 0x4000)]
        }
    }
    fn prg_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.prg_bank = val & 0x1F;
            self.chr_bank = (val >> 5) & 0x01;
            self.mir = if val & 0x80 != 0 { 0 } else { 1 };
        }
    }
    fn chr_read(&mut self, addr: u16) -> u8 {
        let a = (addr & 0x1FFF) as usize;
        if self.chr.len() == 8192 {
            self.chr[a]
        } else {
            let n = self.chr.len() / 0x2000;
            self.chr[(self.chr_bank as usize % n) * 0x2000 + a]
        }
    }
    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.len() == 8192 {
            self.chr[(addr & 0x1FFF) as usize] = val;
        }
    }
    fn mirroring(&self) -> u8 {
        self.mir
    }
}

// ============================ factory =============================

pub fn create(rom: &Rom) -> Result<Box<dyn Mapper>, String> {
    let m = match rom.info.mapper {
        0 => Box::new(Nrom::new(rom)) as Box<dyn Mapper>,
        1 => Box::new(Mmc1::new(rom)),
        2 => Box::new(Uxrom::new(rom)),
        3 => Box::new(Cnrom::new(rom)),
        4 | 121 => Box::new(Mmc3::new(rom)),
        7 => Box::new(Axrom::new(rom)),
        9 => Box::new(Mmc2::new(rom)),
        11 => Box::new(ColorDreams::new(rom)),
        66 => Box::new(Mn3065::new(rom)),
        other => {
            return Err(format!(
                "暂不支持 Mapper {}（本模拟器支持 0/1/2/3/4/7/9/11/66）",
                other
            ))
        }
    };
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::{Rom, RomInfo};

    fn synth_rom(mapper: u16, prg_banks16k: usize, chr_banks8k: usize) -> Rom {
        Rom {
            info: RomInfo {
                mapper,
                submapper: 0,
                prg_banks_16k: prg_banks16k,
                chr_banks_8k: chr_banks8k,
                mirroring_vertical: false,
                battery: false,
                trainer: false,
                four_screen: false,
                prg_ram_size: 0,
                is_nes20: false,
                title: String::new(),
            },
            prg: (0..prg_banks16k).flat_map(|b| vec![b as u8; 16384]).collect(),
            chr: if chr_banks8k == 0 {
                Vec::new()
            } else {
                (0..chr_banks8k).flat_map(|b| vec![64 + b as u8; 8192]).collect()
            },
        }
    }

    #[test]
    fn mmc1_serial_bank_switch() {
        let rom = synth_rom(1, 4, 0);
        let mut m = Mmc1::new(&rom);
        // ctrl = 0b00011: PRG mode 0 (32K switch), mirroring bits=3 (horizontal)
        let ctrl = 0x03u8;
        for i in 0..5 {
            m.prg_write(0x8000, ((ctrl >> i) & 1) as u8); // $8000 block = control
        }
        assert_eq!(m.mirroring(), 0);
        // now load prg bank: mode 0 uses even 16K pairs; $E000 block = PRG
        for i in 0..5 {
            m.prg_write(0xE000, (2 >> i) & 1);
        }
        // after 5th bit, register loads: bank 2 -> $8000 should read prg[2*16K]
        assert_eq!(m.prg_read(0x8000), 2);
        assert_eq!(m.prg_read(0xC000), 3);
    }

    #[test]
    fn mmc3_banks_and_irq() {
        let rom = synth_rom(4, 8, 4); // 128K prg (8x16K), 32K chr
        let mut m = Mmc3::new(&rom);
        // $8000-$9FFF <- reg6 (8K units), $A000-$BFFF <- reg7, $C000 = second-to-last 8K, $E000 = last
        m.prg_write(0x8000, 6); // command = reg6
        m.prg_write(0x8001, 4); // 8K bank 4 -> byte offset 0x8000 -> 16K bank 2 (value 2)
        assert_eq!(m.prg_read(0x8000), 2);
        m.prg_write(0x8000, 7); // command = reg7
        m.prg_write(0x8001, 5); // 8K bank 5 -> second half of 16K bank 2
        assert_eq!(m.prg_read(0xA000), 2);
        // fixed slots: $C000 = 8K unit 14, $E000 = unit 15 (both in 16K bank 7)
        assert_eq!(m.prg_read(0xC000), 7);
        assert_eq!(m.prg_read(0xE000), 7);
        // IRQ: latch 2, arm via C002, expect request after 2 scanlines
        m.prg_write(0xC000, 2);
        m.prg_write(0xC002, 0);
        assert!(!m.irq_active());
        m.scanline(1);
        assert!(!m.irq_active());
        m.scanline(2);
        assert!(m.irq_active());
        // ack via $C001-$C003 write
        m.prg_write(0xC001, 0);
        assert!(!m.irq_active());
    }

    #[test]
    fn mmc3_chr_slots() {
        let mut rom = synth_rom(4, 2, 4); // 32K CHR = 32 x 1K units
        for i in 0..rom.chr.len() {
            rom.chr[i] = (i / 0x400) as u8;
        }
        let mut m = Mmc3::new(&rom);
        // cmd0: 2K-granular (low bit forced 0) at $0000-$07FF
        m.prg_write(0x8000, 0);
        m.prg_write(0x8001, 2);
        assert_eq!(m.chr_read(0x0000), 2);
        assert_eq!(m.chr_read(0x07FF), 3);
        // cmd1: 2K-granular at $0800-$0FFF
        m.prg_write(0x8000, 1);
        m.prg_write(0x8001, 5); // -> unit 4
        assert_eq!(m.chr_read(0x0800), 4);
        assert_eq!(m.chr_read(0x0FFF), 5);
        // cmd2: 1K at $1000-$13FF
        m.prg_write(0x8000, 2);
        m.prg_write(0x8001, 7);
        assert_eq!(m.chr_read(0x1000), 7);
    }

    #[test]
    fn mmc3_mirroring_via_ppu_raw() {
        let rom = synth_rom(4, 2, 0);
        let mut m = Mmc3::new(&rom);
        m.prg_write(0x8000, 0x40 | 0x00); // cmd0, prg swap on? bit6 swap; keep 0
        assert_eq!(m.mirroring(), 0);
    }

    #[test]
    fn cnrom_chr_switch() {
        let rom = synth_rom(3, 2, 4);
        let mut m = Cnrom::new(&rom);
        m.prg_write(0x8000, 1);
        assert_eq!(m.chr_read(0x0000), 64 + 1);
    }

    #[test]
    fn uxrom_prg_switch() {
        let rom = synth_rom(2, 8, 0);
        let mut m = Uxrom::new(&rom);
        m.prg_write(0x8000, 3);
        assert_eq!(m.prg_read(0x8000), 3);
        assert_eq!(m.prg_read(0xC000), 7); // last fixed
    }

    #[test]
    fn axrom_single_screen_mirror() {
        let rom = synth_rom(7, 4, 0);
        let mut m = Axrom::new(&rom);
        m.prg_write(0x8000, 0x80 | 1 | 0x08); // 8K unit bank1 + single upper
        // AxROM uses 8K units: bank 1
        assert_eq!(m.name_table(0, false), 1);
    }
}
