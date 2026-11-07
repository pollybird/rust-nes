//! NES PPU (2C02) emulation.
//!
//! Rendering granularity is one scanline: registers written mid-frame take
//! effect from the next scanline. This covers split-screen scrolling driven by
//! MMC3 scanline IRQs, which is what commercial games use.

use crate::mapper::Mapper;

pub const FB_WIDTH: usize = 256;
pub const FB_HEIGHT: usize = 240;

const PPU_NMI: u8 = 0x80; // ctrl bit3
const M_GREY: u8 = 0x01;
const M_BG_LEFT: u8 = 0x02;
const M_SP_LEFT: u8 = 0x04;
const M_BG: u8 = 0x08;
const M_SP: u8 = 0x10;

/// Approximation of the classic 2C02 NTSC composite palette (64 entries).
pub const NES_PALETTE: [u32; 64] = [
    0x747474, 0x24188C, 0x0000A8, 0x44009C, 0x8C0074, 0xA80010, 0xA40000, 0x7C2C00,
    0x503000, 0x0C4400, 0x006C00, 0x00842C, 0x00706C, 0x000000, 0x000000, 0x000000,
    0xBCBCBC, 0x0070EC, 0x2008E4, 0x6000E4, 0xB000D0, 0xE40060, 0xE43400, 0xB86C00,
    0x888800, 0x349000, 0x00A800, 0x00A844, 0x009494, 0x000000, 0x000000, 0x000000,
    0xFCECFC, 0x3CBCFC, 0x5C94FC, 0xCC88FC, 0xF478FC, 0xFC74B4, 0xFC7460, 0xFC9838,
    0xF0BC3C, 0x80D010, 0x4CDC48, 0x58F898, 0x00E8D8, 0x787878, 0x000000, 0x000000,
    0xFCFCFC, 0xA8E4FC, 0xC4D4FC, 0xD4C8FC, 0xFCC4FC, 0xFCC4D8, 0xFCBCB0, 0xFCD8A8,
    0xFCE4A0, 0xE0FCA0, 0xA8F0BC, 0xB0FCCC, 0x9CFCF0, 0xC4C4C4, 0x000000, 0x000000,
];

pub struct Ppu {
    pub ctrl: u8,
    pub mask: u8,
    pub status: u8,
    pub oam_addr: u8,

    pub vram: [u8; 4096],
    pub oam: [u8; 256],
    pub pal: [u8; 32],

    /// current video-address register, bits: 0-4 coarse x, 5-9 coarse y,
    /// 10 vertical-nametable bit, 11-13 fine y.
    pub v: u16,
    /// temporary address latched to v during vblank.
    pub t: u16,
    /// fine x scroll (0..7).
    pub x: u8,
    write_flip: bool,
    read_buf: u8,

    pub line: u16, // 0..=261; 0..239 visible, 240..260 vblank, 261 prerender
    pub frame: u64,
    pub frame_complete: bool,

    nmi_pending: bool,
    nmi_level_prev: bool,

    bg_color: [u8; FB_WIDTH],
    sp_color: [u8; FB_WIDTH],
    sp_prio: [bool; FB_WIDTH],
    sp_present: [bool; FB_WIDTH],
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

impl Ppu {
    pub fn new() -> Self {
        Ppu {
            ctrl: 0,
            mask: 0,
            status: 0,
            oam_addr: 0,
            vram: [0; 4096],
            oam: [0; 256],
            pal: [0x3F; 32],
            v: 0,
            t: 0,
            x: 0,
            write_flip: false,
            read_buf: 0,
            line: 0,
            frame: 0,
            frame_complete: false,
            nmi_pending: false,
            nmi_level_prev: false,
            bg_color: [0; FB_WIDTH],
            sp_color: [0; FB_WIDTH],
            sp_prio: [false; FB_WIDTH],
            sp_present: [false; FB_WIDTH],
        }
    }

    pub fn reset(&mut self) {
        self.ctrl = 0;
        self.mask = 0;
        self.status = 0;
        self.oam_addr = 0;
        self.v = 0;
        self.t = 0;
        self.x = 0;
        self.write_flip = false;
        self.read_buf = 0;
        self.line = 0;
        self.frame_complete = false;
        self.nmi_pending = false;
        self.nmi_level_prev = false;
    }

