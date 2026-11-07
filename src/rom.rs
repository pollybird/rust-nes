/// iNES / NES 2.0 ROM header parsing.
///
/// Supports the standard 16-byte iNES header, NES 2.0 extensions, and
/// bare 512-byte "FDS/raw" style images (wrapped into a synthetic NROM).
#[derive(Debug, Clone)]
pub struct RomInfo {
    pub mapper: u16,
    pub submapper: u8,
    pub prg_banks_16k: usize,
    pub chr_banks_8k: usize,
    pub mirroring_vertical: bool, // true => vertical arrangement, false => horizontal
    pub battery: bool,
    pub trainer: bool,
    pub four_screen: bool,
    pub prg_ram_size: usize,      // bytes, 0 => default 8KB if battery or mapper wants it
    pub is_nes20: bool,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct Rom {
    pub info: RomInfo,
    pub prg: Vec<u8>,
    pub chr: Vec<u8>,
}

#[derive(Debug)]
pub enum RomError {
    TooSmall,
    BadMagic,
    Empty,
}

impl std::fmt::Display for RomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RomError::TooSmall => write!(f, "文件太小，不是有效的 NES 镜像"),
            RomError::BadMagic => write!(f, "缺少 NES 头（'NES\\x1A' 魔数不匹配）"),
            RomError::Empty => write!(f, "镜像不含 PRG 数据"),
        }
    }
}

impl Rom {
    pub fn parse(data: &[u8]) -> Result<Rom, RomError> {
        // Bare dump: exact multiple of 16K/32K without header => treat as NROM 2K*?
        if data.len() < 16 {
            return Err(RomError::TooSmall);
        }
        if &data[0..4] != b"NES\x1a" {
            // Try to interpret as headerless PRG-only image (rare .nes mislabels)
            if data.len() % 16384 == 0 || data.len() % 32768 == 0 {
                let prg = data.to_vec();
                return Ok(Rom {
                    info: RomInfo {
                        mapper: 0,
                        submapper: 0,
                        prg_banks_16k: prg.len() / 16384,
                        chr_banks_8k: 0, // will get 8KB of CHR-RAM
                        mirroring_vertical: false,
                        battery: false,
                        trainer: false,
                        four_screen: false,
                        prg_ram_size: 0,
                        is_nes20: false,
                        title: String::new(),
                    },
                    prg,
                    chr: Vec::new(),
                });
            }
            return Err(RomError::BadMagic);
        }

        let prg_banks = data[4] as usize;
        let chr_banks = data[5] as usize;
        let flags6 = data[6];
        let flags7 = data[7];

        let mut mapper = ((flags6 >> 4) | (flags7 & 0xf0)) as u16;
        let mut mirroring_vertical = flags6 & 1 != 0;
        let battery = flags6 & 2 != 0;
        let trainer = flags6 & 8 != 0;
        let four_screen = flags6 & 8 != 0; // bit3: vs/unmirrored (4-screen naming)
        let mut prg_ram_size = if flags6 & 0x40 != 0 { 8192 } else { 0 }; // PRG RAM present flag
        let is_nes20 = (flags7 & 0x0c) == 0x08;

        let mut submapper = 0u8;
        let mut prg_banks_16k = prg_banks;
        let mut chr_banks_8k = chr_banks;

        if prg_banks_16k == 0 {
            prg_banks_16k = 2; // some NES2.0 encodings put size elsewhere; default small
        }

        if is_nes20 {
            if data.len() < 24 {
                return Err(RomError::TooSmall);
            }
            let flags8 = data[8];
            let flags9 = data[9];
            mapper |= ((flags8 as u16) & 0x0f) << 8;
            submapper = flags8 >> 4;
            // NES2.0 PRG size: low nibble in byte 4, top 2 bits in byte 9 bits 0-1
            if flags9 & 0x03 != 0 {
                prg_banks_16k = prg_banks | (((flags9 & 0x03) as usize) << 8);
            }
            // NES2.0 CHR size encoding
            if chr_banks == 0 {
                let nib = (flags9 >> 4) & 0x07;
                if nib & 1 != 0 {
                    let shift = (nib >> 1) & 3;
                    chr_banks_8k = 2 << shift; // 16K,32K,64K granules -> in 8K units 2,4,8
                } else if nib != 0 {
                    chr_banks_8k = 1 << nib; // exponent encoding
                }
            }
            // PRG RAM size encoding (bytes 10 exponent, 11 mantissa)
            let exp = data[10] >> 4;
            let mant = data[10] & 0x0f;
            if exp != 0 || mant != 0 {
                let units = 1u32 << exp.saturating_sub(6); // bytes granularity
                prg_ram_size = (units.max(1) * (mant as u32 + 1)) as usize;
                prg_ram_size = prg_ram_size / 8 * 8; // normalize; keep simple
                if prg_ram_size == 0 {
                    prg_ram_size = 8192;
                }
            }
            mirroring_vertical = match data[12] & 0x03 {
                0 => false,
                1 => true,
                2 => false, // single-screen lower
                _ => true,  // single-screen upper
            };
        }

        let mut offset = 16usize;
        if trainer {
            // 512-byte trainer at offset 16
        }

        let prg_len = prg_banks_16k * 16384;
        if offset + 16 > data.len() && prg_len == 0 {
            return Err(RomError::Empty);
        }
        if trainer {
            offset += 512;
        }
        let end = (offset + prg_len).min(data.len());
        let prg = data[offset..end].to_vec();
        if prg.is_empty() {
            return Err(RomError::Empty);
        }
        offset = end;
        let chr_len = chr_banks_8k * 8192;
        let end2 = (offset + chr_len).min(data.len());
        let chr = if end2 > offset { data[offset..end2].to_vec() } else { Vec::new() };

        let title = if is_nes20 && data.len() >= 4 + 12 + 12 {
            let t = &data[12..24];
            let s: String = t
                .iter()
                .take_while(|b| **b != 0)
                .map(|b| if *b >= 0x20 && *b < 0x7f { *b as char } else { ' ' })
                .collect();
            s.trim().to_string()
        } else {
            String::new()
        };

        Ok(Rom {
            info: RomInfo {
                mapper,
                submapper,
                prg_banks_16k: prg.len() / 16384,
                chr_banks_8k: chr.len() / 8192,
                mirroring_vertical,
                battery,
                trainer,
                four_screen,
                prg_ram_size,
                is_nes20,
                title,
            },
            prg,
            chr,
        })
    }

    pub fn load(path: &str) -> Result<Rom, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("读取文件失败: {}", e))?;
        Rom::parse(&bytes).map_err(|e| e.to_string())
    }
}
