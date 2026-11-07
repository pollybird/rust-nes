//! Input configuration: per-player bindings, keyboard & gamepad, JSON persistence.

use serde::{Deserialize, Serialize};

pub const BTN_COUNT: usize = 8;
/// Canonical button order used everywhere:
/// 0=A, 1=B, 2=SELECT, 3=START, 4=UP, 5=DOWN, 6=LEFT, 7=RIGHT
pub const BTN_LABELS: [&str; BTN_COUNT] = ["A", "B", "SELECT", "START", "UP", "DOWN", "LEFT", "RIGHT"];

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum InputSource {
    Keyboard,
    Gamepad,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PlayerConfig {
    pub source: InputSource,
    /// key names, index = BTN order
    pub keyboard: Vec<String>,
    /// gilrs button names, index = BTN order
    pub gamepad: Vec<String>,
    /// which connected gamepad to use (0-based among active pads)
    pub pad_index: usize,
}

impl PlayerConfig {
    fn defaults(keyboard: [&str; BTN_COUNT], gamepad: [&str; BTN_COUNT]) -> Self {
        PlayerConfig {
            source: InputSource::Keyboard,
            keyboard: keyboard.iter().map(|s| s.to_string()).collect(),
            gamepad: gamepad.iter().map(|s| s.to_string()).collect(),
            pad_index: 0,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Config {
    pub p1: PlayerConfig,
    pub p2: PlayerConfig,
    pub muted: bool,
    pub scale: u32,
    /// Start the game in borderless fullscreen (toggle with Ctrl+Enter).
    #[serde(default)]
    pub fullscreen: bool,
    /// Folder the launcher scans for .nes files (empty = not chosen yet).
    #[serde(default)]
    pub rom_dir: String,
}

impl Default for Config {
    fn default() -> Self {
        let gp = ["South", "East", "Select", "Start", "DpadUp", "DpadDown", "DpadLeft", "DpadRight"];
        Config {
            p1: PlayerConfig::defaults(
                ["X", "Z", "Tab", "Enter", "Up", "Down", "Left", "Right"],
                gp,
            ),
            p2: PlayerConfig::defaults(
                ["G", "H", "F", "T", "W", "S", "A", "D"],
                gp,
            ),
            muted: false,
            scale: 3,
            fullscreen: false,
            rom_dir: String::new(),
        }
    }
}

impl Config {
    pub fn path() -> std::path::PathBuf {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        std::path::Path::new(&home).join("rust-nes-config.json")
    }

    pub fn load() -> Config {
        let p = Self::path();
        match std::fs::read_to_string(&p) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Config::default(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let s = serde_json::to_string_pretty(self).unwrap_or_default();
        std::fs::write(Self::path(), s)
    }
}

// ------------------------- keyboard name table -------------------------

use minifb::Key;

pub const KEY_NAMES: &[&str] = &[
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
    "S", "T", "U", "V", "W", "X", "Y", "Z",
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9",
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
    "Up", "Down", "Left", "Right", "Enter", "Space", "Tab", "Backspace", "Escape",
    "LShift", "RShift", "LCtrl", "RCtrl", "LAlt", "RAlt", "CapsLock", "NumLock", "ScrollLock",
    "Comma", "Period", "Slash", "Backslash", "Semicolon", "Quote", "Backquote",
    "LBracket", "RBracket", "Minus", "Equal", "Home", "End", "PageUp", "PageDown",
    "Insert", "Delete", "Pause", "Menu",
    "Num0", "Num1", "Num2", "Num3", "Num4", "Num5", "Num6", "Num7", "Num8", "Num9",
    "NumPlus", "NumMinus", "NumAsterisk", "NumSlash", "NumEnter", "NumDot",
];

pub fn key_name(k: Key) -> Option<&'static str> {
    Some(match k {
        Key::A => "A", Key::B => "B", Key::C => "C", Key::D => "D", Key::E => "E",
        Key::F => "F", Key::G => "G", Key::H => "H", Key::I => "I", Key::J => "J",
        Key::K => "K", Key::L => "L", Key::M => "M", Key::N => "N", Key::O => "O",
        Key::P => "P", Key::Q => "Q", Key::R => "R", Key::S => "S", Key::T => "T",
        Key::U => "U", Key::V => "V", Key::W => "W", Key::X => "X", Key::Y => "Y",
        Key::Z => "Z",
        Key::Key0 => "0", Key::Key1 => "1", Key::Key2 => "2", Key::Key3 => "3",
        Key::Key4 => "4", Key::Key5 => "5", Key::Key6 => "6", Key::Key7 => "7",
        Key::Key8 => "8", Key::Key9 => "9",
        Key::F1 => "F1", Key::F2 => "F2", Key::F3 => "F3", Key::F4 => "F4",
        Key::F5 => "F5", Key::F6 => "F6", Key::F7 => "F7", Key::F8 => "F8",
        Key::F9 => "F9", Key::F10 => "F10", Key::F11 => "F11", Key::F12 => "F12",
        Key::Up => "Up", Key::Down => "Down", Key::Left => "Left", Key::Right => "Right",
        Key::Enter => "Enter", Key::Space => "Space", Key::Tab => "Tab",
        Key::Backspace => "Backspace", Key::Escape => "Escape",
        Key::LeftShift => "LShift", Key::RightShift => "RShift",
        Key::LeftCtrl => "LCtrl", Key::RightCtrl => "RCtrl",
        Key::LeftAlt => "LAlt", Key::RightAlt => "RAlt", Key::CapsLock => "CapsLock",
        Key::NumLock => "NumLock", Key::ScrollLock => "ScrollLock",
        Key::Comma => "Comma", Key::Period => "Period", Key::Slash => "Slash",
        Key::Backslash => "Backslash", Key::Semicolon => "Semicolon",
        Key::Apostrophe => "Quote", Key::Backquote => "Backquote",
        Key::LeftBracket => "LBracket", Key::RightBracket => "RBracket",
        Key::Minus => "Minus", Key::Equal => "Equal",
        Key::Home => "Home", Key::End => "End", Key::PageUp => "PageUp",
        Key::PageDown => "PageDown", Key::Insert => "Insert", Key::Delete => "Delete",
        Key::Pause => "Pause", Key::Menu => "Menu",
        Key::NumPad0 => "Num0", Key::NumPad1 => "Num1", Key::NumPad2 => "Num2",
        Key::NumPad3 => "Num3", Key::NumPad4 => "Num4", Key::NumPad5 => "Num5",
        Key::NumPad6 => "Num6", Key::NumPad7 => "Num7", Key::NumPad8 => "Num8",
        Key::NumPad9 => "Num9", Key::NumPadPlus => "NumPlus",
        Key::NumPadMinus => "NumMinus", Key::NumPadAsterisk => "NumAsterisk",
        Key::NumPadSlash => "NumSlash", Key::NumPadEnter => "NumEnter",
        Key::NumPadDot => "NumDot",
        _ => return None,
    })
}

pub fn name_to_key(name: &str) -> Option<Key> {
    Some(match name {
        "A" => Key::A, "B" => Key::B, "C" => Key::C, "D" => Key::D, "E" => Key::E,
        "F" => Key::F, "G" => Key::G, "H" => Key::H, "I" => Key::I, "J" => Key::J,
        "K" => Key::K, "L" => Key::L, "M" => Key::M, "N" => Key::N, "O" => Key::O,
        "P" => Key::P, "Q" => Key::Q, "R" => Key::R, "S" => Key::S, "T" => Key::T,
        "U" => Key::U, "V" => Key::V, "W" => Key::W, "X" => Key::X, "Y" => Key::Y,
        "Z" => Key::Z,
        "0" => Key::Key0, "1" => Key::Key1, "2" => Key::Key2, "3" => Key::Key3,
        "4" => Key::Key4, "5" => Key::Key5, "6" => Key::Key6, "7" => Key::Key7,
        "8" => Key::Key8, "9" => Key::Key9,
        "F1" => Key::F1, "F2" => Key::F2, "F3" => Key::F3, "F4" => Key::F4,
        "F5" => Key::F5, "F6" => Key::F6, "F7" => Key::F7, "F8" => Key::F8,
        "F9" => Key::F9, "F10" => Key::F10, "F11" => Key::F11, "F12" => Key::F12,
        "Up" => Key::Up, "Down" => Key::Down, "Left" => Key::Left, "Right" => Key::Right,
        "Enter" => Key::Enter, "Space" => Key::Space, "Tab" => Key::Tab,
        "Backspace" => Key::Backspace, "Escape" => Key::Escape,
        "LShift" => Key::LeftShift, "RShift" => Key::RightShift,
        "LCtrl" => Key::LeftCtrl, "RCtrl" => Key::RightCtrl,
        "LAlt" => Key::LeftAlt, "RAlt" => Key::RightAlt, "CapsLock" => Key::CapsLock,
        "NumLock" => Key::NumLock, "ScrollLock" => Key::ScrollLock,
        "Comma" => Key::Comma, "Period" => Key::Period, "Slash" => Key::Slash,
        "Backslash" => Key::Backslash, "Semicolon" => Key::Semicolon,
        "Quote" => Key::Apostrophe, "Backquote" => Key::Backquote,
        "LBracket" => Key::LeftBracket, "RBracket" => Key::RightBracket,
        "Minus" => Key::Minus, "Equal" => Key::Equal,
        "Home" => Key::Home, "End" => Key::End, "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown, "Insert" => Key::Insert, "Delete" => Key::Delete,
        "Pause" => Key::Pause, "Menu" => Key::Menu,
        "Num0" => Key::NumPad0, "Num1" => Key::NumPad1, "Num2" => Key::NumPad2,
        "Num3" => Key::NumPad3, "Num4" => Key::NumPad4, "Num5" => Key::NumPad5,
        "Num6" => Key::NumPad6, "Num7" => Key::NumPad7, "Num8" => Key::NumPad8,
        "Num9" => Key::NumPad9, "NumPlus" => Key::NumPadPlus,
        "NumMinus" => Key::NumPadMinus, "NumAsterisk" => Key::NumPadAsterisk,
        "NumSlash" => Key::NumPadSlash, "NumEnter" => Key::NumPadEnter,
        "NumDot" => Key::NumPadDot,
        _ => return None,
    })
}

// ------------------------- gamepad name table -------------------------

pub const PAD_NAMES: &[&str] = &[
    "South", "East", "North", "West", "LeftTrigger", "RightTrigger", "LeftTrigger2",
    "RightTrigger2", "Select", "Start", "Mode", "LeftThumb", "RightThumb",
    "DpadUp", "DpadDown", "DpadLeft", "DpadRight",
];

#[cfg(feature = "gamepad")]
pub fn pad_button_name(b: gilrs::Button) -> &'static str {
    use gilrs::Button::*;
    match b {
        South => "South", East => "East", North => "North", West => "West",
        LeftTrigger => "LeftTrigger", RightTrigger => "RightTrigger",
        LeftTrigger2 => "LeftTrigger2", RightTrigger2 => "RightTrigger2",
        Select => "Select", Start => "Start", Mode => "Mode",
        LeftThumb => "LeftThumb", RightThumb => "RightThumb",
        DPadUp => "DPadUp", DPadDown => "DPadDown", DPadLeft => "DPadLeft", DPadRight => "DPadRight",
        _ => "Other",
    }
}

#[cfg(feature = "gamepad")]
pub fn name_to_pad_button(name: &str) -> Option<gilrs::Button> {
    use gilrs::Button::*;
    Some(match name {
        "South" => South, "East" => East, "North" => North, "West" => West,
        "LeftTrigger" => LeftTrigger, "RightTrigger" => RightTrigger,
        "LeftTrigger2" => LeftTrigger2, "RightTrigger2" => RightTrigger2,
        "Select" => Select, "Start" => Start, "Mode" => Mode,
        "LeftThumb" => LeftThumb, "RightThumb" => RightThumb,
        "DPadUp" => DPadUp, "DPadDown" => DPadDown, "DPadLeft" => DPadLeft, "DPadRight" => DPadRight,
        _ => return None,
    })
}
