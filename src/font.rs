//! Minimal 8x8 bitmap font for the in-app settings overlay and error screens.

const GLYPH_W: usize = 7;

const fn rows(s: &[u8; GLYPH_W]) -> [u8; 8] {
    let mut out = [0u8; 8];
    let mut i = 0;
    while i < GLYPH_W {
        out[i] = s[i];
        i += 1;
    }
    out
}

/// rows are bitmasks, bit6 = leftmost pixel
const fn r(b: &str) -> u8 {
    let bb = b.as_bytes();
    let mut v = 0u8;
    let mut i = 0;
    while i < 7 {
        if bb[i] == b'#' {
            v |= 1 << (6 - i);
        }
        i += 1;
    }
    v
}

pub struct Glyph(pub [u8; 8]);

const fn g(a: &str, b: &str, c: &str, d: &str, e: &str, f: &str, h: &str) -> Glyph {
    Glyph([r(a), r(b), r(c), r(d), r(e), r(f), r(h), 0])
}

pub const fn glyph(c: char) -> Glyph {
    let cu = c.to_ascii_uppercase() as u32;
    // uppercase A-Z
    if cu >= 'A' as u32 && cu <= 'Z' as u32 {
        return match cu as u8 {
            b'A' => g(".####..", "#....#.", "#....#.", "#######", "#....#.", "#....#.", "#....#."),
            b'B' => g("#####..", "#....#.", "#....#.", "#####..", "#....#.", "#....#.", "#####.."),
            b'C' => g(".####..", "#....#.", "#......", "#......", "#......", "#....#.", ".####.."),
            b'D' => g("#####..", "#....#.", "#....#.", "#....#.", "#....#.", "#....#.", "#####.."),
            b'E' => g("#######", "#......", "#......", "#####..", "#......", "#......", "#######"),
            b'F' => g("#######", "#......", "#......", "#####..", "#......", "#......", "#......"),
            b'G' => g(".####..", "#....#.", "#......", "#.###..", "#....#.", "#....#.", ".####.."),
            b'H' => g("#....#.", "#....#.", "#....#.", "#######", "#....#.", "#....#.", "#....#."),
            b'I' => g("#####..", "...#...", "...#...", "...#...", "...#...", "...#...", "#####.."),
            b'J' => g("..#####", "....#..", "....#..", "....#..", "#...#..", "#...#..", ".###..."),
            b'K' => g("#....#.", "#...#..", "#..#...", "###....", "#..#...", "#...#..", "#....#."),
            b'L' => g("#......", "#......", "#......", "#......", "#......", "#......", "#######"),
            b'M' => g("#.....#", "##...##", "#.#.#.#", "#..#..#", "#.....#", "#.....#", "#.....#"),
            b'N' => g("#.....#", "##....#", "#.#...#", "#..#..#", "#...#.#", "#....##", "#.....#"),
            b'O' => g(".####..", "#....#.", "#....#.", "#....#.", "#....#.", "#....#.", ".####.."),
            b'P' => g("#####..", "#....#.", "#....#.", "#####..", "#......", "#......", "#......"),
            b'Q' => g(".####..", "#....#.", "#....#.", "#....#.", "#.#..#.", "#..###.", ".####.#"),
            b'R' => g("#####..", "#....#.", "#....#.", "#####..", "#..#...", "#...#..", "#....#."),
            b'S' => g(".#####.", "#......", "#......", ".####..", ".....#.", ".....#.", "#####.."),
            b'T' => g("#######", "...#...", "...#...", "...#...", "...#...", "...#...", "...#..."),
            b'U' => g("#....#.", "#....#.", "#....#.", "#....#.", "#....#.", "#....#.", ".####.."),
            b'V' => g("#....#.", "#....#.", "#....#.", "#....#.", ".#..#..", ".#..#..", "..##..."),
            b'W' => g("#.....#", "#.....#", "#.....#", "#..#..#", "#.#.#.#", "##...##", "#.....#"),
            b'X' => g("#....#.", ".#..#..", "..##...", "..##...", ".#..#..", "#....#.", "........"),
            b'Y' => g("#....#.", ".#..#..", "..##...", "...#...", "...#...", "...#...", "...#..."),
            b'Z' => g("#######", ".....#.", "....#..", "...#...", "..#....", ".#.....", "#######"),
            _ => g(".......",".......",".......",".......",".......",".......","......."),
        };
    }
    if cu >= '0' as u32 && cu <= '9' as u32 {
        return match cu as u8 {
            b'0' => g(".####..", "#....#.", "#...##.", "#.#.#..", "##..#..", "#....#.", ".####.."),
            b'1' => g("..#....", ".##....", "..#....", "..#....", "..#....", "..#....", ".###..."),
            b'2' => g(".####..", "#....#.", ".....#.", "...##..", "..#....", ".#.....", "#######"),
            b'3' => g("#####..", "....#..", "...#...", ".####..", ".....#.", "#....#.", ".####.."),
            b'4' => g("...##..", "..###..", ".#.#...", "##.#...", "#######", "...#...", "...#..."),
            b'5' => g("#######", "#......", "#####..", ".....#.", ".....#.", "#....#.", ".####.."),
            b'6' => g("..###..", ".#.....", "#......", "#####..", "#....#.", "#....#.", ".####.."),
            b'7' => g("#######", ".....#.", "....#..", "...#...", "..#....", "..#....", "..#...."),
            b'8' => g(".####..", "#....#.", "#....#.", ".####..", "#....#.", "#....#.", ".####.."),
            b'9' => g(".####..", "#....#.", "#....#.", ".#####.", ".....#.", "....#..", ".###..."),
            _ => g(".......",".......",".......",".......",".......",".......","......."),
        };
    }
    match cu as u8 as char {
        ' ' => Glyph([0; 8]),
        '.' => Glyph([0, 0, 0, 0, 0, r(".##...."), r(".##...."), 0]),
        ',' => Glyph([0, 0, 0, 0, r(".##...."), r(".##...."), r(".#....."), 0]),
        ':' => Glyph([0, r(".##...."), r(".##...."), 0, r(".##...."), r(".##...."), 0, 0]),
        ';' => Glyph([0, r(".##...."), r(".##...."), 0, r(".##...."), r(".#....."), r("#......"), 0]),
        '!' => Glyph([r("..#...."), r("..#...."), r("..#...."), r("..#...."), 0, r("..#...."), 0, 0]),
        '?' => Glyph([r(".###..."), r("#...#.."), r("....#.."), r("..##..."), r("..#...."), 0, r("..#...."), 0]),
        '-' => Glyph([0, 0, 0, r("#####.."), 0, 0, 0, 0]),
        '+' => Glyph([0, r("..#...."), r("..#...."), r("#####.."), r("..#...."), r("..#...."), 0, 0]),
        '=' => Glyph([0, 0, r("#####.."), 0, r("#####.."), 0, 0, 0]),
        '/' => Glyph([0, r(".....#."), r("....#.."), r("..#...."), r(".#....."), r("#......"), 0, 0]),
        '(' => Glyph([r("...#..."), r("..#...."), r("..#...."), r("..#...."), r("..#...."), r("..#...."), r("...#..."), 0]),
        ')' => Glyph([r("..#...."), r("...#..."), r("...#..."), r("...#..."), r("...#..."), r("...#..."), r("..#...."), 0]),
        '[' => Glyph([r("..###.."), r("..#...."), r("..#...."), r("..#...."), r("..#...."), r("..#...."), r("..###.."), 0]),
        ']' => Glyph([r("..###.."), r("....#.."), r("....#.."), r("....#.."), r("....#.."), r("....#.."), r("..###.."), 0]),
        '<' => Glyph([r("....#.."), r("...#..."), r("..#...."), r(".#....."), r("..#...."), r("...#..."), r("....#.."), 0]),
        '>' => Glyph([r("..#...."), r("...#..."), r("....#.."), r(".....#."), r("....#.."), r("...#..."), r("..#...."), 0]),
        '*' => Glyph([0, r("#.#.#.."), r(".###..."), r("#.#.#.."), 0, 0, 0, 0]),
        '#' => Glyph([r(".#.#..."), r("#######"), r(".#.#..."), r(".#.#..."), r("#######"), r(".#.#..."), 0, 0]),
        '%' => Glyph([r("##...#."), r("##..#.."), r("...#..."), r("..#...."), r(".#..##."), r("#...##."), 0, 0]),
        '_' => Glyph([0, 0, 0, 0, 0, 0, r("#######"), 0]),
        '|' => Glyph([r("..#...."), r("..#...."), r("..#...."), r("..#...."), r("..#...."), r("..#...."), r("..#...."), 0]),
        '&' => Glyph([r(".###..."), r("#...#.."), r("#...#.."), r(".###..."), r("#...#.."), r("#...#.."), r(".###.#."), 0]),
        '@' => Glyph([r(".####.."), r("#...#.."), r("#.#.#.."), r("##.#..."), r("#..##.."), r("#......"), r(".####.."), 0]),
        '\'' => Glyph([r("..#...."), r("..#...."), 0, 0, 0, 0, 0, 0]),
        '"' => Glyph([r("..#.#.."), r("..#.#.."), 0, 0, 0, 0, 0, 0]),
        _ => Glyph([0, 0, r("#######"), 0, r("#######"), 0, 0, 0]),
    }
}

