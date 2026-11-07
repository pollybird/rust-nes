//! rust-nes: NES emulator shell — window, settings overlay, audio, input.

mod apu;
mod config;
mod console;
mod cpu;
mod font;
mod mapper;
mod ppu;
mod rom;

use apu::Ring;
use config::{Config, InputSource, BTN_LABELS};
use console::Console;
use font::{char_width, draw_text, draw_text_mixed, fill_rect, text_width};
use minifb::{Key, KeyRepeat, Window, WindowOptions};
use std::sync::{Arc, Mutex};

const W: usize = 256;
const H: usize = 240;

const COL_BG: u32 = 0x080810;
const COL_TEXT: u32 = 0xE0E0E0;
const COL_SEL: u32 = 0x40A0FF;
const COL_VALUE: u32 = 0xFFD060;
const COL_HINT: u32 = 0x80C080;
const COL_RED: u32 = 0xFF6060;

/// gamepad handle, feature-gated
#[cfg(feature = "gamepad")]
type Pads = Option<gilrs::Gilrs>;
#[cfg(not(feature = "gamepad"))]
type Pads = Option<()>;

enum Row {
    Source(usize),
    PadIdx(usize),
    Bind(usize, usize),
    Mute,
    Fullscreen,
}

struct Ui {
    open: bool,
    selected: usize,
    capture: Option<(usize, usize)>, // (player, button idx)
    status_line: String,
}

impl Ui {
    fn new() -> Self {
        Ui { open: false, selected: 0, capture: None, status_line: String::new() }
    }
}

fn build_rows(cfg: &Config) -> Vec<Row> {
    let mut rows = Vec::new();
    for p in 0..2usize {
        rows.push(Row::Source(p));
        let pc = if p == 0 { &cfg.p1 } else { &cfg.p2 };
        if pc.source == InputSource::Gamepad {
            rows.push(Row::PadIdx(p));
        }
        for b in 0..8usize {
            rows.push(Row::Bind(p, b));
        }
    }
    rows.push(Row::Mute);
    rows.push(Row::Fullscreen);
    rows
}

// ---------------------------------------------------------------------------
// Launcher: ROM folder picker + game list
// ---------------------------------------------------------------------------

/// Canvas size used by the launcher UI (scaled into the window by `present`).
const LW: usize = 320;
const LH: usize = 240;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Picker,
    List,
    Play,
}

struct GameEntry {
    name: String,
    path: std::path::PathBuf,
}

enum PickItem {
    Up,
    Choose,
    Dir(String, std::path::PathBuf),
}

struct Picker {
    dir: std::path::PathBuf,
    items: Vec<PickItem>,
    sel: usize,
    /// true when opened from the game list, so ESC goes back instead of quitting
    from_list: bool,
    err: String,
}

impl Picker {
    fn new(dir: std::path::PathBuf, from_list: bool) -> Picker {
        let dir = if dir.is_dir() { dir } else { home_dir() };
        let mut p = Picker { dir, items: Vec::new(), sel: 0, from_list, err: String::new() };
        p.refresh();
        p
    }

    fn refresh(&mut self) {
        let mut items = Vec::new();
        let has_parent = self.dir.parent().is_some();
        if has_parent {
            items.push(PickItem::Up);
        }
        items.push(PickItem::Choose);
        let mut dirs: Vec<(String, std::path::PathBuf)> = Vec::new();
        match std::fs::read_dir(&self.dir) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let path = e.path();
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') {
                        continue;
                    }
                    if path.is_dir() {
                        dirs.push((name, path));
                    }
                }
                self.err.clear();
            }
            Err(e) => self.err = format!("READ DIR FAILED: {}", e),
        }
        dirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        for (name, path) in dirs {
            items.push(PickItem::Dir(name, path));
        }
        self.items = items;
        self.sel = if has_parent { 1 } else { 0 };
    }
}

struct Session {
    console: Console,
    sram_path: String,
    save_counter: u32,
    paused: bool,
    /// ignore pad input until the keys used to start the game are released
    warmup: bool,
    title: String,
}

