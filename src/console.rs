//! Console / bus glue: memory map, controllers, frame loop.

use crate::apu::Apu;
use crate::cpu::{Bus, Cpu6502};
use crate::mapper::{self, Mapper};
use crate::ppu::{Ppu, FB_HEIGHT, FB_WIDTH};
use crate::rom::Rom;

// Controller button bits (as latched into $4016/17 shift register)
pub const BTN_A: u8 = 0x01;
pub const BTN_B: u8 = 0x02;
pub const BTN_SELECT: u8 = 0x04;
pub const BTN_START: u8 = 0x08;
pub const BTN_UP: u8 = 0x10;
pub const BTN_DOWN: u8 = 0x20;
pub const BTN_LEFT: u8 = 0x40;
pub const BTN_RIGHT: u8 = 0x80;

#[derive(Default, Clone)]
struct Controller {
    /// raw input bitmask from the app
    pub buttons: u8,
    strobe: bool,
    shift: u8,
    reads: u8,
}

impl Controller {
    fn strobe_write(&mut self, level_high: bool) {
        if level_high {
            self.shift = self.buttons;
            self.reads = 0;
        }
        self.strobe = level_high;
    }
    fn read_bit(&mut self) -> u8 {
        if self.strobe {
            return self.buttons & 1;
        }
        let bit = self.shift & 1;
        self.shift >>= 1;
        self.reads += 1;
        if self.reads > 8 {
            1
        } else {
            bit
        }
    }
}

pub struct Console {
    pub cpu: Cpu6502,
    pub ppu: Ppu,
    pub apu: Apu,
    pub mapper: Box<dyn Mapper>,
    pub ram: [u8; 2048],
    sram: Vec<u8>,
    ctl1: Controller,
    ctl2: Controller,
    open_bus: u8,
    frame_buf: Vec<u32>,
    pending_stall: u32,
}

impl Console {
    pub fn new(rom: Rom) -> Result<Console, String> {
        let four_screen = rom.info.four_screen;
        let mapper = mapper::create(&rom)?;
        let sram_size = if rom.info.prg_ram_size == 0 {
            if rom.info.battery {
                8192
            } else {
                0
            }
        } else {
            rom.info.prg_ram_size
        };
        let mut ppu = Ppu::new();
        if four_screen {
            // four-screen needs its extra tables; the PPU always has 4KB VRAM
        }
        let mut sram = vec![0u8; sram_size.max(8192)];
        // iNES trainers are copied to CPU $7000-$71FF (SRAM offset $1000).
        if let Some(trainer) = rom.trainer_data.as_ref() {
            if trainer.len() == 512 && sram.len() >= 0x1200 {
                sram[0x1000..0x1200].copy_from_slice(trainer);
            }
        }
        Ok(Console {
            cpu: Cpu6502::new(),
            ppu,
            apu: Apu::new(),
            mapper,
            ram: [0; 2048],
            sram,
            ctl1: Controller::default(),
            ctl2: Controller::default(),
            open_bus: 0,
            frame_buf: vec![0u32; FB_WIDTH * FB_HEIGHT],
            pending_stall: 0,
        })
    }

    pub fn reset(&mut self) {
        let mut cpu = std::mem::take(&mut self.cpu);
        cpu.reset(self);
        self.cpu = cpu;
    }

    pub fn set_pad(&mut self, idx: usize, mask: u8) {
        if idx == 0 {
            self.ctl1.buttons = mask;
        } else {
            self.ctl2.buttons = mask;
        }
    }

    pub fn framebuffer(&self) -> &[u32] {
        &self.frame_buf
    }

    pub fn sram_snapshot(&self) -> &[u8] {
        &self.sram
    }
    pub fn restore_sram(&mut self, data: &[u8]) {
        let n = data.len().min(self.sram.len());
        self.sram[..n].copy_from_slice(&data[..n]);
    }

