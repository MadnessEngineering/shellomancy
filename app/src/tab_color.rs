//! Shellomancy: tab and tab-group colors beyond the theme's six. The theme's ANSI colors and
//! their bright variants follow theme changes; the fixed hues and any hex color don't.
//!
//! A color is written as a string wherever it's saved or typed: an ANSI name (`red`, the form
//! older Shellomancy builds saved), `bright_red`, a fixed hue's name (`ember`), or `#rrggbb`.

use std::fmt;
use std::str::FromStr;

use pathfinder_color::ColorU;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use warp_core::ui::theme::{AnsiColorIdentifier, TerminalColors};

/// A tab or tab-group color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabColor {
    /// One of the theme's normal ANSI colors.
    Ansi(AnsiColorIdentifier),
    /// One of the theme's bright ANSI colors.
    Bright(AnsiColorIdentifier),
    /// A color the theme doesn't change.
    Rgb(u8, u8, u8),
}

/// The theme colors offered in the color picker, in its order.
pub const THEME_COLORS: [AnsiColorIdentifier; 6] = [
    AnsiColorIdentifier::Red,
    AnsiColorIdentifier::Green,
    AnsiColorIdentifier::Yellow,
    AnsiColorIdentifier::Blue,
    AnsiColorIdentifier::Magenta,
    AnsiColorIdentifier::Cyan,
];

/// A fixed hue with a name, for the picker's tooltip and for typing (`--color ember`).
pub struct FixedHue {
    pub name: &'static str,
    /// Voice key of the tooltip.
    pub label_key: &'static str,
    pub rgb: (u8, u8, u8),
}

/// Fixed hues spread around the wheel, in the picker's order: two rows of seven.
pub const FIXED_HUES: [FixedHue; 14] = [
    hue("ember", "shellomancy.tabColor.ember", 0xF9, 0x73, 0x16),
    hue("gold", "shellomancy.tabColor.gold", 0xF5, 0xB8, 0x3D),
    hue("lime", "shellomancy.tabColor.lime", 0xA3, 0xE6, 0x35),
    hue("mint", "shellomancy.tabColor.mint", 0x34, 0xD3, 0x99),
    hue("teal", "shellomancy.tabColor.teal", 0x2D, 0xD4, 0xBF),
    hue("sky", "shellomancy.tabColor.sky", 0x38, 0xBD, 0xF8),
    hue("indigo", "shellomancy.tabColor.indigo", 0x81, 0x8C, 0xF8),
    hue("violet", "shellomancy.tabColor.violet", 0xA7, 0x8B, 0xFA),
    hue("orchid", "shellomancy.tabColor.orchid", 0xE8, 0x79, 0xF9),
    hue("rose", "shellomancy.tabColor.rose", 0xFB, 0x71, 0x85),
    hue("crimson", "shellomancy.tabColor.crimson", 0xDC, 0x26, 0x26),
    hue("copper", "shellomancy.tabColor.copper", 0xB8, 0x73, 0x33),
    hue("moss", "shellomancy.tabColor.moss", 0x6B, 0x8E, 0x23),
    hue("slate", "shellomancy.tabColor.slate", 0x94, 0xA3, 0xB8),
];

const fn hue(name: &'static str, label_key: &'static str, r: u8, g: u8, b: u8) -> FixedHue {
    FixedHue {
        name,
        label_key,
        rgb: (r, g, b),
    }
}

/// The color picker's rows, top to bottom: theme colors, bright theme colors, fixed hues.
pub fn picker_rows() -> Vec<Vec<TabColor>> {
    let (first_hues, second_hues) = FIXED_HUES.split_at(FIXED_HUES.len() / 2);
    let fixed_row =
        |hues: &[FixedHue]| -> Vec<TabColor> { hues.iter().map(FixedHue::color).collect() };
    vec![
        THEME_COLORS.iter().copied().map(TabColor::Ansi).collect(),
        THEME_COLORS.iter().copied().map(TabColor::Bright).collect(),
        fixed_row(first_hues),
        fixed_row(second_hues),
    ]
}

impl FixedHue {
    pub fn color(&self) -> TabColor {
        let (r, g, b) = self.rgb;
        TabColor::Rgb(r, g, b)
    }
}

impl TabColor {
    /// The color drawn for this tab color under a theme with these terminal colors.
    pub fn to_color(self, terminal_colors: &TerminalColors) -> ColorU {
        match self {
            TabColor::Ansi(id) => id.to_ansi_color(&terminal_colors.normal).into(),
            TabColor::Bright(id) => id.to_ansi_color(&terminal_colors.bright).into(),
            TabColor::Rgb(r, g, b) => ColorU::new(r, g, b, 255),
        }
    }

    /// The tooltip naming this color in the picker.
    pub fn label(self) -> String {
        match self {
            TabColor::Ansi(id) => id.to_string(),
            TabColor::Bright(id) => madness_themes::tf(
                "shellomancy.tabColor.bright",
                &[("color", &id.to_string().to_lowercase())],
            ),
            TabColor::Rgb(..) => match self.fixed_hue() {
                Some(hue) => madness_themes::t(hue.label_key).to_owned(),
                None => self.to_string(),
            },
        }
    }

    fn fixed_hue(self) -> Option<&'static FixedHue> {
        FIXED_HUES.iter().find(|hue| hue.color() == self)
    }
}

impl From<AnsiColorIdentifier> for TabColor {
    fn from(id: AnsiColorIdentifier) -> Self {
        TabColor::Ansi(id)
    }
}

impl fmt::Display for TabColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TabColor::Ansi(id) => write!(f, "{}", id.to_string().to_lowercase()),
            TabColor::Bright(id) => write!(f, "bright_{}", id.to_string().to_lowercase()),
            TabColor::Rgb(r, g, b) => write!(f, "#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// Why a string isn't a tab color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseTabColorError(String);

impl fmt::Display for ParseTabColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown color '{}': use an ANSI name (red), bright_<name>, a fixed hue (ember, \
             gold, lime, mint, teal, sky, indigo, violet, orchid, rose, crimson, copper, moss, \
             slate) or #rrggbb",
            self.0
        )
    }
}

impl std::error::Error for ParseTabColorError {}

impl FromStr for TabColor {
    type Err = ParseTabColorError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let trimmed = text.trim();
        let error = || ParseTabColorError(trimmed.to_owned());
        if let Some(hex) = trimmed.strip_prefix('#') {
            if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(error());
            }
            let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| error());
            return Ok(TabColor::Rgb(channel(0)?, channel(2)?, channel(4)?));
        }
        let lower = trimmed.to_ascii_lowercase();
        if let Some(hue) = FIXED_HUES.iter().find(|hue| hue.name == lower) {
            return Ok(hue.color());
        }
        let bright_name = lower
            .strip_prefix("bright")
            .map(|rest| rest.trim_start_matches(['_', '-', ' ']));
        match bright_name {
            Some(name) if !name.is_empty() => AnsiColorIdentifier::from_str(name)
                .map(TabColor::Bright)
                .map_err(|_| error()),
            _ => AnsiColorIdentifier::from_str(&lower)
                .map(TabColor::Ansi)
                .map_err(|_| error()),
        }
    }
}

impl Serialize for TabColor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TabColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[path = "tab_color_tests.rs"]
mod tests;