impl Session {
    fn start(
        path: &std::path::Path,
        cfg: &Config,
        ring: &Arc<Mutex<Ring>>,
        audio_rate: Option<f64>,
    ) -> Result<Session, String> {
        let path_str = path.to_string_lossy().to_string();
        let rom = rom::Rom::load(&path_str)?;
        let mut console = Console::new(rom.clone())?;
        if let Some(rate) = audio_rate {
            console.apu.set_ring(Some(ring.clone()), rate);
        }
        console.apu.set_muted(cfg.muted);
        let sram_path = format!("{}.sram", path_str);
        if let Ok(data) = std::fs::read(&sram_path) {
            console.restore_sram(&data);
        }
        console.reset();
        let title = if rom.info.title.is_empty() {
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "NES".to_string())
        } else {
            rom.info.title.clone()
        };
        Ok(Session { console, sram_path, save_counter: 0, paused: false, warmup: true, title })
    }
}

fn home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

fn scan_roms(dir: &std::path::Path) -> Vec<GameEntry> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let path = e.path();
            if !path.is_file() {
                continue;
            }
            let is_nes = path
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("nes"))
                .unwrap_or(false);
            if is_nes {
                v.push(GameEntry {
                    name: e.file_name().to_string_lossy().to_string(),
                    path,
                });
            }
        }
    }
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

