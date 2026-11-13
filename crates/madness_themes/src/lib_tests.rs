use std::collections::BTreeSet;

use super::*;

fn base() -> &'static Voice {
    voices().iter().find(|v| v.id == BASE_VOICE).unwrap()
}

#[test]
fn default_and_base_voices_are_compiled_in() {
    assert!(voices().iter().any(|v| v.id == DEFAULT_VOICE));
    assert!(
        base().keys().count() > 0,
        "standard has no shellomancy strings"
    );
}

#[test]
fn every_voice_has_exactly_standards_keys() {
    let want: BTreeSet<&str> = base().keys().collect();
    for voice in voices() {
        let have: BTreeSet<&str> = voice.keys().collect();
        assert_eq!(have, want, "voice {} drifts from standard", voice.id);
    }
}

#[test]
fn debug_echoes_its_keys() {
    let debug = voices().iter().find(|v| v.id == "debug").unwrap();
    for key in debug.keys() {
        assert_eq!(debug.get(key), Some(key));
    }
}

#[test]
fn shellomancy_strings_never_say_warp() {
    for voice in voices() {
        for key in voice.keys() {
            let text = voice.get(key).unwrap();
            assert!(
                !text.contains("Warp"),
                "{}:{key} says Warp: {text}",
                voice.id
            );
        }
    }
}

#[test]
fn lookup_falls_back_to_the_key_and_switching_is_checked() {
    assert_eq!(t("shellomancy.no.such.key"), "shellomancy.no.such.key");
    assert!(!set_voice("no-such-voice"));
    assert!(set_voice(BASE_VOICE));
    assert_eq!(t("shellomancy.plugin.update"), "Update Shellomancy plugin");
    assert!(set_voice(DEFAULT_VOICE));
    assert_eq!(active_voice().id, DEFAULT_VOICE);
}

#[test]
fn every_palette_has_a_full_terminal_block() {
    assert!(palettes().len() >= 13);
    for p in palettes() {
        let t = &p.terminal;
        for c in [
            &t.background,
            &t.foreground,
            &t.accent,
            &t.cursor,
            &t.normal.cyan,
            &t.bright.white,
        ] {
            assert!(c.len() == 7 && c.starts_with('#'), "{}: {c}", p.id);
        }
        assert!(t.details == "darker" || t.details == "lighter", "{}", p.id);
    }
}
