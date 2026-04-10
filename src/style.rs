// Some helpers are queued for use once we wire up `style`/`classDef` parsing.
#![allow(dead_code)]

use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Default,
    Rgb(u8, u8, u8),
    // Raw SGR foreground code, e.g. 90 for bright black.
    Sgr(u8),
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color::Rgb(r, g, b)
    }

    pub const GREY: Self = Color::Sgr(90);
    pub const BRIGHT_WHITE: Self = Color::Sgr(97);
    pub const VIOLET: Self = Color::Rgb(188, 19, 254);
    pub const WHITE: Self = Color::Rgb(255, 255, 255);
    pub const NEON_GREEN: Self = Color::Rgb(110, 190, 130);
    pub const HOT_PINK: Self = Color::Rgb(255, 20, 147);
    pub const BRIGHT_YELLOW: Self = Color::Sgr(93);

    /// Emit an ANSI foreground SGR into `out`.
    pub fn write_fg(&self, out: &mut String) {
        match *self {
            Color::Default => out.push_str("\x1b[39m"),
            Color::Rgb(r, g, b) => {
                let _ = write!(out, "\x1b[38;2;{};{};{}m", r, g, b);
            }
            Color::Sgr(n) => {
                let _ = write!(out, "\x1b[{}m", n);
            }
        }
    }

    /// Emit an ANSI background SGR into `out`.
    pub fn write_bg(&self, out: &mut String) {
        match *self {
            Color::Default => out.push_str("\x1b[49m"),
            Color::Rgb(r, g, b) => {
                let _ = write!(out, "\x1b[48;2;{};{};{}m", r, g, b);
            }
            // Foreground 30..37 → bg 40..47; 90..97 → 100..107.
            Color::Sgr(n) => {
                let bg = if (30..=37).contains(&n) || (90..=97).contains(&n) {
                    n + 10
                } else {
                    n
                };
                let _ = write!(out, "\x1b[{}m", bg);
            }
        }
    }

    /// Parse `#rgb`, `#rrggbb`. Returns `None` on malformed input.
    pub fn parse_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#')?;
        match s.len() {
            3 => {
                let r = u8::from_str_radix(&s[0..1], 16).ok()?;
                let g = u8::from_str_radix(&s[1..2], 16).ok()?;
                let b = u8::from_str_radix(&s[2..3], 16).ok()?;
                // Expand: 0xF → 0xFF (each digit repeats).
                Some(Color::Rgb(r * 17, g * 17, b * 17))
            }
            6 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                Some(Color::Rgb(r, g, b))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
}

impl Style {
    pub const fn new() -> Self {
        Self {
            fg: None,
            bg: None,
            bold: false,
            italic: false,
            dim: false,
        }
    }
    pub const fn fg(fg: Color) -> Self {
        Self {
            fg: Some(fg),
            bg: None,
            bold: false,
            italic: false,
            dim: false,
        }
    }
    pub const fn dim() -> Self {
        Self {
            fg: None,
            bg: None,
            bold: false,
            italic: false,
            dim: true,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.fg.is_none() && self.bg.is_none() && !self.bold && !self.italic && !self.dim
    }
    pub fn write(&self, out: &mut String) {
        if self.bold {
            out.push_str("\x1b[1m");
        }
        if self.italic {
            out.push_str("\x1b[3m");
        }
        if self.dim {
            out.push_str("\x1b[2m");
        }
        if let Some(fg) = self.fg {
            fg.write_fg(out);
        }
        if let Some(bg) = self.bg {
            bg.write_bg(out);
        }
    }
}