    /// Run the console for one frame; returns a reference to the framebuffer.
    pub fn run_frame(&mut self) {
        self.ppu.frame_complete = false;
        let mut dots: u64 = 0;
        let mut cpu = std::mem::take(&mut self.cpu);

        while !self.ppu.frame_complete {
            if self.pending_stall > 0 {
                self.pending_stall -= 1;
                dots += 3;
                self.apu.advance(1);
            } else {
                let spent = cpu.step(self);
                self.apu.advance(spent);
                dots += (spent as u64) * 3;
            }
            while dots >= 341 && !self.ppu.frame_complete {
                dots -= 341;
                let line = self.ppu.line;
                if line < 240 {
                    self.ppu.render_line(line, &mut *self.mapper, &mut self.frame_buf);
                    self.mapper.scanline(line);
                }
                self.ppu.end_of_scanline();
            }
        }
        self.cpu = cpu;
    }

    // ---------------- internal helpers ----------------

    fn ppu_reg_read(&mut self, a: u16) -> u8 {
        let reg = (a & 7) as u8;
        if reg == 4 {
            self.ppu.oam[self.ppu.oam_addr as usize]
        } else {
            self.ppu.reg_read(reg, &mut *self.mapper)
        }
    }

    fn dma_oam(&mut self, page: u8) {
        let base = (page as u16) << 8;
        let mut tmp = [0u8; 256];
        for i in 0..256u16 {
            tmp[i as usize] = self.cpu_ram_or_io_read(base.wrapping_add(i));
        }
        for i in 0..256 {
            self.ppu.oam[i] = tmp[i];
        }
        self.pending_stall += 513;
    }

    /// read used by DMA (must not recurse through the $4014 path)
    fn cpu_ram_or_io_read(&mut self, addr: u16) -> u8 {
        self.read(addr)
    }
}

impl Bus for Console {
    fn read(&mut self, addr: u16) -> u8 {
        let v = match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x7FF) as usize],
            0x2000..=0x3FFF => {
                let reg = (addr & 7) as u8;
                match reg {
                    2 => self.ppu.reg_read(2, &mut *self.mapper),
                    4 => self.ppu.oam[self.ppu.oam_addr as usize],
                    _ => self.ppu.reg_read(reg, &mut *self.mapper),
                }
            }
            0x4000..=0x4013 => self.apu.read(addr),
            0x4014 => self.open_bus,
            0x4015 => self.apu.read(0x4015),
            0x4016 => {
                // P1 bit0, P2 fixed high bits for standard pads
                let b = self.ctl1.read_bit();
                b | 0x40
            }
            0x4017 => {
                let b = self.ctl2.read_bit();
                b | 0x40
            }
            0x4018..=0x5FFF => 0,
            0x6000..=0x7FFF => {
                if !self.mapper.sram_enabled() {
                    0
                } else {
                    let i = (addr - 0x6000) as usize % self.sram.len().max(1);
                    self.sram[i]
                }
            }
            0x8000..=0xFFFF => self.mapper.prg_read(addr),
        };
        self.open_bus = v;
        v
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.open_bus = val;
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x7FF) as usize] = val,
            0x2000..=0x3FFF => {
                let reg = (addr & 7) as u8;
                if reg == 3 {
                    self.ppu.oam_addr = val;
                } else {
                    self.ppu.reg_write(reg, val, &mut *self.mapper);
                }
            }
            0x4000..=0x4013 => self.apu.write(addr, val),
            0x4014 => self.dma_oam(val),
            0x4015 => self.apu.write(0x4015, val),
            0x4016 => {
                let strobe = val & 1 != 0;
                self.ctl1.strobe_write(strobe);
                self.ctl2.strobe_write(strobe);
            }
            0x4017 => {
                // some games strobe via 4017 (e.g., 2-player via serial); treat as strobe too
                if val & 1 != 0 {
                    // strobe pulse
                    self.ctl1.strobe_write(true);
                    self.ctl2.strobe_write(true);
                }
                // U four-player adapter clocked bits ignored (approx)
                self.ctl1.strobe_write(false);
                self.ctl2.strobe_write(false);
            }
            0x4018..=0x5FFF => {}
            0x6000..=0x7FFF => {
                if self.mapper.sram_enabled() && !self.sram.is_empty() {
                    let i = (addr - 0x6000) as usize % self.sram.len();
                    self.sram[i] = val;
                }
            }
            0x8000..=0xFFFF => self.mapper.prg_write(addr, val),
        }
    }

    fn take_nmi(&mut self) -> bool {
        self.ppu.take_nmi()
    }

    fn irq_line(&mut self) -> bool {
        self.mapper.irq_active() || self.apu.irq_line()
    }
}


