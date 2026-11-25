use super::*;
use crate::tab::SelectedTabColor;

#[test]
fn colors_round_trip_through_their_text() {
    for color in picker_rows().into_iter().flatten() {
        assert_eq!(color.to_string().parse::<TabColor>(), Ok(color), "{color}");
    }
    assert_eq!(TabColor::Rgb(1, 2, 255).to_string(), "#0102ff");
}

#[test]
fn colors_saved_by_older_builds_still_load() {
    // Older builds saved a tab's color as a bare ANSI name, or as a SelectedTabColor holding one.
    assert_eq!(
        serde_yaml::from_str::<TabColor>("red").unwrap(),
        TabColor::Ansi(AnsiColorIdentifier::Red)
    );
    let old =
        serde_yaml::to_string(&OldSelectedTabColor::Color(AnsiColorIdentifier::Cyan)).unwrap();
    assert_eq!(
        serde_yaml::from_str::<SelectedTabColor>(&old).unwrap(),
        SelectedTabColor::Color(TabColor::Ansi(AnsiColorIdentifier::Cyan))
    );
}

#[test]
fn new_colors_survive_a_save() {
    for color in [
        TabColor::Bright(AnsiColorIdentifier::Magenta),
        TabColor::Rgb(0xF9, 0x73, 0x16),
    ] {
        let saved = serde_yaml::to_string(&SelectedTabColor::Color(color)).unwrap();
        assert_eq!(
            serde_yaml::from_str::<SelectedTabColor>(&saved).unwrap(),
            SelectedTabColor::Color(color)
        );
    }
}

#[test]
fn typed_colors_parse() {
    assert_eq!(
        "Blue".parse(),
        Ok(TabColor::Ansi(AnsiColorIdentifier::Blue))
    );
    for bright in ["bright_red", "bright-red", "BrightRed", "bright red"] {
        assert_eq!(
            bright.parse(),
            Ok(TabColor::Bright(AnsiColorIdentifier::Red)),
            "{bright}"
        );
    }
    assert_eq!("ember".parse(), Ok(TabColor::Rgb(0xF9, 0x73, 0x16)));
    assert_eq!(" #FF8800 ".parse(), Ok(TabColor::Rgb(0xFF, 0x88, 0x00)));
    for bad in ["", "bright", "#ff88", "#gg0000", "plaid"] {
        assert!(bad.parse::<TabColor>().is_err(), "{bad:?}");
    }
}

#[test]
fn the_picker_offers_twenty_six_distinct_colors() {
    let colors: Vec<TabColor> = picker_rows().into_iter().flatten().collect();
    assert_eq!(colors.len(), 26);
    for (i, color) in colors.iter().enumerate() {
        assert!(!colors[i + 1..].contains(color), "{color} offered twice");
    }
}

/// `SelectedTabColor` as older builds defined it, to produce the text they saved.
#[derive(serde::Serialize)]
enum OldSelectedTabColor {
    Color(AnsiColorIdentifier),
}
