//! MOS 6502 CPU emulation: all official opcodes plus the common NMOS illegal
//! opcode families (LAX/SAX/DCP/ISC/SLO/RLA/SRE/RRA/ARR/ANC/...), NMOS decimal
//! mode, NMI/IRQ handling, JAM lockup.

pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, val: u8);
    /// Console-side: PPU produced a rising NMI edge since last check.
    fn take_nmi(&mut self) -> bool;
    /// Console-side: IRQ line asserted (e.g. MMC3 scanline IRQ).
    fn irq_line(&mut self) -> bool;
}

pub const FLAG_C: u8 = 0x01;
pub const FLAG_Z: u8 = 0x02;
pub const FLAG_I: u8 = 0x04;
pub const FLAG_D: u8 = 0x08;
pub const FLAG_B: u8 = 0x10;
pub const FLAG_U: u8 = 0x20;
pub const FLAG_V: u8 = 0x40;
pub const FLAG_N: u8 = 0x80;

#[derive(Clone, Default)]
pub struct Cpu6502 {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub p: u8,
    pub cycles: u64,
    pub halted: bool,
    nmi_latch: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum AM {
    Imp,
    Acc,
    Imm,
    Zp0,
    ZpX,
    ZpY,
    Abs,
    AbsX,
    AbsY,
    Ind,
    IdX,
    IdY,
    Rel,
}

#[derive(Clone, Copy)]
struct Op {
    mode: AM,
    cycles: u8,
}

const fn op(mode: AM, cycles: u8) -> Op {
    Op { mode, cycles }
}

const NOP: Op = op(AM::Imp, 2);

/// Opcode table: addressing mode + base cycle count.
const TABLE: [Op; 256] = {
    let mut t = [NOP; 256];

    // ---- 0x0x ----
    t[0x00] = op(AM::Imp, 7); // BRK
    t[0x01] = op(AM::IdX, 6); // ORA
    t[0x04] = op(AM::Zp0, 3); // NOP
    t[0x05] = op(AM::Zp0, 3); // ORA
    t[0x06] = op(AM::Zp0, 5); // ASL
    t[0x07] = op(AM::Zp0, 5); // SLO
    t[0x08] = op(AM::Imp, 3); // PHP
    t[0x09] = op(AM::Imm, 2); // ORA
    t[0x0A] = op(AM::Acc, 2); // ASL
    t[0x0B] = op(AM::Imm, 2); // ANC
    t[0x0C] = op(AM::Abs, 4); // NOP
    t[0x0D] = op(AM::Abs, 4); // ORA
    t[0x0E] = op(AM::Abs, 6); // ASL
    t[0x0F] = op(AM::Abs, 5); // SLO

    // ---- 0x1x ----
    t[0x10] = op(AM::Rel, 2); // BPL
    t[0x11] = op(AM::IdY, 5); // ORA
    t[0x12] = op(AM::Imm, 2); // JAM
    t[0x13] = op(AM::IdY, 5); // SLO
    t[0x14] = op(AM::ZpX, 4); // NOP
    t[0x15] = op(AM::ZpX, 4); // ORA
    t[0x16] = op(AM::ZpX, 6); // ASL
    t[0x17] = op(AM::ZpY, 5); // SLO
    t[0x18] = op(AM::Imp, 2); // CLC
    t[0x19] = op(AM::AbsY, 4); // ORA
    t[0x1A] = op(AM::Imp, 2); // NOP
    t[0x1B] = op(AM::AbsY, 4); // SLO
    t[0x1C] = op(AM::AbsX, 4); // NOP
    t[0x1D] = op(AM::AbsX, 4); // ORA
    t[0x1E] = op(AM::AbsX, 6); // ASL
    t[0x1F] = op(AM::AbsY, 4); // SLO

    // ---- 0x2x ----
    t[0x20] = op(AM::Abs, 6); // JSR
    t[0x21] = op(AM::IdX, 6); // AND
    t[0x22] = op(AM::Imm, 2); // JAM
    t[0x23] = op(AM::IdX, 6); // RLA
    t[0x24] = op(AM::Zp0, 3); // BIT
    t[0x25] = op(AM::Zp0, 3); // AND
    t[0x26] = op(AM::Zp0, 5); // ROL
    t[0x27] = op(AM::Zp0, 5); // RLA
    t[0x28] = op(AM::Imp, 4); // PLP
    t[0x29] = op(AM::Imm, 2); // AND
    t[0x2A] = op(AM::Acc, 2); // ROL
    t[0x2B] = op(AM::Imm, 2); // ANC
    t[0x2C] = op(AM::Abs, 4); // BIT
    t[0x2D] = op(AM::Abs, 4); // AND
    t[0x2E] = op(AM::Abs, 6); // ROL
    t[0x2F] = op(AM::Abs, 5); // RLA

    // ---- 0x3x ----
    t[0x30] = op(AM::Rel, 2); // BMI
    t[0x31] = op(AM::IdY, 5); // AND
    t[0x32] = op(AM::Imm, 2); // JAM
    t[0x33] = op(AM::IdY, 5); // RLA
    t[0x34] = op(AM::ZpX, 4); // NOP
    t[0x35] = op(AM::ZpX, 4); // AND
    t[0x36] = op(AM::ZpX, 6); // ROL
    t[0x37] = op(AM::ZpY, 5); // RLA
    t[0x38] = op(AM::Imp, 2); // SEC
    t[0x39] = op(AM::AbsY, 4); // AND
    t[0x3A] = op(AM::Imp, 2); // NOP
    t[0x3B] = op(AM::AbsY, 4); // RLA
    t[0x3C] = op(AM::AbsX, 4); // NOP
    t[0x3D] = op(AM::AbsX, 4); // AND
    t[0x3E] = op(AM::AbsX, 6); // ROL
    t[0x3F] = op(AM::AbsY, 4); // RLA

    // ---- 0x4x ----
    t[0x40] = op(AM::Imp, 6); // RTI
    t[0x41] = op(AM::IdX, 6); // EOR
    t[0x42] = op(AM::Imm, 2); // JAM
    t[0x43] = op(AM::IdX, 6); // SRE
    t[0x44] = op(AM::Zp0, 3); // NOP
    t[0x45] = op(AM::Zp0, 3); // EOR
    t[0x46] = op(AM::Zp0, 5); // LSR
    t[0x47] = op(AM::Zp0, 5); // SRE
    t[0x48] = op(AM::Imp, 3); // PHA
    t[0x49] = op(AM::Imm, 2); // EOR
    t[0x4A] = op(AM::Acc, 2); // LSR
    t[0x4B] = op(AM::Imm, 2); // ARR
    t[0x4C] = op(AM::Abs, 3); // JMP
    t[0x4D] = op(AM::Abs, 4); // EOR
    t[0x4E] = op(AM::Abs, 6); // LSR
    t[0x4F] = op(AM::Abs, 5); // SRE

    // ---- 0x5x ----
    t[0x50] = op(AM::Rel, 2); // BVC
    t[0x51] = op(AM::IdY, 5); // EOR
    t[0x52] = op(AM::Imm, 2); // JAM
    t[0x53] = op(AM::IdY, 5); // SRE
    t[0x54] = op(AM::ZpX, 4); // NOP
    t[0x55] = op(AM::ZpX, 4); // EOR
    t[0x56] = op(AM::ZpX, 6); // LSR
    t[0x57] = op(AM::ZpY, 5); // SRE
    t[0x58] = op(AM::Imp, 2); // CLI
    t[0x59] = op(AM::AbsY, 4); // EOR
    t[0x5A] = op(AM::Imp, 2); // NOP
    t[0x5B] = op(AM::AbsY, 4); // SRE
    t[0x5C] = op(AM::AbsX, 4); // NOP
    t[0x5D] = op(AM::AbsX, 4); // EOR
    t[0x5E] = op(AM::AbsX, 6); // LSR
    t[0x5F] = op(AM::AbsY, 4); // SRE

    // ---- 0x6x ----
    t[0x60] = op(AM::Imp, 6); // RTS
    t[0x61] = op(AM::IdX, 6); // ADC
    t[0x62] = op(AM::Imm, 2); // JAM
    t[0x63] = op(AM::IdX, 6); // RRA
    t[0x64] = op(AM::Zp0, 3); // NOP
    t[0x65] = op(AM::Zp0, 3); // ADC
    t[0x66] = op(AM::Zp0, 5); // ROR
    t[0x67] = op(AM::Zp0, 5); // RRA
    t[0x68] = op(AM::Imp, 3); // PLA
    t[0x69] = op(AM::Imm, 2); // ADC
    t[0x6A] = op(AM::Acc, 2); // ROR
    t[0x6B] = op(AM::Imm, 2); // ARR
    t[0x6C] = op(AM::Ind, 5); // JMP (ind)
    t[0x6D] = op(AM::Abs, 4); // ADC
    t[0x6E] = op(AM::Abs, 6); // ROR
    t[0x6F] = op(AM::Abs, 5); // RRA

    // ---- 0x7x ----
    t[0x70] = op(AM::Rel, 2); // BVS
    t[0x71] = op(AM::IdY, 5); // ADC
    t[0x72] = op(AM::Imm, 2); // JAM
    t[0x73] = op(AM::IdY, 5); // RRA
    t[0x74] = op(AM::ZpX, 4); // NOP
    t[0x75] = op(AM::ZpX, 4); // ADC
    t[0x76] = op(AM::ZpX, 6); // ROR
    t[0x77] = op(AM::ZpY, 5); // RRA
    t[0x78] = op(AM::Imp, 2); // SEI
    t[0x79] = op(AM::AbsY, 4); // ADC
    t[0x7A] = op(AM::Imp, 2); // NOP
    t[0x7B] = op(AM::AbsY, 4); // RRA
    t[0x7C] = op(AM::AbsX, 4); // NOP
    t[0x7D] = op(AM::AbsX, 4); // ADC
    t[0x7E] = op(AM::AbsX, 6); // ROR
    t[0x7F] = op(AM::AbsY, 4); // RRA

    // ---- 0x8x ----
    t[0x80] = op(AM::Imm, 2); // NOP
    t[0x81] = op(AM::IdX, 6); // STA
    t[0x82] = op(AM::Imm, 2); // NOP
    t[0x83] = op(AM::IdX, 6); // SAX
    t[0x84] = op(AM::Zp0, 3); // STY
    t[0x85] = op(AM::Zp0, 3); // STA
    t[0x86] = op(AM::Zp0, 3); // STX
    t[0x87] = op(AM::Zp0, 3); // SAX
    t[0x88] = op(AM::Imp, 2); // DEY
    t[0x89] = op(AM::Imm, 2); // NOP
    t[0x8A] = op(AM::Imp, 2); // TXA
    t[0x8B] = op(AM::Imm, 2); // XAA (NOP on NES)
    t[0x8C] = op(AM::Abs, 4); // STY
    t[0x8D] = op(AM::Abs, 4); // STA
    t[0x8E] = op(AM::Abs, 4); // STX
    t[0x8F] = op(AM::Abs, 4); // SAX

    // ---- 0x9x ----
    t[0x90] = op(AM::Rel, 2); // BCC
    t[0x91] = op(AM::IdY, 6); // STA
    t[0x92] = op(AM::Imm, 2); // JAM
    t[0x93] = op(AM::IdY, 6); // SHA
    t[0x94] = op(AM::ZpX, 4); // STY
    t[0x95] = op(AM::ZpX, 4); // STA
    t[0x96] = op(AM::ZpY, 4); // STX
    t[0x97] = op(AM::ZpY, 4); // SAX
    t[0x98] = op(AM::Imp, 2); // TYA
    t[0x99] = op(AM::AbsY, 5); // STA
    t[0x9A] = op(AM::Imp, 2); // TXS
    t[0x9B] = op(AM::AbsY, 5); // TAS
    t[0x9C] = op(AM::AbsX, 5); // SHY
    t[0x9D] = op(AM::AbsX, 5); // STA
    t[0x9E] = op(AM::AbsY, 5); // SHX
    t[0x9F] = op(AM::AbsY, 5); // SHA

    // ---- 0xAx ----
    t[0xA0] = op(AM::Imm, 2); // LDY
    t[0xA1] = op(AM::IdX, 6); // LDA
    t[0xA2] = op(AM::Imm, 2); // LDX
    t[0xA3] = op(AM::IdX, 6); // LAX
    t[0xA4] = op(AM::Zp0, 3); // LDY
    t[0xA5] = op(AM::Zp0, 3); // LDA
    t[0xA6] = op(AM::Zp0, 3); // LDX
    t[0xA7] = op(AM::Zp0, 3); // LAX
    t[0xA8] = op(AM::Imp, 2); // TAY
    t[0xA9] = op(AM::Imm, 2); // LDA
    t[0xAA] = op(AM::Imp, 2); // TAX
    t[0xAB] = op(AM::Imm, 2); // NOP
    t[0xAC] = op(AM::Abs, 4); // LDY
    t[0xAD] = op(AM::Abs, 4); // LDA
    t[0xAE] = op(AM::Abs, 4); // LDX
    t[0xAF] = op(AM::Abs, 4); // LAX

    // ---- 0xBx ----
    t[0xB0] = op(AM::Rel, 2); // BCS
    t[0xB1] = op(AM::IdY, 5); // LDA
    t[0xB2] = op(AM::ZpY, 4); // STA (zp),y
    t[0xB3] = op(AM::IdY, 5); // LAX
    t[0xB4] = op(AM::ZpX, 4); // LDY
    t[0xB5] = op(AM::ZpX, 4); // LDA
    t[0xB6] = op(AM::ZpY, 4); // LDX
    t[0xB7] = op(AM::ZpY, 4); // LAX
    t[0xB8] = op(AM::Imp, 2); // CLV
    t[0xB9] = op(AM::AbsY, 4); // LDA
    t[0xBA] = op(AM::Imp, 2); // TSX
    t[0xBB] = op(AM::AbsY, 4); // LAS
    t[0xBC] = op(AM::AbsX, 4); // LDY
    t[0xBD] = op(AM::AbsX, 4); // LDA
    t[0xBE] = op(AM::AbsY, 4); // LDX
    t[0xBF] = op(AM::AbsY, 4); // LAX

    // ---- 0xCx ----
    t[0xC0] = op(AM::Imm, 2); // CPY
    t[0xC1] = op(AM::IdX, 6); // CMP
    t[0xC2] = op(AM::Imm, 2); // NOP
    t[0xC3] = op(AM::IdX, 6); // DCP
    t[0xC4] = op(AM::Zp0, 3); // CPY
    t[0xC5] = op(AM::Zp0, 3); // CMP
    t[0xC6] = op(AM::Zp0, 5); // DEC
    t[0xC7] = op(AM::Zp0, 5); // DCP
    t[0xC8] = op(AM::Imp, 2); // INY
    t[0xC9] = op(AM::Imm, 2); // CMP
    t[0xCA] = op(AM::Imp, 2); // DEX
    t[0xCB] = op(AM::Imm, 2); // ANX
    t[0xCC] = op(AM::Abs, 4); // CPY
    t[0xCD] = op(AM::Abs, 4); // CMP
    t[0xCE] = op(AM::Abs, 6); // DEC
    t[0xCF] = op(AM::Abs, 5); // DCP

    // ---- 0xDx ----
    t[0xD0] = op(AM::Rel, 2); // BNE
    t[0xD1] = op(AM::IdY, 5); // CMP
    t[0xD2] = op(AM::Imm, 2); // JAM
    t[0xD3] = op(AM::IdY, 5); // DCP
    t[0xD4] = op(AM::ZpX, 4); // NOP
    t[0xD5] = op(AM::ZpX, 4); // CMP
    t[0xD6] = op(AM::ZpX, 6); // DEC
    t[0xD7] = op(AM::ZpY, 5); // DCP
    t[0xD8] = op(AM::Imp, 2); // CLD
    t[0xD9] = op(AM::AbsY, 4); // CMP
    t[0xDA] = op(AM::Imp, 2); // NOP
    t[0xDB] = op(AM::AbsY, 4); // LAS
    t[0xDC] = op(AM::AbsX, 4); // NOP
    t[0xDD] = op(AM::AbsX, 4); // CMP
    t[0xDE] = op(AM::AbsX, 6); // DEC
    t[0xDF] = op(AM::AbsY, 4); // DCP

    // ---- 0xEx ----
    t[0xE0] = op(AM::Imm, 2); // CPX
    t[0xE1] = op(AM::IdX, 6); // SBC
    t[0xE2] = op(AM::Imm, 2); // NOP
    t[0xE3] = op(AM::IdX, 6); // ISC
    t[0xE4] = op(AM::Zp0, 3); // CPX
    t[0xE5] = op(AM::Zp0, 3); // SBC
    t[0xE6] = op(AM::Zp0, 5); // INC
    t[0xE7] = op(AM::Zp0, 5); // ISC
    t[0xE8] = op(AM::Imp, 2); // INX
    t[0xE9] = op(AM::Imm, 2); // SBC
    t[0xEA] = op(AM::Imp, 2); // NOP
    t[0xEB] = op(AM::Imm, 2); // SBC
    t[0xEC] = op(AM::Abs, 4); // CPX
    t[0xED] = op(AM::Abs, 4); // SBC
    t[0xEE] = op(AM::Abs, 6); // INC
    t[0xEF] = op(AM::Abs, 5); // ISC

    // ---- 0xFx ----
    t[0xF0] = op(AM::Rel, 2); // BEQ
    t[0xF1] = op(AM::IdY, 5); // SBC
    t[0xF2] = op(AM::Imm, 2); // JAM
    t[0xF3] = op(AM::IdY, 5); // ISC
    t[0xF4] = op(AM::ZpX, 4); // NOP
    t[0xF5] = op(AM::ZpX, 4); // SBC
    t[0xF6] = op(AM::ZpX, 6); // INC
    t[0xF7] = op(AM::ZpY, 5); // ISC
    t[0xF8] = op(AM::Imp, 2); // SED
    t[0xF9] = op(AM::AbsY, 4); // SBC
    t[0xFA] = op(AM::Imp, 2); // NOP
    t[0xFB] = op(AM::AbsY, 4); // ISC
    t[0xFC] = op(AM::AbsX, 4); // NOP
    t[0xFD] = op(AM::AbsX, 4); // SBC
    t[0xFE] = op(AM::AbsX, 6); // INC
    t[0xFF] = op(AM::AbsY, 4); // ISC
    t
};

/// Opcodes that gain +1 cycle when the effective address crosses a page (reads only).
const fn page_crossable(op: u8) -> bool {
    matches!(
        op,
        0x01 | 0x05 | 0x0D | 0x11 | 0x15 | 0x19 | 0x1D
        | 0x21 | 0x25 | 0x2D | 0x31 | 0x35 | 0x39 | 0x3D
        | 0x41 | 0x45 | 0x4D | 0x51 | 0x55 | 0x59 | 0x5D
        | 0x61 | 0x65 | 0x6D | 0x71 | 0x75 | 0x79 | 0x7D
        | 0xA1 | 0xA5 | 0xA9 | 0xAD | 0xB1 | 0xB5 | 0xB9 | 0xBD
        | 0xC1 | 0xC5 | 0xC9 | 0xCD | 0xD1 | 0xD5 | 0xD9 | 0xDD
        | 0xE1 | 0xE5 | 0xE9 | 0xED | 0xF1 | 0xF5 | 0xF9 | 0xFD
        // illegal read-modify/combine that use the value
        | 0x03 | 0x07 | 0x0F | 0x13 | 0x17 | 0x1B | 0x1F
        | 0x23 | 0x27 | 0x2F | 0x33 | 0x37 | 0x3B | 0x3F
        | 0x43 | 0x47 | 0x4F | 0x53 | 0x57 | 0x5B | 0x5F
        | 0x63 | 0x67 | 0x6F | 0x73 | 0x77 | 0x7B | 0x7F
        | 0xC3 | 0xC7 | 0xCF | 0xD3 | 0xD7 | 0xDB | 0xDF
        | 0xE3 | 0xE7 | 0xEF | 0xF3 | 0xF7 | 0xFB | 0xFF
        | 0xA3 | 0xA7 | 0xAF | 0xB3 | 0xB7 | 0xBF
        | 0xBB | 0xCB
    )
}

impl Cpu6502 {
    pub fn new() -> Self {
        Self {
            p: FLAG_U | FLAG_I,
            sp: 0xFD,
            ..Default::default()
        }
    }

