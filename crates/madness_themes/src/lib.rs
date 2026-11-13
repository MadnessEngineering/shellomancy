//! MadnessThemes for Shellomancy.
//!
//! **Voices** are the UI text: every user-visible string Shellomancy has moved
//! off Warp's literals is a key in the `shellomancy` section of each voice in
//! the madness-themes submodule, compiled in at build time. [`t`] looks a key up
//! in the active voice, then in `standard`, then gives the key back.
//!
//! **Palettes** are the colour themes' terminal colours (their `terminal`
//! block), for the theme picker.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Deserialize;

/// The voice Shellomancy speaks until the user picks another.
pub const DEFAULT_VOICE: &str = "shellomancer";
/// Plain English; every other voice falls back to it key by key.
pub const BASE_VOICE: &str = "standard";

#[derive(Debug, Deserialize)]
pub struct Voice {
    pub id: String,
    pub name: String,
    pub description: String,
    strings: HashMap<String, String>,
}

impl Voice {
    /// This voice's own wording for `key`, without falling back.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.strings.get(key).map(String::as_str)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.strings.keys().map(String::as_str)
    }
}

static VOICES: LazyLock<Vec<Voice>> = LazyLock::new(|| {
    serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/voices.json")))
        .expect("voices.json is written by build.rs")
});

static ACTIVE: LazyLock<AtomicUsize> =
    LazyLock::new(|| AtomicUsize::new(index_of(DEFAULT_VOICE).unwrap_or(0)));

fn index_of(id: &str) -> Option<usize> {
    VOICES.iter().position(|v| v.id == id)
}

/// Every voice compiled into this build, in file-name order.
pub fn voices() -> &'static [Voice] {
    &VOICES
}

pub fn active_voice() -> &'static Voice {
    &VOICES[ACTIVE.load(Ordering::Relaxed)]
}

/// Switches the voice; returns false (and changes nothing) for an unknown id.
///
/// Strings already handed out keep their old wording, so a switch shows fully
/// only after a restart.
pub fn set_voice(id: &str) -> bool {
    match index_of(id) {
        Some(i) => {
            ACTIVE.store(i, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// The active voice's wording for `key`, else `standard`'s, else the key.
pub fn t(key: &'static str) -> &'static str {
    let active = active_voice();
    active
        .get(key)
        .or_else(|| index_of(BASE_VOICE).and_then(|i| VOICES[i].get(key)))
        .unwrap_or(key)
}

/// [`t`] with `{{name}}` placeholders filled in, the MadnessThemes convention:
/// `tf("shellomancy.x", &[("count", &n.to_string())])`. A placeholder with no
/// value is left as written, so a missing argument shows up instead of vanishing.
pub fn tf(key: &'static str, args: &[(&str, &str)]) -> String {
    let mut text = t(key).to_owned();
    for (name, value) in args {
        text = text.replace(&format!("{{{{{name}}}}}"), value);
    }
    text
}

/// Sixteen ANSI colours, as `#rrggbb`.
#[derive(Debug, Clone, Deserialize)]
pub struct AnsiColors {
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
    pub cyan: String,
    pub white: String,
}

/// A palette's `terminal` block. MadnessThemes' themeContract.js validates it.
#[derive(Debug, Clone, Deserialize)]
pub struct Terminal {
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub cursor: String,
    /// "darker" or "lighter".
    pub details: String,
    pub normal: AnsiColors,
    pub bright: AnsiColors,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Palette {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub terminal: Terminal,
}

static PALETTES: LazyLock<Vec<Palette>> = LazyLock::new(|| {
    serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/palettes.json")))
        .expect("palettes.json is written by build.rs")
});

/// Every palette with a terminal block, in file-name order.
pub fn palettes() -> &'static [Palette] {
    &PALETTES
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