pub fn draw_text(fb: &mut [u32], w: usize, h: usize, x: usize, y: usize, text: &str, color: u32, scale: usize) {
    let mut cx = x;
    for ch in text.chars() {
        draw_builtin(fb, w, h, cx, y, &glyph(ch), color, scale);
        cx += 8 * scale;
    }
}

fn draw_builtin(
    fb: &mut [u32],
    w: usize,
    h: usize,
    x: usize,
    y: usize,
    glyph: &Glyph,
    color: u32,
    scale: usize,
) {
    for (ry, bits) in glyph.0.iter().enumerate() {
        if *bits == 0 {
            continue;
        }
        for rx in 0..7usize {
            if bits & (1 << (6 - rx)) != 0 {
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = x + rx * scale + sx;
                        let py = y + ry * scale + sy;
                        if px < w && py < h {
                            fb[py * w + px] = color;
                        }
                    }
                }
            }
        }
    }
}

// ------------------------- CJK text (system font) -------------------------
//
// The builtin 8x8 font only covers ASCII. Game/folder names are frequently
// non-ASCII, so those characters are rasterized from a system CJK font when
// one is available; everything else stays on the pixel font.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const CJK_PX: usize = 12;

const FONT_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/opentype/source-han-cjk/SourceHanSansSC-Regular.otf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    "/usr/share/fonts/truetype/winfonts/NotoSansSC-VF.ttf",
    "/usr/share/fonts/truetype/winfonts/msyh.ttc",
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    "/usr/share/fonts/truetype/arphic/uming.ttc",
    "/System/Library/Fonts/PingFang.ttc",
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\simhei.ttf",
];

