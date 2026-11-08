//! iNES / NES 2.0 cartridge image parsing.

const HEADER_LEN: usize = 16;
const TRAINER_LEN: usize = 512;
const PRG_BANK_SIZE: usize = 16 * 1024;
const CHR_BANK_SIZE: usize = 8 * 1024;
const RAM_SHIFT_UNIT: usize = 64;

#[derive(Debug, Clone)]
pub struct RomInfo {
    pub mapper: u16,
    pub submapper: u8,
    pub prg_banks_16k: usize,
    pub chr_banks_8k: usize,
    pub mirroring_vertical: bool,
    pub battery: bool,
    pub trainer: bool,
    pub four_screen: bool,
    /// Total volatile + non-volatile PRG RAM reported by the header.
    pub prg_ram_size: usize,
    pub is_nes20: bool,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct Rom {
    pub info: RomInfo,
    pub prg: Vec<u8>,
    pub chr: Vec<u8>,
    /// The optional 512-byte trainer, loaded into $7000-$71FF by Console.
    pub trainer_data: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RomError {
    TooSmall,
    BadMagic,
    Empty,
    InvalidPrgSize,
    TruncatedTrainer,
    TruncatedPrg { expected: usize, available: usize },
    TruncatedChr { expected: usize, available: usize },
    SizeOverflow,
}

impl std::fmt::Display for RomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RomError::TooSmall => write!(f, "文件太小，不是有效的 NES 镜像"),
            RomError::BadMagic => write!(f, "缺少 NES 头（'NES\\x1A' 魔数不匹配）"),
            RomError::Empty => write!(f, "镜像不含 PRG 数据"),
            RomError::InvalidPrgSize => write!(f, "ROM 头声明的 PRG 容量无效"),
            RomError::TruncatedTrainer => write!(f, "ROM 声明包含 512 字节 Trainer，但数据不完整"),
            RomError::TruncatedPrg { expected, available } => {
                write!(f, "PRG 数据不完整：需要 {} 字节，实际只有 {} 字节", expected, available)
            }
            RomError::TruncatedChr { expected, available } => {
                write!(f, "CHR 数据不完整：需要 {} 字节，实际只有 {} 字节", expected, available)
            }
            RomError::SizeOverflow => write!(f, "ROM 头声明的容量超出当前平台可处理范围"),
        }
    }
}

impl std::error::Error for RomError {}

fn checked_add(a: usize, b: usize) -> Result<usize, RomError> {
    a.checked_add(b).ok_or(RomError::SizeOverflow)
}

fn checked_mul(a: usize, b: usize) -> Result<usize, RomError> {
    a.checked_mul(b).ok_or(RomError::SizeOverflow)
}

/// NES 2.0 ROM size. A high nibble of 0xF selects exponent/multiplier mode.
fn nes20_rom_size(lsb: u8, ms_nibble: u8, bank_size: usize) -> Result<usize, RomError> {
    if ms_nibble == 0x0F {
        let exponent = (lsb >> 2) as u32;
        let multiplier = ((lsb & 0x03) as usize) * 2 + 1;
        let base = 1usize.checked_shl(exponent).ok_or(RomError::SizeOverflow)?;
        checked_mul(base, multiplier)
    } else {
        let banks = (lsb as usize) | ((ms_nibble as usize) << 8);
        checked_mul(banks, bank_size)
    }
}

/// NES 2.0 RAM size encoding: zero means absent; otherwise 64 << shift bytes.
fn nes20_ram_size(shift: u8) -> Result<usize, RomError> {
    if shift == 0 {
        return Ok(0);
    }
    RAM_SHIFT_UNIT
        .checked_shl(shift as u32)
        .ok_or(RomError::SizeOverflow)
}

