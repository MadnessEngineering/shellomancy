use serde_json::json;

use super::*;

fn theme(value: Value, at: i64) -> Option<Stamped<Value>> {
    Some(Stamped { value, at })
}

fn voice(value: &str, at: i64) -> Option<Stamped<String>> {
    Some(Stamped {
        value: value.to_owned(),
        at,
    })
}

#[test]
fn newer_local_values_win_and_are_pushed() {
    let local = SyncDoc {
        v: 1,
        theme: theme(json!("Dracula"), 20),
        voice: voice("shellomancer", 5),
    };
    let remote = SyncDoc {
        v: 1,
        theme: theme(json!("Light"), 10),
        voice: voice("dwarf", 15),
    };

    let merged = merge(&local, Some(&remote));

    assert_eq!(merged.doc.theme, theme(json!("Dracula"), 20));
    assert_eq!(merged.doc.voice, voice("dwarf", 15));
    assert_eq!(merged.apply, vec![Key::Voice]);
    assert!(merged.push);
}

#[test]
fn ties_go_to_the_remote_copy() {
    let local = SyncDoc {
        theme: theme(json!("Dracula"), 10),
        ..Default::default()
    };
    let remote = SyncDoc {
        theme: theme(json!("Light"), 10),
        ..Default::default()
    };

    let merged = merge(&local, Some(&remote));

    assert_eq!(merged.doc.theme, theme(json!("Light"), 10));
    assert_eq!(merged.apply, vec![Key::Theme]);
    assert!(!merged.push);
}

#[test]
fn identical_values_need_nothing() {
    let doc = SyncDoc {
        v: 1,
        theme: theme(json!("Dark"), 3),
        voice: voice("standard", 4),
    };

    let merged = merge(&doc, Some(&doc));

    assert!(merged.apply.is_empty());
    assert!(!merged.push);
}

#[test]
fn nothing_stored_yet_pushes_the_local_copy() {
    let local = SyncDoc {
        voice: voice("dwarf", 1),
        ..Default::default()
    };

    let merged = merge(&local, None);

    assert!(merged.push);
    assert!(merged.apply.is_empty());
    assert_eq!(merged.doc.v, DOC_VERSION);
}

#[test]
fn the_category_stamp_is_the_newest_value_stamp() {
    let doc = SyncDoc {
        v: 1,
        theme: theme(json!("Dark"), 7),
        voice: voice("dwarf", 42),
    };
    assert_eq!(doc.stamp(), 42);
    assert_eq!(SyncDoc::default().stamp(), 0);
}

#[test]
fn custom_and_in_memory_themes_are_never_synced() {
    let custom = serde_json::from_value::<ThemeKind>(json!({
        "Custom": {"name": "Mine", "path": "/Users/me/.warp/themes/mine.yaml"}
    }));
    if let Ok(custom) = custom {
        assert!(!is_portable_theme(&custom));
        assert_eq!(theme_value(&custom), None);
    }
    assert!(is_portable_theme(&ThemeKind::Dracula));
    assert_eq!(
        theme_value(&ThemeKind::Madness("labops".to_owned())),
        Some(json!({"Madness": "labops"}))
    );
}

#[test]
fn only_themes_this_build_has_are_applied() {
    let palettes = ["labops", "gunmetal"];

    assert_eq!(
        theme_from_value(&json!({"Madness": "labops"}), &palettes),
        Some(ThemeKind::Madness("labops".to_owned()))
    );
    assert_eq!(
        theme_from_value(&json!({"Madness": "nope"}), &palettes),
        None
    );
    assert_eq!(
        theme_from_value(&json!("Dracula"), &palettes),
        Some(ThemeKind::Dracula)
    );
    assert_eq!(theme_from_value(&json!("NotATheme"), &palettes), None);
}

#[test]
fn follow_reads_the_web_apps_palette_and_voice_fields() {
    let palettes = ["labops"];
    let voices = ["mad-wizard", "shellomancer"];
    let appearance =
        json!({"chromamancyTheme": "labops", "colorTheme": "mad-wizard", "language": "en"});

    assert_eq!(
        follow_targets(&appearance, &palettes, &voices),
        (
            Some(ThemeKind::Madness("labops".to_owned())),
            Some("mad-wizard".to_owned())
        )
    );
}

#[test]
fn follow_ignores_missing_and_unknown_values() {
    let palettes = ["labops"];
    let voices = ["mad-wizard"];

    assert_eq!(
        follow_targets(&Value::Null, &palettes, &voices),
        (None, None)
    );
    assert_eq!(
        follow_targets(
            &json!({"chromamancyTheme": "gone", "colorTheme": "gone"}),
            &palettes,
            &voices
        ),
        (None, None)
    );
}

#[test]
fn every_inventorium_palette_and_voice_is_known_here() {
    // Inventorium's registries (src/locales/chromamancyRegistry.js and
    // src/locales/index.js) share MadnessThemes with Shellomancy.
    let palettes: Vec<&str> = madness_themes::palettes()
        .iter()
        .map(|palette| palette.id.as_str())
        .collect();
    let voices: Vec<&str> = madness_themes::voices()
        .iter()
        .map(|voice| voice.id.as_str())
        .collect();
    for id in [
        "standard",
        "labops",
        "gunmetal",
        "cyan-lab",
        "mad-laboratory",
    ] {
        assert!(palettes.contains(&id), "palette {id}");
    }
    for id in ["mad-wizard", "shellomancer", "standard", "dwarf", "banana"] {
        assert!(voices.contains(&id), "voice {id}");
    }
}

#[test]
fn unsent_changes_are_those_newer_than_the_last_ack() {
    let mut state = LocalState {
        doc: SyncDoc {
            voice: voice("dwarf", 50),
            ..Default::default()
        },
        last_acked: 40,
        account: "me".to_owned(),
    };
    assert!(state.has_unsent());
    state.last_acked = 50;
    assert!(!state.has_unsent());
    assert_eq!(LocalState::from_json(&state.to_json()), state);
    assert_eq!(LocalState::from_json("not json"), LocalState::default());
}