    #[inline]
    fn nmi_enable(&self) -> bool {
        self.ctrl & PPU_NMI != 0
    }
    #[inline]
    fn vblank(&self) -> bool {
        self.status & 0x80 != 0
    }

    fn update_nmi_edge(&mut self) {
        let level = self.vblank() && self.nmi_enable();
        if level && !self.nmi_level_prev {
            self.nmi_pending = true;
        }
        self.nmi_level_prev = level;
    }

    /// Consumed by the CPU via Bus::take_nmi.
    pub fn take_nmi(&mut self) -> bool {
        let b = self.nmi_pending;
        self.nmi_pending = false;
        b
    }

    // ---------------------------------------------------------------- regs

    pub fn reg_read(&mut self, reg: u8, mapper: &mut dyn Mapper) -> u8 {
        match reg & 7 {
            2 => {
                let r = self.status;
                self.status &= 0x7F;
                self.write_flip = false;
                self.nmi_pending = false;
                self.update_nmi_edge();
                r
            }
            4 => self.oam[self.oam_addr as usize],
            7 => {
                let ret = self.read_buf;
                let addr = self.v & 0x3FFF;
                self.read_buf = self.mem_read(addr, mapper);
                self.v = self.v.wrapping_add(if self.ctrl & 4 != 0 { 32 } else { 1 });
                ret
            }
            _ => self.read_buf,
        }
    }

