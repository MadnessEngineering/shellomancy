use super::*;

#[test]
fn when_label_is_local_time_and_age() {
    assert_eq!(when_label(0, 100), "");
    let at = 1_794_744_000;
    let label = when_label(at, at + 3 * 3600);
    let local = chrono::DateTime::from_timestamp(at, 0)
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M")
        .to_string();
    assert_eq!(label, format!("{local} (3h)"));
}

#[test]
fn log_card_keys_stay_put_when_newer_entries_arrive() {
    let entry = api::LogEntry {
        todo_id: "t1".to_owned(),
        operation: "update".to_owned(),
        at: 1_794_744_000,
        ..Default::default()
    };
    assert_eq!(log_card_key(&entry), "log:t1:1794744000:update");
    let completed = api::LogEntry {
        operation: "complete".to_owned(),
        ..entry.clone()
    };
    assert_ne!(log_card_key(&entry), log_card_key(&completed));
}

#[test]
fn long_values_are_cut_and_diff_lines_stay_on_one_line() {
    let long = "x".repeat(MAX_DETAIL_CHARS + 50);
    let cut = truncate(&long);
    assert_eq!(cut.chars().count(), MAX_DETAIL_CHARS + 1);
    assert!(cut.ends_with('…'));
    assert_eq!(truncate("short"), "short");
    assert_eq!(
        one_line("first line\n  second\tline "),
        "first line second line"
    );
}

#[test]
fn card_button_words_are_voiced() {
    for key in [
        "shellomancy.manaBar.copy",
        "shellomancy.manaBar.copied",
        "shellomancy.manaBar.copyText",
        "shellomancy.manaBar.open",
    ] {
        assert_ne!(madness_themes::t(key), key, "{key} has no wording");
    }
}