    #[inline]
    fn flag(&self, f: u8) -> bool {
        self.p & f != 0
    }
    #[inline]
    fn set_flag(&mut self, f: u8, on: bool) {
        self.p = if on { self.p | f } else { self.p & !f };
    }
    #[inline]
    fn zn(&mut self, v: u8) {
        self.set_flag(FLAG_Z, v == 0);
        self.set_flag(FLAG_N, v & 0x80 != 0);
    }

    #[inline]
    fn fetch8(&mut self, bus: &mut impl Bus) -> u8 {
        let v = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }
    #[inline]
    fn fetch16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch8(bus) as u16;
        let hi = self.fetch8(bus) as u16;
        (hi << 8) | lo
    }
    #[inline]
    fn push8(&mut self, bus: &mut impl Bus, v: u8) {
        bus.write(0x0100 | self.sp as u16, v);
        self.sp = self.sp.wrapping_sub(1);
    }
    #[inline]
    fn pop8(&mut self, bus: &mut impl Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.read(0x0100 | self.sp as u16)
    }
    fn push16(&mut self, bus: &mut impl Bus, v: u16) {
        self.push8(bus, (v >> 8) as u8);
        self.push8(bus, v as u8);
    }
    fn pop16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.pop8(bus) as u16;
        let hi = self.pop8(bus) as u16;
        (hi << 8) | lo
    }
    #[inline]
    fn read16(&mut self, bus: &mut impl Bus, a: u16) -> u16 {
        let lo = bus.read(a) as u16;
        let hi = bus.read(a.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    pub fn reset(&mut self, bus: &mut impl Bus) {
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.sp = 0xFD;
        self.p = FLAG_U | FLAG_I;
        self.cycles = 0;
        self.halted = false;
        self.nmi_latch = false;
        bus.read(0xFFFA);
        bus.read(0xFFFB);
        self.pc = self.read16(bus, 0xFFFC);
    }

    pub fn nmi(&mut self) {
        self.nmi_latch = true;
    }

    fn do_interrupt(&mut self, bus: &mut impl Bus, vector: u16) -> u32 {
        self.push16(bus, self.pc);
        self.push8(bus, (self.p & !FLAG_B) | FLAG_U);
        self.set_flag(FLAG_I, true);
        self.set_flag(FLAG_D, false);
        self.pc = self.read16(bus, vector);
        7
    }

    /// Execute one instruction, returns cycles consumed.
    pub fn step(&mut self, bus: &mut impl Bus) -> u32 {
        if self.halted {
            self.cycles += 1;
            return 1;
        }

        if self.nmi_latch || bus.take_nmi() {
            self.nmi_latch = false;
            let c = self.do_interrupt(bus, 0xFFFA);
            self.cycles += c as u64;
            return c;
        }
        if !self.flag(FLAG_I) && bus.irq_line() {
            let c = self.do_interrupt(bus, 0xFFFE);
            self.cycles += c as u64;
            return c;
        }

        let opcode = self.fetch8(bus);
        let entry = TABLE[opcode as usize];
        let mut cycles = entry.cycles as u32;

        let mut crossed = false;
        let addr: u16 = match entry.mode {
            AM::Imp | AM::Acc => 0,
            AM::Imm => {
                let a = self.pc;
                self.pc = self.pc.wrapping_add(1);
                a
            }
            AM::Rel => {
                let off = self.fetch8(bus) as i8;
                let from = self.pc;
                let to = self.pc.wrapping_add(off as u16);
                crossed = (from >> 8) != (to >> 8);
                to
            }
            AM::Zp0 => {
                let lo = self.fetch8(bus) as u16;
                lo & 0xFF
            }
            AM::ZpX => {
                let lo = self.fetch8(bus) as u16;
                (lo + self.x as u16) & 0xFF
            }
            AM::ZpY => {
                let lo = self.fetch8(bus) as u16;
                (lo + self.y as u16) & 0xFF
            }
            AM::Abs => self.fetch16(bus),
            AM::AbsX => {
                let base = self.fetch16(bus);
                let a = base.wrapping_add(self.x as u16);
                crossed = (base & 0xFF) + self.x as u16 > 0xFF;
                a
            }
            AM::AbsY => {
                let base = self.fetch16(bus);
                let a = base.wrapping_add(self.y as u16);
                crossed = (base & 0xFF) + self.y as u16 > 0xFF;
                a
            }
            AM::Ind => {
                let ptr = self.fetch16(bus);
                let lo = bus.read(ptr) as u16;
                let hi = bus.read((ptr & 0xFF00) | (ptr.wrapping_add(1) & 0x00FF)) as u16;
                (hi << 8) | lo
            }
            AM::IdX => {
                let ptr = ((self.fetch8(bus) as u16 + self.x as u16) & 0xFF) as u16;
                self.read16(bus, ptr)
            }
            AM::IdY => {
                let ptr = self.fetch8(bus) as u16;
                let base = self.read16(bus, ptr);
                let a = base.wrapping_add(self.y as u16);
                crossed = (base & 0xFF) + self.y as u16 > 0xFF;
                a
            }
        };

        if crossed && page_crossable(opcode) {
            cycles += 1;
        }

        let mut take_branch = false;

        // read-modify-write with a single write-back (safe for write-only regs)
        macro_rules! rmw {
            ($f:expr) => {{
                let old = bus.read(addr);
                let (new, extra) = $f(self, old);
                bus.write(addr, new);
                cycles += extra;
                new
            }};
        }

        match opcode {
            // ================= branches =================
            0x10 => take_branch = !self.flag(FLAG_N),
            0x30 => take_branch = self.flag(FLAG_N),
            0x50 => take_branch = !self.flag(FLAG_V),
            0x70 => take_branch = self.flag(FLAG_V),
            0x90 => take_branch = !self.flag(FLAG_C),
            0xB0 => take_branch = self.flag(FLAG_C),
            0xD0 => take_branch = !self.flag(FLAG_Z),
            0xF0 => take_branch = self.flag(FLAG_Z),

            // ================= loads =================
            0xA1 | 0xA5 | 0xA9 | 0xAD | 0xB1 | 0xB5 | 0xB9 | 0xBD => {
                let v = bus.read(addr);
                self.a = v;
                self.zn(v);
            }
            0xA2 | 0xA6 | 0xB6 | 0xAE | 0xBE => {
                let v = bus.read(addr);
                self.x = v;
                self.zn(v);
            }
            0xA0 | 0xA4 | 0xB4 | 0xAC | 0xBC => {
                let v = bus.read(addr);
                self.y = v;
                self.zn(v);
            }
            // LAX
            0xA3 | 0xA7 | 0xAF | 0xB3 | 0xB7 | 0xBF => {
                let v = bus.read(addr);
                self.a = v;
                self.x = v;
                self.zn(v);
            }
            // LAS
            0xBB | 0xDB => {
                let v = bus.read(addr) & self.sp;
                self.a = v;
                self.x = v;
                self.sp = v;
                self.zn(v);
            }

            // ================= stores =================
            0x81 | 0x85 | 0x8D | 0x91 | 0x95 | 0x99 | 0x9D | 0xB2 => {
                bus.write(addr, self.a);
            }
            0x86 | 0x8E | 0x96 => bus.write(addr, self.x),
            0x84 | 0x8C | 0x94 => bus.write(addr, self.y),
            // SAX
            0x83 | 0x87 | 0x8F | 0x97 => bus.write(addr, self.a & self.x),
            // SHA
            0x93 | 0x9F => {
                let v = (self.a & self.x) & ((addr >> 8) as u8).wrapping_add(1);
                bus.write(addr.wrapping_add(if opcode == 0x93 { (self.y as u16) & 0 } else { 0 }), v);
                self.sp = v;
            }
            // SHY
            0x9C => {
                let v = self.y & ((addr >> 8) as u8).wrapping_add(1);
                bus.write(addr, v);
            }
            // SHX
            0x9E => {
                let v = self.x & ((addr >> 8) as u8).wrapping_add(1);
                bus.write(addr, v);
            }
            // TAS
            0x9B => {
                let v = (self.a & self.x) & ((addr >> 8) as u8).wrapping_add(1);
                self.sp = v;
                bus.write(addr, v);
            }

            // ================= arithmetic =================
            0x61 | 0x65 | 0x69 | 0x6D | 0x71 | 0x75 | 0x79 | 0x7D | 0xEB => {
                let v = bus.read(addr);
                self.adc(v);
            }
            0xE1 | 0xE5 | 0xE9 | 0xED | 0xF1 | 0xF5 | 0xF9 | 0xFD => {
                let v = bus.read(addr);
                self.sbc(v);
            }
            0x21 | 0x25 | 0x29 | 0x2D | 0x31 | 0x35 | 0x39 | 0x3D => {
                let v = bus.read(addr);
                self.a &= v;
                self.zn(self.a);
            }
            0x01 | 0x05 | 0x09 | 0x0D | 0x11 | 0x15 | 0x19 | 0x1D => {
                let v = bus.read(addr);
                self.a |= v;
                self.zn(self.a);
            }
            0x41 | 0x45 | 0x49 | 0x4D | 0x51 | 0x55 | 0x59 | 0x5D => {
                let v = bus.read(addr);
                self.a ^= v;
                self.zn(self.a);
            }
            0x24 | 0x2C => {
                let v = bus.read(addr);
                self.set_flag(FLAG_Z, self.a & v == 0);
                self.set_flag(FLAG_N, v & 0x80 != 0);
                self.set_flag(FLAG_V, v & 0x40 != 0);
            }
            // CMP / CPX / CPY
            0xC1 | 0xC5 | 0xC9 | 0xCD | 0xD1 | 0xD5 | 0xD9 | 0xDD => {
                let v = bus.read(addr);
                let diff = self.a.wrapping_sub(v);
                self.set_flag(FLAG_C, self.a >= v);
                self.set_flag(FLAG_Z, diff == 0);
                self.set_flag(FLAG_N, diff & 0x80 != 0);
            }
            0xE0 | 0xE4 | 0xEC => {
                let v = bus.read(addr);
                let diff = self.x.wrapping_sub(v);
                self.set_flag(FLAG_C, self.x >= v);
                self.set_flag(FLAG_Z, diff == 0);
                self.set_flag(FLAG_N, diff & 0x80 != 0);
            }
            0xC0 | 0xC4 | 0xCC => {
                let v = bus.read(addr);
                let diff = self.y.wrapping_sub(v);
                self.set_flag(FLAG_C, self.y >= v);
                self.set_flag(FLAG_Z, diff == 0);
                self.set_flag(FLAG_N, diff & 0x80 != 0);
            }

            // ================= accumulator shifts =================
            0x0A => {
                self.set_flag(FLAG_C, self.a & 0x80 != 0);
                self.a <<= 1;
                self.zn(self.a);
            }
            0x4A => {
                self.set_flag(FLAG_C, self.a & 1 != 0);
                self.a >>= 1;
                self.zn(self.a);
            }
            0x2A => {
                let c = self.flag(FLAG_C);
                self.set_flag(FLAG_C, self.a & 0x80 != 0);
                self.a = (self.a << 1) | c as u8;
                self.zn(self.a);
            }
            0x6A => {
                let c = self.flag(FLAG_C);
                self.set_flag(FLAG_C, self.a & 1 != 0);
                self.a = (self.a >> 1) | (c as u8 * 0x80);
                self.zn(self.a);
            }

            // ================= memory shifts (RMW) =================
            0x06 | 0x0E | 0x16 | 0x1E => {
                rmw!(|c: &mut Self, v: u8| {
                    c.set_flag(FLAG_C, v & 0x80 != 0);
                    let n = v << 1;
                    c.zn(n);
                    (n, 0)
                });
            }
            0x46 | 0x4E | 0x56 | 0x5E => {
                rmw!(|c: &mut Self, v: u8| {
                    c.set_flag(FLAG_C, v & 1 != 0);
                    let n = v >> 1;
                    c.zn(n);
                    (n, 0)
                });
            }
            0x26 | 0x2E | 0x36 | 0x3E => {
                rmw!(|c: &mut Self, v: u8| {
                    let cin = c.flag(FLAG_C);
                    c.set_flag(FLAG_C, v & 0x80 != 0);
                    let n = (v << 1) | cin as u8;
                    c.zn(n);
                    (n, 0)
                });
            }
            0x66 | 0x6E | 0x76 | 0x7E => {
                rmw!(|c: &mut Self, v: u8| {
                    let cin = c.flag(FLAG_C);
                    c.set_flag(FLAG_C, v & 1 != 0);
                    let n = (v >> 1) | (cin as u8 * 0x80);
                    c.zn(n);
                    (n, 0)
                });
            }
            0xE6 | 0xEE | 0xF6 | 0xFE => {
                rmw!(|c: &mut Self, v: u8| {
                    let n = v.wrapping_add(1);
                    c.zn(n);
                    (n, 0)
                });
            }
            0xC6 | 0xCE | 0xD6 | 0xDE => {
                rmw!(|c: &mut Self, v: u8| {
                    let n = v.wrapping_sub(1);
                    c.zn(n);
                    (n, 0)
                });
            }

            // ================= illegal RMW combos =================
            // SLO = ASL + ORA
            0x03 | 0x07 | 0x0F | 0x13 | 0x17 | 0x1B | 0x1F => {
                let n = rmw!(|c: &mut Self, v: u8| {
                    c.set_flag(FLAG_C, v & 0x80 != 0);
                    (v << 1, 0)
                });
                self.a |= n;
                self.zn(self.a);
            }
            // RLA = ROL + AND
            0x23 | 0x27 | 0x2F | 0x33 | 0x37 | 0x3B | 0x3F => {
                let n = rmw!(|c: &mut Self, v: u8| {
                    let cin = c.flag(FLAG_C);
                    c.set_flag(FLAG_C, v & 0x80 != 0);
                    ((v << 1) | cin as u8, 0)
                });
                self.a &= n;
                self.zn(self.a);
            }
            // SRE = LSR + EOR
            0x43 | 0x47 | 0x4F | 0x53 | 0x57 | 0x5B | 0x5F => {
                let n = rmw!(|c: &mut Self, v: u8| {
                    c.set_flag(FLAG_C, v & 1 != 0);
                    (v >> 1, 0)
                });
                self.a ^= n;
                self.zn(self.a);
            }
            // RRA = ROR + ADC
            0x63 | 0x67 | 0x6F | 0x73 | 0x77 | 0x7B | 0x7F => {
                let n = rmw!(|c: &mut Self, v: u8| {
                    let cin = c.flag(FLAG_C);
                    c.set_flag(FLAG_C, v & 1 != 0);
                    ((v >> 1) | (cin as u8 * 0x80), 0)
                });
                self.adc(n);
            }
            // DCP = DEC + CMP
            0xC3 | 0xC7 | 0xCF | 0xD3 | 0xD7 | 0xDF => {
                let n = rmw!(|_: &mut Self, v: u8| (v.wrapping_sub(1), 0));
                let diff = self.a.wrapping_sub(n);
                self.set_flag(FLAG_C, self.a >= n);
                self.set_flag(FLAG_Z, diff == 0);
                self.set_flag(FLAG_N, diff & 0x80 != 0);
            }
            // ISC = INC + SBC
            0xE3 | 0xE7 | 0xEF | 0xF3 | 0xF7 | 0xFB | 0xFF => {
                let n = rmw!(|_: &mut Self, v: u8| (v.wrapping_add(1), 0));
                self.sbc(n);
            }

            // ================= special imm illegal =================
            // ANC: AND imm then copy N to C
            0x0B | 0x2B => {
                let v = bus.read(addr);
                self.a &= v;
                self.zn(self.a);
                self.set_flag(FLAG_C, self.flag(FLAG_N));
            }
            // ARR: AND imm then ROR
            0x4B | 0x6B => {
                let v = bus.read(addr);
                self.a &= v;
                let c = self.flag(FLAG_C);
                let mut r = self.a >> 1;
                if c {
                    r |= 0x80;
                }
                self.a = r;
                self.zn(self.a);
                let bit6 = (self.a >> 6) & 1;
                let bit5 = (self.a >> 5) & 1;
                self.set_flag(FLAG_V, bit6 ^ bit5 == 1);
                self.set_flag(FLAG_C, bit6 == 1);
            }
            // ANX / XAA: NOP-ish with value
            0xCB => {
                let v = bus.read(addr);
                self.x = self.a & self.x & v;
                self.zn(self.x);
            }
            0x8B => {
                let _ = bus.read(addr);
            }

            // ================= transfers =================
            0x8A => {
                self.a = self.x;
                self.zn(self.a);
            }
            0x98 => {
                self.a = self.y;
                self.zn(self.a);
            }
            0xA8 => {
                self.y = self.a;
                self.zn(self.y);
            }
            0xAA => {
                self.x = self.a;
                self.zn(self.x);
            }
            0xBA => {
                self.x = self.sp;
                self.zn(self.x);
            }
            0x9A => self.sp = self.x,

            0xCA => {
                self.x = self.x.wrapping_sub(1);
                self.zn(self.x);
            }
            0xE8 => {
                self.x = self.x.wrapping_add(1);
                self.zn(self.x);
            }
            0x88 => {
                self.y = self.y.wrapping_sub(1);
                self.zn(self.y);
            }
            0xC8 => {
                self.y = self.y.wrapping_add(1);
                self.zn(self.y);
            }

            // ================= flags =================
            0x18 => self.set_flag(FLAG_C, false),
            0x38 => self.set_flag(FLAG_C, true),
            0x58 => self.set_flag(FLAG_I, false),
            0x78 => self.set_flag(FLAG_I, true),
            0xB8 => self.set_flag(FLAG_V, false),
            0xD8 => self.set_flag(FLAG_D, false),
            0xF8 => self.set_flag(FLAG_D, true),

            // ================= stack =================
            0x08 => self.push8(bus, self.p | FLAG_B | FLAG_U),
            0x28 => self.p = (self.pop8(bus) & !FLAG_B) | FLAG_U,
            0x48 => self.push8(bus, self.a),
            0x68 => {
                self.a = self.pop8(bus);
                self.zn(self.a);
            }

            // ================= control =================
            0x00 => {
                // BRK
                self.pc = self.pc.wrapping_add(1);
                self.push16(bus, self.pc);
                self.push8(bus, self.p | FLAG_B | FLAG_U);
                self.set_flag(FLAG_I, true);
                self.set_flag(FLAG_D, false);
                self.pc = self.read16(bus, 0xFFFE);
            }
            0x40 => {
                self.p = (self.pop8(bus) & !FLAG_B) | FLAG_U;
                self.pc = self.pop16(bus);
            }
            0x20 => {
                // JSR: operand already consumed by Abs addressing
                self.push16(bus, self.pc.wrapping_sub(1));
                self.pc = addr;
            }
            0x4C => self.pc = addr,
            0x6C => self.pc = addr,
            0x60 => {
                let lo = self.pop8(bus) as u16;
                let hi = self.pop8(bus) as u16;
                self.pc = ((hi << 8) | lo).wrapping_add(1);
            }

            // ================= JAM =================
            0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xD2 | 0xF2 => {
                self.halted = true;
            }

            // ================= NOP =================
            0x04 | 0x0C | 0x14 | 0x1A | 0x1C | 0x34 | 0x3A | 0x3C | 0x44 | 0x54 | 0x5A | 0x5C
            | 0x64 | 0x74 | 0x7A | 0x7C | 0x80 | 0x82 | 0x89 | 0xAB | 0xC2 | 0xE2 | 0xDA | 0xEA
            | 0xFA => {
                let _ = bus.read(addr);
            }

            _ => {}
        }

        if take_branch {
            self.pc = addr;
            cycles += 1;
            if crossed {
                cycles += 1;
            }
        }

        self.cycles += cycles as u64;
        cycles
    }

    fn adc(&mut self, v: u8) {
        if self.flag(FLAG_D) {
            let a = self.a;
            let c = self.flag(FLAG_C) as u8;
            let mut lo = (a & 0x0f) + (v & 0x0f) + c;
            let mut hi = (a >> 4) + (v >> 4);
            let mut carry = false;
            if lo > 9 {
                hi += 1;
                lo = (lo + 6) & 0x0F;
            }
            if hi > 9 {
                hi = (hi + 6) & 0x0F;
                carry = true;
            }
            let res = ((hi << 4) | (lo & 0x0f)) & 0xFF;
            self.set_flag(FLAG_V, ((a ^ v) & (a ^ res) & 0x80) != 0);
            self.set_flag(FLAG_C, carry);
            self.a = res;
            self.zn(self.a);
        } else {
            let sum = self.a as u16 + v as u16 + self.flag(FLAG_C) as u16;
            self.set_flag(FLAG_V, (!(self.a ^ v) & (self.a ^ (sum as u8)) & 0x80) != 0);
            self.set_flag(FLAG_C, sum > 0xFF);
            self.a = sum as u8;
            self.zn(self.a);
        }
    }

    fn sbc(&mut self, v: u8) {
        if self.flag(FLAG_D) {
            let a = self.a;
            let borrow = (!self.flag(FLAG_C)) as u8;
            let mut lo = (a & 0x0f) as i16 - (v & 0x0f) as i16 - borrow as i16;
            let mut hi = (a >> 4) as i16 - (v >> 4) as i16;
            if lo < 0 {
                lo -= 6;
                hi -= 1;
            }
            if hi < 0 {
                hi -= 6;
            }
            let borrow_out = hi < 0;
            let res = (((hi & 0x0f) << 4) | (lo & 0x0f)) as u8;
            self.set_flag(FLAG_V, ((a ^ v) & (a ^ res) & 0x80) != 0);
            self.set_flag(FLAG_C, !borrow_out);
            self.a = res;
            self.zn(self.a);
        } else {
            self.adc(!v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ram {
        m: [u8; 0x10000],
    }
    impl Bus for Ram {
        fn read(&mut self, a: u16) -> u8 {
            self.m[a as usize]
        }
        fn write(&mut self, a: u16, v: u8) {
            self.m[a as usize] = v;
        }
        fn take_nmi(&mut self) -> bool {
            false
        }
        fn irq_line(&mut self) -> bool {
            false
        }
    }
    fn ram() -> Ram {
        Ram { m: [0; 0x10000] }
    }

    #[test]
    fn lda_imm_flags() {
        let mut bus = ram();
        bus.m[0x0000] = 0xA9;
        bus.m[0x0001] = 0x00;
        let mut c = Cpu6502::new();
        c.step(&mut bus);
        assert_eq!(c.a, 0);
        assert!(c.flag(FLAG_Z));
        assert_eq!(c.pc, 0x0002);
    }

    #[test]
    fn adc_basic() {
        let mut bus = ram();
        // LDA #$80 ; CLC ; ADC #$80 => 0 with V and C
        bus.m[0x0000] = 0xA9;
        bus.m[0x0001] = 0x80;
        bus.m[0x0002] = 0x18;
        bus.m[0x0003] = 0x69;
        bus.m[0x0004] = 0x80;
        let mut c = Cpu6502::new();
        c.step(&mut bus);
        c.step(&mut bus);
        c.step(&mut bus);
        assert_eq!(c.a, 0x00);
        assert!(c.flag(FLAG_C));
        assert!(c.flag(FLAG_V));
        assert!(c.flag(FLAG_Z));
    }

    #[test]
    fn adc_decimal() {
        let mut bus = ram();
        bus.m[0x0000] = 0xF8; // SED
        bus.m[0x0001] = 0xA9;
        bus.m[0x0002] = 0x09;
        bus.m[0x0003] = 0x69;
        bus.m[0x0004] = 0x01;
        let mut c = Cpu6502::new();
        c.step(&mut bus);
        c.step(&mut bus);
        c.step(&mut bus);
        assert_eq!(c.a, 0x10, "BCD 09+01=10");
        assert!(!c.flag(FLAG_C));
    }

    #[test]
    fn branch_taken_page_cycle() {
        let mut bus = ram();
        bus.m[0x00FF] = 0xF0; // BEQ rel8
        bus.m[0x0100] = 0xFE; // -2 -> back to 0x00FF (crosses page)
        let mut c = Cpu6502::new();
        c.p |= FLAG_Z;
        c.pc = 0x00FF;
        let cy = c.step(&mut bus);
        assert_eq!(cy, 2 + 1 + 1);
        assert_eq!(c.pc, 0x00FF);
    }

    #[test]
    fn jmp_ind_page_bug() {
        let mut bus = ram();
        bus.m[0x0000] = 0x6C;
        bus.m[0x0001] = 0xFF;
        bus.m[0x0002] = 0x02; // ptr 0x02FF: lo at 0x02FF, hi should be at 0x0200 (bug)
        bus.m[0x02FF] = 0x34;
        bus.m[0x0200] = 0x12;
        let mut c = Cpu6502::new();
        c.step(&mut bus);
        assert_eq!(c.pc, 0x1234);
    }

    #[test]
    fn nmi_service() {
        struct B {
            m: [u8; 0x10000],
            nmi: bool,
        }
        impl Bus for B {
            fn read(&mut self, a: u16) -> u8 {
                self.m[a as usize]
            }
            fn write(&mut self, a: u16, v: u8) {
                self.m[a as usize] = v;
            }
            fn take_nmi(&mut self) -> bool {
                std::mem::take(&mut self.nmi)
            }
            fn irq_line(&mut self) -> bool {
                false
            }
        }
        let mut bus = B {
            m: [0; 0x10000],
            nmi: true,
        };
        bus.m[0xFFFA] = 0x00;
        bus.m[0xFFFB] = 0x80;
        let mut c = Cpu6502::new();
        let _ = c.step(&mut bus);
        assert_eq!(c.pc, 0x8000);
    }

    #[test]
    fn brk_and_rti() {
        let mut bus = ram();
        bus.m[0x0000] = 0x00; // BRK
        bus.m[0xFFFE] = 0x00;
        bus.m[0xFFFF] = 0x40; // handler at 0x4000: RTI
        bus.m[0x4000] = 0x40;
        let mut c = Cpu6502::new();
        c.step(&mut bus);
        assert_eq!(c.pc, 0x4000);
        assert!(c.flag(FLAG_I));
        c.step(&mut bus);
        assert_eq!(c.pc, 0x0002); // BRK consumes operand byte
    }

    #[test]
    fn rmw_single_write_pulse_reg() {
        // INC on $10 should read 0 write 1
        let mut bus = ram();
        bus.m[0x0000] = 0xE6;
        bus.m[0x0001] = 0x10;
        let mut c = Cpu6502::new();
        c.step(&mut bus);
        assert_eq!(bus.m[0x10], 1);
        assert_eq!(c.cycles, 5);
    }
}