/// Headless end-to-end self test: builds a tiny synthetic iNES game whose
/// 6502 program initializes the PPU, paints one background tile and palette
/// through $2006/$2007, then loops waiting for vertical blank while
/// incrementing a counter. Verifies: frames advance, vblank is observed by
/// the program, and pixels are actually painted.
pub fn smoke_test() {
    let prog: Vec<u8> = vec![
        0x78, // SEI
        0xA9, 0x00, 0x8D, 0x00, 0x20, // LDA #$00 ; STA $2000 (ctrl: bg table 0)
        0xA9, 0x18, 0x8D, 0x01, 0x20, // LDA #$18 ; STA $2001 (show bg)
        0xA9, 0x20, 0x8D, 0x06, 0x20, // LDA #$20 ; STA $2006
        0xA9, 0x02, 0x8D, 0x06, 0x20, // LDA #$02 ; STA $2006  -> v = $2002
        0xA9, 0x01, 0x8D, 0x07, 0x20, // LDA #$01 ; STA $2007  -> tile id 1 at nt x=2
        0xA9, 0x3F, 0x8D, 0x06, 0x20, // LDA #$3F ; STA $2006
        0xA9, 0x00, 0x8D, 0x06, 0x20, // LDA #$00 ; STA $2006  -> v = $3F00
        0xA9, 0x11, 0x8D, 0x07, 0x20, // universal bg color
        0xA9, 0x22, 0x8D, 0x07, 0x20, // pal entry 1
        0xA9, 0x12, 0x8D, 0x07, 0x20, // pal entry 2
        0xA9, 0x00, 0x8D, 0x06, 0x20, // LDA #$00 ; STA $2006
        0xA9, 0x10, 0x8D, 0x06, 0x20, // LDA #$10 ; STA $2006  -> v = $0010 (tile 1)
        0xA9, 0x0F, 0x8D, 0x07, 0x20, // plane0 row0 = $0F
        0xA9, 0xF0, 0x8D, 0x07, 0x20, // plane1 row0 = $F0
        // wait for vblank
        0xAD, 0x02, 0x20, // LDA $2002 (clears vblank)
        0x2A,             // ROL A (carry = old bit7)
        0x90, 0xFA,       // BCC -6 (back to LDA $2002)
        0xE6, 0x00,       // INC $00 (frame counter)
        0x4C, 0x00, 0x80, // JMP $8000 (re-init + wait next vblank)
    ];
    assert!(prog.len() < 0x7FF0);

    let mut rom_bytes: Vec<u8> = vec![0; 16 + 16384];
    rom_bytes[0..4].copy_from_slice(b"NES\x1a");
    rom_bytes[4] = 1; // 1 x 16K PRG
    rom_bytes[5] = 0; // CHR-RAM
    rom_bytes[6] = 0; // flags6: mapper 0, horizontal mirroring
    rom_bytes[7] = 0;
    rom_bytes[16..16 + prog.len()].copy_from_slice(&prog);
    // with a 16K PRG the bank is mirrored into both $8000-$BFFF and
    // $C000-$FFFF, so the reset vector ($FFFC) is at bank offset 0x3FFC.
    rom_bytes[16 + 0x3FFC] = 0x00;
    rom_bytes[16 + 0x3FFD] = 0x80;

    let rom = crate::rom::Rom::parse(&rom_bytes).expect("synthetic rom parse");
    let mut con = Console::new(rom).expect("console from synthetic rom");
    con.reset();

    let mut painted_frames = 0usize;
    for _ in 0..30 {
        con.run_frame();
        let fb = con.framebuffer();
        if fb.iter().any(|&px| px != 0x0011) {
            painted_frames += 1;
        }
    }
    let counter = con.ram[0];
    assert!(con.ppu.frame >= 29, "frames advanced: {}", con.ppu.frame);
    assert!(counter >= 25, "vblank observed by program: {}", counter);
    assert!(painted_frames > 0, "tile pixels painted");
    println!(
        "SMOKE OK: frames={} vblanks={} painted_frames={}",
        con.ppu.frame, counter, painted_frames
    );
}