struct CjkGlyph {
    w: usize,
    h: usize,
    xmin: i32,
    advance: usize,
    data: Vec<u8>,
}

struct CjkFont {
    font: fontdue::Font,
    cache: HashMap<(char, usize), Option<CjkGlyph>>,
}

impl CjkFont {
    fn load() -> Option<CjkFont> {
        for path in FONT_CANDIDATES {
            if let Ok(data) = std::fs::read(path) {
                if let Ok(font) = fontdue::Font::from_bytes(data, fontdue::FontSettings::default())
                {
                    return Some(CjkFont { font, cache: HashMap::new() });
                }
            }
        }
        None
    }

    fn glyph(&mut self, ch: char, px: usize) -> Option<&CjkGlyph> {
        let key = (ch, px);
        if !self.cache.contains_key(&key) {
            let (m, data) = self.font.rasterize(ch, px as f32);
            let g = if m.width == 0 || m.height == 0 {
                None
            } else {
                Some(CjkGlyph {
                    w: m.width,
                    h: m.height,
                    xmin: m.xmin,
                    advance: (m.advance_width.ceil() as usize).max(1),
                    data,
                })
            };
            self.cache.insert(key, g);
        }
        self.cache.get(&key).and_then(|g| g.as_ref())
    }
}

fn cjk() -> &'static Mutex<Option<CjkFont>> {
    static CJK: OnceLock<Mutex<Option<CjkFont>>> = OnceLock::new();
    CJK.get_or_init(|| Mutex::new(CjkFont::load()))
}