fn truncate_right(s: &str, max_px: usize) -> String {
    if text_width(s, 1) <= max_px {
        return s.to_string();
    }
    let ell = text_width("...", 1);
    let mut w = 0usize;
    let mut out = String::new();
    for ch in s.chars() {
        let cw = char_width(ch, 1);
        if w + cw + ell > max_px {
            break;
        }
        w += cw;
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn truncate_left(s: &str, max_px: usize) -> String {
    if text_width(s, 1) <= max_px {
        return s.to_string();
    }
    let ell = text_width("...", 1);
    let chars: Vec<char> = s.chars().collect();
    let mut w = 0usize;
    let mut start = chars.len();
    for i in (0..chars.len()).rev() {
        let cw = char_width(chars[i], 1);
        if w + cw + ell > max_px {
            break;
        }
        w += cw;
        start = i;
    }
    format!("...{}", chars[start..].iter().collect::<String>())
}

fn draw_picker(fb: &mut [u32], p: &Picker) {
    fill_rect(fb, LW, LH, 0, 0, LW, LH, COL_BG, false);
    fill_rect(fb, LW, LH, 2, 2, LW - 2, LH - 2, 0x101820, false);
    draw_text(fb, LW, LH, 8, 6, "RUST-NES", COL_SEL, 1);
    draw_text(fb, LW, LH, 80, 6, "SELECT ROM FOLDER", COL_TEXT, 1);
    fill_rect(fb, LW, LH, 6, 18, LW - 6, 36, 0x1A2230, false);
    let path = truncate_left(&p.dir.display().to_string(), LW - 22);
    draw_text_mixed(fb, LW, LH, 8, 20, &path, COL_VALUE, 1);
    if !p.err.is_empty() {
        draw_text_mixed(fb, LW, LH, 8, 39, &truncate_right(&p.err, LW - 16), COL_RED, 1);
    }

    let y0 = 54usize;
    let dy = 13usize;
    let rows = ((LH - 16 - y0) / dy).max(1);
    let first = if p.sel >= rows { p.sel + 1 - rows } else { 0 };
    for (i, item) in p.items.iter().enumerate().skip(first).take(rows) {
        let y = y0 + (i - first) * dy;
        let sel = i == p.sel;
        if sel {
            fill_rect(fb, LW, LH, 4, y - 1, LW - 4, y + 12, 0x2A3A5A, false);
        }
        let (text, color) = match item {
            PickItem::Up => ("[..] UP".to_string(), COL_HINT),
            PickItem::Choose => ("[USE THIS FOLDER]".to_string(), COL_VALUE),
            PickItem::Dir(name, _) => (format!("[+] {}", name), COL_TEXT),
        };
        draw_text_mixed(fb, LW, LH, 8, y, &truncate_right(&text, LW - 16), color, 1);
    }
    draw_text(fb, LW, LH, 6, LH - 12, "UP/DN MOVE  ENTER OPEN/USE  ESC BACK", COL_HINT, 1);
}

fn draw_list(
    fb: &mut [u32],
    dir: &str,
    games: &[GameEntry],
    sel: usize,
    msg: &str,
) {
    fill_rect(fb, LW, LH, 0, 0, LW, LH, COL_BG, false);
    fill_rect(fb, LW, LH, 2, 2, LW - 2, LH - 2, 0x101820, false);
    draw_text(fb, LW, LH, 8, 6, "RUST-NES", COL_SEL, 1);
    draw_text(fb, LW, LH, 80, 6, "GAME LIST", COL_TEXT, 1);
    fill_rect(fb, LW, LH, 6, 18, LW - 6, 36, 0x1A2230, false);
    draw_text_mixed(fb, LW, LH, 8, 20, &truncate_left(dir, LW - 22), COL_VALUE, 1);
    if !msg.is_empty() {
        draw_text_mixed(fb, LW, LH, 8, 39, &truncate_right(msg, LW - 16), COL_RED, 1);
    } else if games.is_empty() {
        draw_text(fb, LW, LH, 8, 39, "NO .NES FILES FOUND - PRESS D", COL_RED, 1);
    }

    let y0 = 54usize;
    let dy = 13usize;
    let rows = ((LH - 26 - y0) / dy).max(1);
    let first = if sel >= rows { sel + 1 - rows } else { 0 };
    for (i, g) in games.iter().enumerate().skip(first).take(rows) {
        let y = y0 + (i - first) * dy;
        let sel_i = i == sel;
        if sel_i {
            fill_rect(fb, LW, LH, 4, y - 1, LW - 4, y + 12, 0x2A3A5A, false);
            draw_text(fb, LW, LH, 6, y, ">", COL_SEL, 1);
        }
        let color = if sel_i { COL_SEL } else { COL_TEXT };
        draw_text_mixed(fb, LW, LH, 16, y, &truncate_right(&g.name, LW - 22), color, 1);
    }
    draw_text(fb, LW, LH, 6, LH - 12, "ENTER START  D FOLDER  ESC QUIT", COL_HINT, 1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--smoke" {
        console::smoke_test();
        return;
    }
    if matches!(args.get(1).map(|s| s.as_str()), Some("--help") | Some("-h")) {
        println!("用法: rust-nes [游戏镜像.nes]");
        println!("  不带参数启动时先选择 ROM 目录，再显示游戏列表");
        println!("  支持 iNES / NES2.0 镜像，Mapper 0/1/2/3/4/7/9/11/66");
        println!("热键: F1 设置(键位/手柄/全屏)  Ctrl+Enter 全屏  F2 复位  F3 暂停  ESC 返回列表/退出");
        return;
    }

    let mut cfg = Config::load();

    // ---- audio ----
    let ring: Arc<Mutex<Ring>> = Arc::new(Mutex::new(Ring::new(8192)));
    #[cfg(feature = "audio")]
    let (audio_rate, _audio_stream) = {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let mut rate: Option<f64> = None;
        let stream = cpal::default_host().default_output_device().and_then(|dev| {
            let config = dev.default_output_config().ok()?;
            rate = Some(config.sample_rate().0 as f64);
            let channels = config.channels() as usize;
            let ring_cb = ring.clone();
            let err_fn = |e| eprintln!("audio stream error: {}", e);
            let stream = dev
                .build_output_stream(
                    &config.into(),
                    move |data: &mut [f32], _| {
                        if let Ok(mut r) = ring_cb.lock() {
                            r.drain_into(data, channels);
                        }
                    },
                    err_fn,
                    None,
                )
                .ok()?;
            let _ = stream.play();
            Some(stream)
        });
        (rate, stream)
    };
    #[cfg(not(feature = "audio"))]
    let audio_rate: Option<f64> = {
        let _ = &ring;
        None
    };

    // ---- window ----
    let scale = cfg.scale.max(1).min(6) as usize;
    let mut window = Window::new("rust-nes", W * scale, H * scale, WindowOptions::default())
        .expect("无法创建窗口");
    window.set_target_fps(60);
    window.limit_update_rate(Some(std::time::Duration::from_micros(16600)));

    #[cfg(feature = "gamepad")]
    let mut pads: Pads = gilrs::Gilrs::new().ok();
    #[cfg(not(feature = "gamepad"))]
    let mut pads: Pads = None;

    let mut ui = Ui::new();
    let mut fs_pending = cfg.fullscreen;
    let mut fb: Vec<u32> = vec![0u32; W * H];
    let mut lfb: Vec<u32> = vec![0u32; LW * LH];
    // window-sized output buffer (reallocated when the window is resized,
    // e.g. when entering/leaving fullscreen)
    let mut out: Vec<u32> = Vec::new();

    // ---- launcher state ----
    let rom_dir = std::path::PathBuf::from(&cfg.rom_dir);
    let has_rom_dir = !cfg.rom_dir.is_empty() && rom_dir.is_dir();
    let mut games: Vec<GameEntry> = if has_rom_dir { scan_roms(&rom_dir) } else { Vec::new() };
    let mut game_sel = 0usize;
    let mut list_msg = String::new();
    let mut picker: Option<Picker> = None;
    let mut session: Option<Session> = None;
    let mut quiet_input = false;
    let mut quit = false;

    let mut screen = if let Some(arg) = args.get(1) {
        let path = std::path::PathBuf::from(arg);
        match Session::start(&path, &cfg, &ring, audio_rate) {
            Ok(s) => {
                window.set_title(&format!("rust-nes - {}", s.title));
                session = Some(s);
                Screen::Play
            }
            Err(e) => {
                fatal(&format!("无法加载镜像: {}", e));
                return;
            }
        }
    } else if has_rom_dir {
        Screen::List
    } else {
        picker = Some(Picker::new(home_dir(), false));
        Screen::Picker
    };

    while window.is_open() && !quit {
        let pressed: Vec<Key> = window.get_keys_pressed(KeyRepeat::No);

        // apply a requested fullscreen mode once the window is mapped
        if fs_pending {
            fs_pending = false;
            set_fullscreen(&window, true);
        }

        // Ctrl+Enter: toggle fullscreen (not while capturing a bind)
        let capturing = screen == Screen::Play && ui.open && ui.capture.is_some();
        if !capturing
            && pressed.contains(&Key::Enter)
            && (window.is_key_down(Key::LeftCtrl) || window.is_key_down(Key::RightCtrl))
        {
            let want = !cfg.fullscreen;
            if set_fullscreen(&window, want) {
                cfg.fullscreen = want;
                let _ = cfg.save();
            }
        }

        // ---------------- input ----------------
        // NOTE: minifb only clears its "pressed since last frame" set inside
        // update(), so every iteration must end with exactly one present().
        match screen {
            // ---------------- ROM folder picker ----------------
            Screen::Picker => {
                let mut p = picker.take().unwrap_or_else(|| Picker::new(home_dir(), false));
                let n = p.items.len();
                if n > 0 {
                    if pressed.contains(&Key::Down) || pressed.contains(&Key::S) {
                        p.sel = (p.sel + 1) % n;
                    }
                    if pressed.contains(&Key::Up) || pressed.contains(&Key::W) {
                        p.sel = (p.sel + n - 1) % n;
                    }
                }
                if pressed.contains(&Key::Escape) {
                    if p.from_list {
                        screen = Screen::List;
                    } else {
                        quit = true;
                    }
                } else if pressed.contains(&Key::Enter) || pressed.contains(&Key::Space) {
                    match p.items.get(p.sel) {
                        Some(PickItem::Up) => {
                            if let Some(parent) = p.dir.parent() {
                                p.dir = parent.to_path_buf();
                                p.refresh();
                            }
                        }
                        Some(PickItem::Choose) => {
                            let dir = p.dir.clone();
                            cfg.rom_dir = dir.display().to_string();
                            let _ = cfg.save();
                            games = scan_roms(&dir);
                            game_sel = 0;
                            list_msg.clear();
                            quiet_input = true;
                            screen = Screen::List;
                        }
                        Some(PickItem::Dir(_, path)) => {
                            p.dir = path.clone();
                            p.refresh();
                        }
                        None => {}
                    }
                }
                picker = Some(p);
            }

            // ---------------- game list ----------------
            Screen::List => {
                if quiet_input {
                    if !window.is_key_down(Key::Enter) && !window.is_key_down(Key::Space) {
                        quiet_input = false;
                    }
                } else if pressed.contains(&Key::Escape) {
                    quit = true;
                } else if pressed.contains(&Key::D) {
                    picker = Some(Picker::new(
                        std::path::PathBuf::from(&cfg.rom_dir),
                        true,
                    ));
                    screen = Screen::Picker;
                } else {
                    if !games.is_empty() {
                        if pressed.contains(&Key::Down) || pressed.contains(&Key::S) {
                            game_sel = (game_sel + 1) % games.len();
                            list_msg.clear();
                        }
                        if pressed.contains(&Key::Up) || pressed.contains(&Key::W) {
                            game_sel = (game_sel + games.len() - 1) % games.len();
                            list_msg.clear();
                        }
                    }
                    if pressed.contains(&Key::Enter) || pressed.contains(&Key::Space) {
                        if let Some(g) = games.get(game_sel) {
                            match Session::start(&g.path, &cfg, &ring, audio_rate) {
                                Ok(s) => {
                                    window.set_title(&format!("rust-nes - {}", s.title));
                                    session = Some(s);
                                    if let Ok(mut r) = ring.lock() {
                                        r.buf.clear();
                                    }
                                    screen = Screen::Play;
                                    quiet_input = true;
                                }
                                Err(e) => list_msg = format!("LOAD FAILED: {}", e),
                            }
                        }
                    }
                }
            }

            // ---------------- game ----------------
            Screen::Play => {
                if ui.open {
                    if pressed.contains(&Key::F1) && ui.capture.is_none() {
                        ui.open = false;
                        ui.capture = None;
                    } else {
                        handle_settings(&mut window, &mut cfg, &mut ui, &mut pads);
                    }
                } else {
                    if pressed.contains(&Key::F1) {
                        ui.open = true;
                        ui.capture = None;
                    }
                    if pressed.contains(&Key::F2) {
                        if let Some(s) = session.as_mut() {
                            s.console.ppu.reset();
                            s.console.reset();
                        }
                    }
                    if pressed.contains(&Key::F3) {
                        if let Some(s) = session.as_mut() {
                            s.paused = !s.paused;
                        }
                    }
                    if pressed.contains(&Key::Escape) {
                        if let Some(s) = &session {
                            save_sram(&s.console, &s.sram_path);
                        }
                        screen = Screen::List;
                        quiet_input = true;
                        list_msg.clear();
                        window.set_title("rust-nes");
                    }
                }

                if screen == Screen::Play {
                    if let Some(s) = session.as_mut() {
                        // wait for the launcher keys to be released before the
                        // pad starts receiving input (avoids a stray START press)
                        if s.warmup
                            && !window.is_key_down(Key::Enter)
                            && !window.is_key_down(Key::Space)
                        {
                            s.warmup = false;
                        }
                        if !ui.open && !s.paused {
                            let m1 = if s.warmup {
                                0
                            } else {
                                poll_pad(&window, &mut pads, &cfg.p1)
                            };
                            let m2 = if s.warmup {
                                0
                            } else {
                                poll_pad(&window, &mut pads, &cfg.p2)
                            };
                            s.console.set_pad(0, m1);
                            s.console.set_pad(1, m2);

                            let frame_no_before = s.console.ppu.frame;
                            s.console.run_frame();
                            if s.console.ppu.frame != frame_no_before {
                                s.save_counter += 1;
                                if s.save_counter >= 600 {
                                    s.save_counter = 0;
                                    save_sram(&s.console, &s.sram_path);
                                }
                            }
                        }
                    }
                }

                // drain gamepad events even when not capturing (keeps queue fresh)
                #[cfg(feature = "gamepad")]
                if let Some(g) = &mut pads {
                    while g.next_event().is_some() {}
                }
            }
        }

        if quit {
            break;
        }

        // ---------------- render ----------------
        match screen {
            Screen::Picker => {
                if picker.is_none() {
                    picker = Some(Picker::new(home_dir(), false));
                }
                if let Some(p) = &picker {
                    draw_picker(&mut lfb, p);
                }
                present(&mut window, &lfb, LW, LH, &mut out);
            }
            Screen::List => {
                draw_list(&mut lfb, &cfg.rom_dir, &games, game_sel, &list_msg);
                present(&mut window, &lfb, LW, LH, &mut out);
            }
            Screen::Play => {
                if let Some(s) = session.as_mut() {
                    s.console.apu.set_muted(cfg.muted);
                    let src = s.console.framebuffer();
                    fb.copy_from_slice(src);
                    if ui.open {
                        draw_settings_overlay(&mut fb, &cfg, &ui);
                    } else if s.paused {
                        draw_text(&mut fb, W, H, 8, 4, "PAUSED  (F3)", COL_RED, 1);
                    }
                    present(&mut window, &fb, W, H, &mut out);
                } else {
                    // no game to draw; make sure the frame still pumps events
                    present(&mut window, &fb, W, H, &mut out);
                }
            }
        }

        // once the list (or picker) is on screen, drop the game console
        if screen != Screen::Play && session.is_some() {
            session = None;
            if let Ok(mut r) = ring.lock() {
                r.buf.clear();
            }
        }
    }

    if let Some(s) = &session {
        save_sram(&s.console, &s.sram_path);
    }
    let _ = cfg.save();
}

fn save_sram(console: &Console, path: &str) {
    let s = console.sram_snapshot();
    if !s.iter().all(|b| *b == 0) || std::path::Path::new(path).exists() {
        let _ = std::fs::write(path, s);
    }
}

fn poll_pad(window: &Window, pads: &mut Pads, pc: &config::PlayerConfig) -> u8 {
    let mut mask = 0u8;
    match pc.source {
        InputSource::Keyboard => {
            for (i, name) in pc.keyboard.iter().enumerate().take(8) {
                if let Some(k) = config::name_to_key(name) {
                    if window.is_key_down(k) {
                        mask |= 1 << i;
                    }
                }
            }
        }
        InputSource::Gamepad => {
            #[cfg(feature = "gamepad")]
            if let Some(gilrs) = pads {
                let pad_ids: Vec<gilrs::GamepadId> =
                    gilrs.gamepads().map(|(id, _)| id).collect();
                if let Some(&id) = pad_ids.get(pc.pad_index) {
                    let gp = gilrs.gamepad(id);
                    for (i, name) in pc.gamepad.iter().enumerate().take(8) {
                        if let Some(b) = config::name_to_pad_button(name) {
                            if gp.button_data(b).map(|d| d.is_pressed()).unwrap_or(false) {
                                mask |= 1 << i;
                            }
                        }
                    }
                }
            }
            #[cfg(not(feature = "gamepad"))]
            let _ = pads;
        }
    }
    mask
}

/// settings page input handling (called every frame while open)
fn handle_settings(window: &mut Window, cfg: &mut Config, ui: &mut Ui, pads: &mut Pads) {
    let pressed: Vec<Key> = window.get_keys_pressed(KeyRepeat::Yes);
    let rows = build_rows(cfg);
    let n = rows.len();

    // capture mode first
    if let Some((player, btn)) = ui.capture {
        // keyboard capture
        if let Some(k) = pressed.iter().find(|k| config::key_name(**k).is_some()) {
            if let Some(name) = config::key_name(*k) {
                let p = if player == 0 { &mut cfg.p1 } else { &mut cfg.p2 };
                if name == "Escape" {
                    ui.capture = None;
                } else {
                    p.keyboard[btn] = name.to_string();
                    p.source = InputSource::Keyboard;
                    ui.capture = None;
                    ui.status_line = format!("P{} {} = {}", player + 1, BTN_LABELS[btn], name);
                    let _ = cfg.save();
                }
                return;
            }
        }
        // gamepad capture
        #[cfg(feature = "gamepad")]
        if let Some(g) = pads {
            while let Some(ev) = g.next_event() {
                if let gilrs::EventType::ButtonPressed(b, _) = ev.event {
                    let name = config::pad_button_name(b);
                    if name != "Other" {
                        let p = if player == 0 { &mut cfg.p1 } else { &mut cfg.p2 };
                        p.gamepad[btn] = name.to_string();
                        p.source = InputSource::Gamepad;
                        ui.status_line = format!("P{} {} = PAD:{}", player + 1, BTN_LABELS[btn], name);
                    }
                    ui.capture = None;
                    let _ = cfg.save();
                    return;
                }
            }
        }
        #[cfg(not(feature = "gamepad"))]
        {
            let _ = pads;
        }
        // escape cancels via keyboard even without gamepad
        if pressed.contains(&Key::Escape) {
            ui.capture = None;
        }
        return;
    }

    if pressed.contains(&Key::Escape) {
        ui.open = false;
        return;
    }
    if pressed.contains(&Key::Down) || pressed.contains(&Key::S) {
        ui.selected = (ui.selected + 1) % n;
    }
    if pressed.contains(&Key::Up) || pressed.contains(&Key::W) {
        ui.selected = (ui.selected + n - 1) % n;
    }
    let change = pressed.contains(&Key::Left) || pressed.contains(&Key::Right)
        || pressed.contains(&Key::Enter) || pressed.contains(&Key::Space);
    if !change {
        return;
    }
    match &rows[ui.selected.min(n - 1)] {
        Row::Source(p) => {
            let left = pressed.contains(&Key::Left);
            let pp = if *p == 0 { &mut cfg.p1 } else { &mut cfg.p2 };
            pp.source = if left { InputSource::Keyboard } else { InputSource::Gamepad };
            let _ = cfg.save();
        }
        Row::PadIdx(p) => {
            let right = pressed.contains(&Key::Right);
            let pp = if *p == 0 { &mut cfg.p1 } else { &mut cfg.p2 };
            if right {
                pp.pad_index = (pp.pad_index + 1) % 4;
            } else if pp.pad_index > 0 {
                pp.pad_index -= 1;
            }
            let _ = cfg.save();
        }
        Row::Bind(p, b) => {
            if pressed.contains(&Key::Enter) || pressed.contains(&Key::Space) {
                ui.capture = Some((*p, *b));
                ui.status_line = format!(
                    "PRESS P{} {} KEY OR PAD BUTTON (ESC CANCEL)",
                    p + 1,
                    BTN_LABELS[*b]
                );
            }
        }
        Row::Mute => {
            cfg.muted = !cfg.muted;
            let _ = cfg.save();
        }
        Row::Fullscreen => {
            cfg.fullscreen = !cfg.fullscreen;
            let _ = cfg.save();
            set_fullscreen(window, cfg.fullscreen);
        }
    }
}

fn draw_settings_overlay(fb: &mut [u32], cfg: &Config, ui: &Ui) {
    fill_rect(fb, W, H, 0, 0, W, H, COL_BG, true);
    fill_rect(fb, W, H, 2, 2, W - 2, H - 2, 0x181828, false);
    draw_text(fb, W, H, 8, 6, "RUST-NES SETTINGS", COL_SEL, 1);
    draw_text(fb, W, H, 150, 6, "F1/ESC CLOSE", COL_HINT, 1);

    let rows = build_rows(cfg);
    let y0 = 20usize;
    let dy = 11usize;
    // keep the selected row on screen when the list is longer than the overlay
    let visible = ((H - 14 - y0) / dy).max(1);
    let first = if ui.selected >= visible { ui.selected + 1 - visible } else { 0 };
    for (i, row) in rows.iter().enumerate().skip(first).take(visible) {
        let y = y0 + (i - first) * dy;
        let sel = i == ui.selected;
        let label: String;
        let value: String;
        match row {
            Row::Source(p) => {
                label = format!("P{} DEVICE", p + 1);
                let s = if p == &0 { cfg.p1.source } else { cfg.p2.source };
                value = match s {
                    InputSource::Keyboard => "KEYBOARD".to_string(),
                    InputSource::Gamepad => "GAMEPAD".to_string(),
                };
            }
            Row::PadIdx(p) => {
                label = format!("P{} PAD#", p + 1);
                let s = if p == &0 { cfg.p1.pad_index } else { cfg.p2.pad_index };
                value = s.to_string();
            }
            Row::Bind(p, b) => {
                label = format!("P{} {}", p + 1, BTN_LABELS[*b]);
                let pc = if p == &0 { &cfg.p1 } else { &cfg.p2 };
                value = match pc.source {
                    InputSource::Keyboard => pc.keyboard[*b].clone(),
                    InputSource::Gamepad => pc.gamepad[*b].clone(),
                };
            }
            Row::Mute => {
                label = "AUDIO".to_string();
                value = if cfg.muted { "OFF" } else { "ON" }.to_string();
            }
            Row::Fullscreen => {
                label = "FULLSCREEN".to_string();
                value = if cfg.fullscreen { "ON" } else { "OFF" }.to_string();
            }
        }
        if sel {
            fill_rect(fb, W, H, 4, y - 1, W - 4, y + 9, 0x2A3A5A, false);
            draw_text(fb, W, H, 6, y, ">", COL_SEL, 1);
        }
        draw_text(fb, W, H, 16, y, &label, COL_TEXT, 1);
        draw_text(fb, W, H, 100, y, &value, COL_VALUE, 1);
    }

    let hy = H - 12;
    if ui.capture.is_some() {
        draw_text(fb, W, H, 4, hy - 11, &ui.status_line, COL_RED, 1);
    }
    draw_text(fb, W, H, 4, hy, "UP/DN MOVE  L/R CHANGE  ENTER SET KEY", COL_HINT, 1);
}

/// Blit an `sw` x `sh` framebuffer into the current window size with
/// aspect-preserving nearest-neighbour scaling (letterboxed).
fn present(window: &mut Window, src: &[u32], sw: usize, sh: usize, out: &mut Vec<u32>) {
    let (w, h) = window.get_size();
    if w == 0 || h == 0 || sw == 0 || sh == 0 {
        return;
    }
    if out.len() != w * h {
        out.resize(w * h, 0);
    }
    let scale = (w as f64 / sw as f64).min(h as f64 / sh as f64);
    let dw = ((sw as f64 * scale) as usize).clamp(1, w);
    let dh = ((sh as f64 * scale) as usize).clamp(1, h);
    let ox = (w - dw) / 2;
    let oy = (h - dh) / 2;
    out.fill(0);
    for y in 0..dh {
        let sy = (y * sh / dh).min(sh - 1);
        let s = &src[sy * sw..sy * sw + sw];
        let row = &mut out[(oy + y) * w + ox..(oy + y) * w + ox + dw];
        for (x, px) in row.iter_mut().enumerate() {
            *px = s[(x * sw / dw).min(sw - 1)];
        }
    }
    let _ = window.update_with_buffer(out, w, h);
}

/// Ask the window manager to toggle borderless fullscreen (X11/EWMH).
/// Returns false when the current backend has no window-manager protocol.
#[cfg(target_os = "linux")]
fn set_fullscreen(window: &Window, on: bool) -> bool {
    use raw_window_handle::{
        HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
    };
    let (Ok(wh), Ok(dh)) = (window.window_handle(), window.display_handle()) else {
        return false;
    };
    let win = match wh.as_raw() {
        RawWindowHandle::Xlib(h) => h.window,
        _ => return false,
    };
    let display = match dh.as_raw() {
        RawDisplayHandle::Xlib(h) => match h.display {
            Some(p) => p.as_ptr() as *mut x11_dl::xlib::Display,
            None => return false,
        },
        _ => return false,
    };
    let Ok(xlib) = x11_dl::xlib::Xlib::open() else {
        return false;
    };
    unsafe {
        let state = (xlib.XInternAtom)(display, b"_NET_WM_STATE\0".as_ptr().cast(), 0);
        let full = (xlib.XInternAtom)(
            display,
            b"_NET_WM_STATE_FULLSCREEN\0".as_ptr().cast(),
            0,
        );
        let root = (xlib.XDefaultRootWindow)(display);
        let mut ev: x11_dl::xlib::XEvent = std::mem::zeroed();
        ev.client_message.type_ = x11_dl::xlib::ClientMessage;
        ev.client_message.send_event = 1;
        ev.client_message.display = display;
        ev.client_message.window = win;
        ev.client_message.message_type = state;
        ev.client_message.format = 32;
        ev.client_message.data.set_long(0, if on { 1 } else { 0 });
        ev.client_message.data.set_long(1, full as _);
        ev.client_message.data.set_long(3, 1);
        (xlib.XSendEvent)(
            display,
            root,
            0,
            x11_dl::xlib::SubstructureRedirectMask | x11_dl::xlib::SubstructureNotifyMask,
            &mut ev,
        );
        (xlib.XFlush)(display);
    }
    true
}

#[cfg(not(target_os = "linux"))]
fn set_fullscreen(_window: &Window, _on: bool) -> bool {
    false
}

fn fatal(msg: &str) {
    // show a small window with the message; block until user closes it
    let ww = 640usize;
    let wh = 200usize;
    if let Ok(mut window) = Window::new("rust-nes", ww, wh, WindowOptions::default()) {
        let mut buf = vec![0u32; ww * wh];
        fill_rect(&mut buf, ww, wh, 0, 0, ww, wh, 0x100808, false);
        draw_text(&mut buf, ww, wh, 16, 16, "RUST-NES ERROR", COL_RED, 2);
        // wrap message at ~72 chars per line
        let chars: Vec<char> = msg.chars().collect();
        let mut y = 60usize;
        let mut line: String = String::new();
        for c in chars {
            if c == '\n' || line.chars().count() >= 72 {
                draw_text(&mut buf, ww, wh, 16, y, &line, COL_TEXT, 2);
                line.clear();
                y += 24;
            }
            line.push(c);
        }
        if !line.is_empty() {
            draw_text(&mut buf, ww, wh, 16, y, &line, COL_TEXT, 2);
        }
        draw_text(&mut buf, ww, wh, 16, wh - 30, "CLOSE WINDOW TO EXIT", COL_HINT, 2);
        while window.is_open() && !window.is_key_down(Key::Escape) {
            let _ = window
                .update_with_buffer(&buf, ww, wh)
                .is_ok();
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    } else {
        eprintln!("错误: {}", msg);
    }
}