    pub fn reg_write(&mut self, reg: u8, val: u8, mapper: &mut dyn Mapper) {
        match reg & 7 {
            0 => {
                self.ctrl = val;
                self.t = (self.t & 0xF3FF) | (((val as u16) & 0x03) << 10);
                self.update_nmi_edge();
            }
            1 => self.mask = val,
            3 => self.oam_addr = val,
            4 => {
                self.oam[self.oam_addr as usize] = val;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            5 => {
                if !self.write_flip {
                    self.x = val & 7;
                    self.t = (self.t & 0xFFE0) | ((val >> 3) as u16);
                } else {
                    self.t = (self.t & 0xF41F)
                        | ((((val >> 3) as u16) & 0x1F) << 5)
                        | ((val as u16 & 1) << 11);
                }
                self.write_flip = !self.write_flip;
            }
            6 => {
                if !self.write_flip {
                    self.t = (self.t & 0x00FF) | ((val as u16) << 8);
                } else {
                    self.t = (self.t & 0xFF00) | (val as u16);
                    self.v = self.t & 0x7FFF;
                }
                self.write_flip = !self.write_flip;
            }
            7 => {
                let addr = self.v & 0x3FFF;
                self.mem_write(addr, val, mapper);
                self.v = self.v.wrapping_add(if self.ctrl & 4 != 0 { 32 } else { 1 });
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ ppumem

    /// Map PPU address space read (0x0000-0x3FFF).
    pub fn mem_read(&mut self, addr: u16, mapper: &mut dyn Mapper) -> u8 {
        match addr & 0x3FFF {
            0x0000..=0x1FFF => mapper.chr_read(addr & 0x1FFF),
            0x2000..=0x3EFF => {
                let nt_addr = addr - 0x2000;
                let coarse_x = (nt_addr % 32) as u8; // 0..31
                let coarse_y = (nt_addr / 32 % 30) as u8;
                let raw = if nt_addr >= 0x800 {
                    if nt_addr < 0xC00 {
                        2
                    } else {
                        3
                    }
                } else if nt_addr >= 0x400 {
                    1
                } else {
                    0
                };
                let _ = (coarse_x, coarse_y);
                let phys = mapper.name_table(raw, (coarse_x & 0x1F) >= 16);
                let mirror = mapper.mirroring();
                let final_phys = if phys != raw {
                    phys // mapper explicitly overrides
                } else {
                    match mirror {
                        0 => raw >> 1, // horizontal mirroring: pages stacked vertically
                        1 => raw & 1,  // vertical mirroring: pages side by side
                        _ => raw,
                    }
                };
                self.vram[(final_phys as usize) * 0x400 + (nt_addr & 0x3FF) as usize]
            }
            0x3F00..=0x3FFF => {
                let i = (addr & 0x1F) as usize;
                mapper.palette_access(addr & 0x3F00, true);
                if i >= 0x10 && (i & 3) == 0 {
                    self.pal[i - 0x10]
                } else {
                    self.pal[i]
                }
            }
            _ => 0,
        }
    }

    pub fn mem_write(&mut self, addr: u16, val: u8, mapper: &mut dyn Mapper) {
        match addr & 0x3FFF {
            0x0000..=0x1FFF => mapper.chr_write(addr & 0x1FFF, val),
            0x2000..=0x3EFF => {
                let nt_addr = addr - 0x2000;
                let coarse_x = (nt_addr % 32) as u8;
                let raw = if nt_addr >= 0x800 {
                    if nt_addr < 0xC00 {
                        2
                    } else {
                        3
                    }
                } else if nt_addr >= 0x400 {
                    1
                } else {
                    0
                };
                let phys = mapper.name_table(raw, coarse_x >= 16);
                let mirror = mapper.mirroring();
                let final_phys = if phys != raw {
                    phys
                } else {
                    match mirror {
                        0 => raw >> 1,
                        1 => raw & 1,
                        _ => raw,
                    }
                };
                self.vram[(final_phys as usize) * 0x400 + (nt_addr & 0x3FF) as usize] = val;
            }
            0x3F00..=0x3FFF => {
                let mut i = (addr & 0x1F) as usize;
                mapper.palette_access(addr & 0x3F00, false);
                if i >= 0x10 && (i & 3) == 0 {
                    i -= 0x10;
                }
                self.pal[i] = val & 0x3F;
                if i == 0 {
                    self.pal[0x10] = val & 0x3F;
                }
            }
            _ => {}
        }
    }

    // --------------------------------------------------------------- timing

    /// Increment fine Y / coarse Y the way the PPU does during rendering.
    fn increment_y(&mut self) {
        if self.v & 0x7000 != 0x7000 {
            self.v = self.v.wrapping_add(0x1000);
        } else {
            self.v &= !0x7000;
            let mut cy = (self.v >> 5) & 0x1F;
            if cy == 29 {
                cy = 0;
                self.v ^= 0x0800; // switch vertical nametable
            } else if cy == 31 {
                cy = 0;
            } else {
                cy += 1;
            }
            self.v = (self.v & !0x03E0) | (cy << 5);
        }
    }

    /// Copy the horizontal scroll bits of t back into v (done each scanline).
    fn copy_horizontal(&mut self) {
        self.v = (self.v & 0x7BE0) | (self.t & 0x041F);
    }

    /// Called by the console after each completed scanline (post-render).
    pub fn end_of_scanline(&mut self) {
        // The PPU only advances v during rendering; with rendering off the
        // game is free to stream VRAM writes across scanlines.
        let rendering = self.mask & (M_BG | M_SP) != 0;
        match self.line {
            0..=239 => {
                // one vertical unit per scanline, then restart horizontal scroll
                if rendering {
                    self.increment_y();
                    self.copy_horizontal();
                }
                if self.line == 239 {
                    self.status |= 0x80; // vblank
                    self.update_nmi_edge();
                }
            }
            260 => {
                // last vblank line; vblank flag falls
                self.status &= 0x7F;
                self.update_nmi_edge();
            }
            261 => {
                // pre-render: latch t into v. Real hardware only performs the
                // v<-t copy while rendering is enabled, which is what lets a
                // game keep a CPU-chosen v across frames during forced blank.
                if rendering {
                    self.v = self.t & 0x7FFF;
                }
            }
            _ => {}
        }
        self.line = if self.line == 261 {
            0
        } else {
            self.line + 1
        };
        if self.line == 0 {
            // new frame
            self.status &= !(0x20 | 0x40); // clear overflow + sprite0
            self.frame += 1;
            self.frame_complete = true;
        }
    }

    // -------------------------------------------------------------- render

    #[inline]
    fn pal_rgb(&self, idx: u8) -> u32 {
        NES_PALETTE[(idx & 0x3F) as usize]
    }

    fn apply_effects(&self, rgb: u32, idx: u8) -> u32 {
        let mut r = (rgb >> 16) & 0xFF;
        let mut g = (rgb >> 8) & 0xFF;
        let mut b = rgb & 0xFF;
        // grayscale
        if self.mask & M_GREY != 0 {
            let v = if idx == 0x0F || idx == 0x10 || idx == 0x2D || idx == 0x30 {
                0x30
            } else {
                0x04
            };
            let _ = v;
            let gray = (r * 30 + g * 59 + b * 11) / 100;
            r = gray;
            g = gray;
            b = gray;
        }
        // $2001 bits 5/6/7: color emphasis (attenuate complementary channels)
        let em = self.mask & 0xE0;
        if em != 0 {
            if em & 0x20 != 0 {
                g = g * 3 / 4;
                b = b * 3 / 4;
            }
            if em & 0x40 != 0 {
                r = r * 3 / 4;
                b = b * 3 / 4;
            }
            if em & 0x80 != 0 {
                r = r * 3 / 4;
                g = g * 3 / 4;
            }
        }
        (r << 16) | (g << 8) | b
    }

    /// Render scanline `y` (0..239) into fb row y.
    pub fn render_line(&mut self, y: u16, mapper: &mut dyn Mapper, fb: &mut [u32]) {
        let row = (y as usize) * FB_WIDTH;
        let show_bg = self.mask & M_BG != 0;
        let show_sp = self.mask & M_SP != 0;

        for i in 0..FB_WIDTH {
            self.bg_color[i] = 0;
            self.sp_present[i] = false;
            self.sp_color[i] = 0;
            self.sp_prio[i] = false;
        }

        // ---------------- background ----------------
        if show_bg {
            let coarse_x = (self.v & 0x1F) as u16;
            let coarse_y = ((self.v >> 5) & 0x1F) as u16;
            let fine_x = self.x as u16;
            let fine_y = ((self.v >> 12) & 7) as u16;
            let nt_base = ((self.v >> 10) & 3) as u8;
            // ctrl bit4 selects background pattern table
            let pat_base = ((self.ctrl >> 4) & 1) as u16 * 0x1000;

            let mut tile_col = 0xFFFFu16;
            let mut p0 = 0u8;
            let mut p1 = 0u8;
            let mut pal_idx = 0u8;

            // vertical coarse-y wraps at 30 and flips the nametable bit
            let (trow, nt_v) = if coarse_y >= 30 {
                (coarse_y - 30, (nt_base >> 1) ^ 1)
            } else {
                (coarse_y, nt_base >> 1)
            };

            for x in 0..256u16 {
                let sx = x + fine_x;
                let tc = coarse_x + sx / 8;
                let tcol = (tc % 32) as u8;
                let shift = 7 - (sx % 8);
                if tc != tile_col {
                    tile_col = tc;
                    // horizontal wrap past 32 tiles flips the nametable bit
                    let nt_h = (nt_base & 1) ^ ((tc / 32) & 1) as u8;
                    let phys = self.resolve_nt(nt_v * 2 + nt_h, tcol >= 16, mapper);
                    let nt_addr = 0x2000u16 + (phys as u16) * 0x400 + trow * 32 + tcol as u16;
                    let tile = self.vram_at(nt_addr, mapper);
                    let attr_addr =
                        0x23C0u16 + (phys as u16) * 0x400 + (trow / 4) * 8 + (tcol as u16 / 4);
                    let ab = self.vram_at(attr_addr, mapper);
                    let ashift = ((trow & 2) * 2) + (tcol as u16 & 2);
                    pal_idx = (ab >> ashift) & 3;
                    let fy = fine_y;
                    p0 = mapper.chr_read(pat_base + tile as u16 * 16 + fy);
                    p1 = mapper.chr_read(pat_base + tile as u16 * 16 + fy + 8);
                }
                let lo = (p0 >> shift) & 1;
                let hi = (p1 >> shift) & 1;
                let color = lo | (hi << 1);
                if color == 0 {
                    self.bg_color[x as usize] = 0;
                } else {
                    self.bg_color[x as usize] = pal_idx * 4 + color;
                }
                if x < 8 && self.mask & M_BG_LEFT == 0 {
                    self.bg_color[x as usize] = 0;
                }
            }
        }

        // ---------------- sprites ----------------
        let mut sprite0_hit = false;
        if show_sp {
            let sp_h = if self.ctrl & 0x20 != 0 { 16 } else { 8 };
            let sp_base = ((self.ctrl >> 3) & 1) as u16 * 0x1000;
            let mut count = 0u8;
            // sprites are rendered in OAM-index order; lower index = higher priority
            for i in 0..64usize {
                let sy = self.oam[i * 4] as i16;
                let row_rel = y as i16 - sy;
                if row_rel < 0 || row_rel >= sp_h as i16 {
                    continue;
                }
                if count == 8 {
                    self.status |= 0x20; // sprite overflow
                    break;
                }
                count += 1;
                let mut tile = self.oam[i * 4 + 1];
                let attr = self.oam[i * 4 + 2];
                let sxo = self.oam[i * 4 + 3] as i16;
                let flip_v = attr & 0x80 != 0;
                let flip_h = attr & 0x40 != 0;
                let mut row = if flip_v { sp_h as i16 - 1 - row_rel } else { row_rel } as u8;
                if sp_h == 16 {
                    if row >= 8 {
                        row -= 8;
                        tile |= 1;
                    } else {
                        tile &= 0xFE;
                    }
                }
                let addr = sp_base + (tile as u16) * 16 + row as u16;
                let mut b0 = mapper.chr_read(addr);
                let mut b1 = mapper.chr_read(addr + 8);
                if flip_h {
                    b0 = b0.reverse_bits();
                    b1 = b1.reverse_bits();
                }
                let pal = 0x10 + (attr & 3) * 4;
                let prio = attr & 0x20 != 0;
                for px in 0..8i16 {
                    let x = sxo + px;
                    if x < 0 || x >= 256 {
                        continue;
                    }
                    if x < 8 && self.mask & M_SP_LEFT == 0 {
                        continue;
                    }
                    let bit = 7 - px as u8;
                    let lo = (b0 >> bit) & 1;
                    let hi = (b1 >> bit) & 1;
                    let c = lo | (hi << 1);
                    if c == 0 {
                        continue;
                    }
                    let xi = x as usize;
                    if i == 0 && x > 0 && self.bg_color[xi] != 0 {
                        sprite0_hit = true;
                    }
                    if !self.sp_present[xi] {
                        self.sp_present[xi] = true;
                        self.sp_color[xi] = pal + c;
                        self.sp_prio[xi] = prio;
                    }
                }
            }
        }

        // sprite 0 hit: sprite 0 overlaps a non-transparent background pixel (x != 0)
        if show_bg && show_sp && sprite0_hit {
            self.status |= 0x40;
        }

        // ---------------- compose ----------------
        let bg0 = self.pal[0] & 0x3F;
        for x in 0..FB_WIDTH {
            // bg_color/sp_color hold palette RAM indices (not NES colour numbers)
            let idx = {
                let bgc = self.bg_color[x];
                if self.sp_present[x] && (bgc == 0 || !self.sp_prio[x]) {
                    self.pal[(self.sp_color[x] & 0x3F) as usize] & 0x3F
                } else if bgc != 0 {
                    self.pal[(bgc & 0x3F) as usize] & 0x3F
                } else {
                    bg0
                }
            };
            let rgb = self.pal_rgb(idx);
            fb[row + x] = self.apply_effects(rgb, idx);
        }
    }

    #[inline]
    fn resolve_nt(&self, raw: u8, x_right: bool, mapper: &mut dyn Mapper) -> u8 {
        let phys = mapper.name_table(raw, x_right);
        if phys != raw {
            return phys;
        }
        match mapper.mirroring() {
            0 => raw >> 1,
            1 => raw & 1,
            _ => raw,
        }
    }

    #[inline]
    fn vram_at(&self, addr: u16, _mapper: &mut dyn Mapper) -> u8 {
        self.vram[(addr - 0x2000) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapper::Nrom;

    #[test]
    fn vblank_sets_status_and_nmi() {
        let mut ppu = Ppu::new();
        let mut mapper = Nrom::default_nrom();
        ppu.ctrl = PPU_NMI;
        // run through 240 visible lines
        for _ in 0..240 {
            ppu.end_of_scanline();
        }
        assert!(ppu.vblank());
        assert!(ppu.take_nmi());
        let mut sink = vec![0u32; FB_WIDTH * FB_HEIGHT];
        for _ in 0..22 {
            if ppu.line < 240 {
                ppu.render_line(ppu.line, &mut mapper, &mut sink);
            }
            ppu.end_of_scanline();
        }
        assert_eq!(ppu.line, 0);
        assert!(!ppu.vblank());
        assert!(ppu.frame_complete);
    }
}