fn blend_px(dst: u32, color: u32, a: u8) -> u32 {
    let a = a as u32;
    let mix = |d: u32, c: u32| (d * (255 - a) + c * a) / 255 & 0xFF;
    (mix((dst >> 16) & 0xFF, (color >> 16) & 0xFF) << 16)
        | (mix((dst >> 8) & 0xFF, (color >> 8) & 0xFF) << 8)
        | mix(dst & 0xFF, color & 0xFF)
}

fn draw_cjk_char(
    fb: &mut [u32],
    w: usize,
    h: usize,
    x: usize,
    y: usize,
    ch: char,
    color: u32,
    scale: usize,
) -> usize {
    let px = CJK_PX * scale;
    let Ok(mut guard) = cjk().lock() else {
        return 8 * scale;
    };
    let Some(font) = guard.as_mut() else {
        return 8 * scale;
    };
    let Some(g) = font.glyph(ch, px) else {
        return 8 * scale;
    };
    for gy in 0..g.h {
        for gx in 0..g.w {
            let cov = g.data[gy * g.w + gx];
            if cov == 0 {
                continue;
            }
            let dx = x as i32 + g.xmin + gx as i32;
            let dy = y as i32 + gy as i32;
            if dx >= 0 && dy >= 0 {
                let (dx, dy) = (dx as usize, dy as usize);
                if dx < w && dy < h {
                    let i = dy * w + dx;
                    fb[i] = blend_px(fb[i], color, cov);
                }
            }
        }
    }
    g.advance
}

/// Draw text using the pixel font for ASCII and a system CJK font otherwise.
pub fn draw_text_mixed(
    fb: &mut [u32],
    w: usize,
    h: usize,
    x: usize,
    y: usize,
    text: &str,
    color: u32,
    scale: usize,
) -> usize {
    let mut cx = x;
    for ch in text.chars() {
        if ch.is_ascii() {
            draw_builtin(fb, w, h, cx, y, &glyph(ch), color, scale);
            cx += 8 * scale;
        } else {
            cx += draw_cjk_char(fb, w, h, cx, y, ch, color, scale);
        }
    }
    cx
}

pub fn fill_rect(
    fb: &mut [u32],
    w: usize,
    h: usize,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    color: u32,
    blend: bool,
) {
    for y in y0..y1.min(h) {
        for x in x0..x1.min(w) {
            if blend {
                let c = fb[y * w + x];
                let mix = |a: u32, b: u32| (a / 2 + b / 2);
                fb[y * w + x] = (mix((c >> 16) & 0xFF, (color >> 16) & 0xFF) << 16)
                    | (mix((c >> 8) & 0xFF, (color >> 8) & 0xFF) << 8)
                    | mix(c & 0xFF, color & 0xFF);
            } else {
                fb[y * w + x] = color;
            }
        }
    }
}

/// Width in pixels of a single character (8px ASCII cell, CJK advance otherwise).
pub fn char_width(ch: char, scale: usize) -> usize {
    if ch.is_ascii() {
        return 8 * scale;
    }
    if let Ok(mut guard) = cjk().lock() {
        if let Some(font) = guard.as_mut() {
            if let Some(g) = font.glyph(ch, CJK_PX * scale) {
                return g.advance;
            }
        }
    }
    8 * scale
}

pub fn text_width(text: &str, scale: usize) -> usize {
    text.chars().map(|c| char_width(c, scale)).sum()
}