impl Rom {
    pub fn parse(data: &[u8]) -> Result<Rom, RomError> {
        if data.len() < HEADER_LEN {
            return Err(RomError::TooSmall);
        }

        // Preserve the project's legacy support for headerless PRG-only dumps.
        if &data[..4] != b"NES\x1a" {
            if data.len() % PRG_BANK_SIZE == 0 && !data.is_empty() {
                let prg = data.to_vec();
                return Ok(Rom {
                    info: RomInfo {
                        mapper: 0,
                        submapper: 0,
                        prg_banks_16k: prg.len() / PRG_BANK_SIZE,
                        chr_banks_8k: 0,
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
                    trainer_data: None,
                });
            }
            return Err(RomError::BadMagic);
        }

        let flags6 = data[6];
        let flags7 = data[7];
        let is_nes20 = (flags7 & 0x0C) == 0x08;

        let trainer = flags6 & 0x04 != 0;
        let four_screen = flags6 & 0x08 != 0;
        let battery = flags6 & 0x02 != 0;
        let mirroring_vertical = flags6 & 0x01 != 0;

        let mut mapper = ((flags6 >> 4) | (flags7 & 0xF0)) as u16;
        let mut submapper = 0;
        let (prg_len, chr_len, prg_ram_size, title) = if is_nes20 {
            if data.len() < 24 {
                return Err(RomError::TooSmall);
            }

            let flags8 = data[8];
            let flags9 = data[9];
            mapper |= ((flags8 & 0x0F) as u16) << 8;
            submapper = flags8 >> 4;

            let prg_len = nes20_rom_size(data[4], flags9 & 0x0F, PRG_BANK_SIZE)?;
            let chr_len = nes20_rom_size(data[5], flags9 >> 4, CHR_BANK_SIZE)?;

            // Byte 10: volatile / non-volatile PRG RAM. Byte 11: CHR RAM.
            let prg_ram = checked_add(nes20_ram_size(data[10] & 0x0F)?, nes20_ram_size(data[10] >> 4)?)?;
            let chr_ram = checked_add(nes20_ram_size(data[11] & 0x0F)?, nes20_ram_size(data[11] >> 4)?)?;
            let ram_size = if prg_ram == 0 && battery { 8 * 1024 } else { prg_ram };
            let _ = chr_ram; // CHR RAM is allocated by the PPU/Mapper when CHR-ROM is absent.

            // The 16-byte iNES/NES 2.0 header has no standard title field.
            (prg_len, chr_len, ram_size, String::new())
        } else {
            let prg_banks = data[4] as usize;
            let chr_banks = data[5] as usize;
            if prg_banks == 0 {
                return Err(RomError::InvalidPrgSize);
            }
            let prg_len = checked_mul(prg_banks, PRG_BANK_SIZE)?;
            let chr_len = checked_mul(chr_banks, CHR_BANK_SIZE)?;
            // iNES 1.0 byte 8 is in 8 KiB units; zero conventionally means 8 KiB.
            let ram_banks = if data[8] == 0 { 1 } else { data[8] as usize };
            let ram_size = checked_mul(ram_banks, 8 * 1024)?;
            (prg_len, chr_len, ram_size, String::new())
        };

        if prg_len == 0 {
            return Err(RomError::Empty);
        }

        let mut offset = HEADER_LEN;
        let trainer_data = if trainer {
            let end = checked_add(offset, TRAINER_LEN)?;
            if end > data.len() {
                return Err(RomError::TruncatedTrainer);
            }
            let bytes = data[offset..end].to_vec();
            offset = end;
            Some(bytes)
        } else {
            None
        };

        let prg_end = checked_add(offset, prg_len)?;
        if prg_end > data.len() {
            return Err(RomError::TruncatedPrg {
                expected: prg_len,
                available: data.len().saturating_sub(offset),
            });
        }
        let prg = data[offset..prg_end].to_vec();
        offset = prg_end;

        let chr_end = checked_add(offset, chr_len)?;
        if chr_end > data.len() {
            return Err(RomError::TruncatedChr {
                expected: chr_len,
                available: data.len().saturating_sub(offset),
            });
        }
        let chr = data[offset..chr_end].to_vec();

        Ok(Rom {
            info: RomInfo {
                mapper,
                submapper,
                prg_banks_16k: prg_len.saturating_add(PRG_BANK_SIZE - 1) / PRG_BANK_SIZE,
                chr_banks_8k: chr_len.saturating_add(CHR_BANK_SIZE - 1) / CHR_BANK_SIZE,
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
            trainer_data,
        })
    }

    pub fn load(path: &str) -> Result<Rom, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("读取文件失败: {}", e))?;
        Rom::parse(&bytes).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ines(prg_banks: u8, chr_banks: u8, flags6: u8, flags7: u8, payload: Vec<u8>) -> Vec<u8> {
        let mut data = vec![0; HEADER_LEN];
        data[..4].copy_from_slice(b"NES\x1a");
        data[4] = prg_banks;
        data[5] = chr_banks;
        data[6] = flags6;
        data[7] = flags7;
        data.extend(payload);
        data
    }

    #[test]
    fn ines_trainer_and_four_screen_are_distinct_flags() {
        let mut payload = vec![0xA5; TRAINER_LEN];
        payload.extend(vec![0x11; PRG_BANK_SIZE]);
        let rom = Rom::parse(&ines(1, 0, 0x0C, 0, payload)).unwrap();
        assert!(rom.info.trainer);
        assert!(rom.info.four_screen);
        assert_eq!(rom.trainer_data.as_ref().unwrap().len(), TRAINER_LEN);
        assert_eq!(rom.trainer_data.as_ref().unwrap()[0], 0xA5);
        assert_eq!(rom.prg[0], 0x11);
    }

    #[test]
    fn ines_prg_ram_size_uses_byte_eight() {
        let payload = vec![0; PRG_BANK_SIZE];
        let rom = Rom::parse(&ines(1, 0, 0, 0, payload.clone())).unwrap();
        assert_eq!(rom.info.prg_ram_size, 8 * 1024);

        let mut data = ines(1, 0, 0, 0, payload);
        data[8] = 2;
        let rom = Rom::parse(&data).unwrap();
        assert_eq!(rom.info.prg_ram_size, 16 * 1024);
    }

    #[test]
    fn nes20_uses_full_linear_prg_and_chr_bank_fields() {
        let mut data = vec![0; 24];
        data[..4].copy_from_slice(b"NES\x1a");
        data[4] = 2;
        data[5] = 1;
        data[7] = 0x08;
        data[9] = 0x11; // PRG 258 x 16 KiB, CHR 257 x 8 KiB
        let prg_len = 258 * PRG_BANK_SIZE;
        let chr_len = 257 * CHR_BANK_SIZE;
        data.resize(24 + prg_len + chr_len, 0);
        let rom = Rom::parse(&data).unwrap();
        assert_eq!(rom.prg.len(), prg_len);
        assert_eq!(rom.chr.len(), chr_len);
    }

    #[test]
    fn nes20_exponent_size_encoding_is_supported() {
        let mut data = vec![0; 24];
        data[..4].copy_from_slice(b"NES\x1a");
        data[4] = (14 << 2) | 1; // 2^14 * 3 = 48 KiB
        data[5] = 0;
        data[7] = 0x08;
        data[9] = 0x0F;
        data.resize(24 + 48 * 1024, 0);
        let rom = Rom::parse(&data).unwrap();
        assert_eq!(rom.prg.len(), 48 * 1024);
        assert!(rom.chr.is_empty());
    }

    #[test]
    fn nes20_byte_twelve_does_not_override_mirroring() {
        let mut data = vec![0; 24 + PRG_BANK_SIZE];
        data[..4].copy_from_slice(b"NES\x1a");
        data[4] = 1;
        data[6] = 1; // vertical mirroring
        data[7] = 0x08;
        data[12] = 0; // NTSC timing
        let rom = Rom::parse(&data).unwrap();
        assert!(rom.info.mirroring_vertical);
    }

    #[test]
    fn rejects_truncated_trainer_prg_and_chr() {
        let trainer = ines(1, 0, 0x04, 0, vec![0; 100]);
        assert_eq!(Rom::parse(&trainer).unwrap_err(), RomError::TruncatedTrainer);

        let prg = ines(1, 0, 0, 0, vec![0; 100]);
        assert!(matches!(Rom::parse(&prg), Err(RomError::TruncatedPrg { .. })));

        let chr = ines(1, 1, 0, 0, vec![0; PRG_BANK_SIZE]);
        assert!(matches!(Rom::parse(&chr), Err(RomError::TruncatedChr { .. })));
    }

    #[test]
    fn rejects_zero_prg_banks_in_ines() {
        let data = ines(0, 0, 0, 0, vec![]);
        assert_eq!(Rom::parse(&data).unwrap_err(), RomError::InvalidPrgSize);
    }

    #[test]
    fn headerless_prg_dump_remains_supported() {
        let data = vec![0xEA; PRG_BANK_SIZE];
        let rom = Rom::parse(&data).unwrap();
        assert_eq!(rom.info.mapper, 0);
        assert_eq!(rom.prg.len(), PRG_BANK_SIZE);
        assert!(rom.trainer_data.is_none());
    }
}
