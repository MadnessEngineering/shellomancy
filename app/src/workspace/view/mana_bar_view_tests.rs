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

#[test]
fn copy_all_is_every_filled_field_then_the_extra_uncut() {
    let long_notes = "n".repeat(MAX_DETAIL_CHARS + 50);
    let details = vec![
        Detail::copyable("id", "t1", "id"),
        Detail::new("ticket", "  "),
        Detail::new("notes", long_notes.clone()),
    ];
    assert_eq!(
        copy_all_text(&details, "\n- status: pending\n"),
        format!("id: t1\nnotes: {long_notes}\n\n- status: pending")
    );
    assert_eq!(copy_all_text(&[], "body"), "body");
    assert_eq!(copy_all_text(&details[..1], " "), "id: t1");
}

#[test]
fn changes_text_matches_the_diff_the_card_shows() {
    let changes = vec![
        api::Change {
            field: "status".to_owned(),
            old: Some("pending".to_owned()),
            new: Some("review".to_owned()),
        },
        api::Change {
            field: "tags".to_owned(),
            old: None,
            new: None,
        },
    ];
    assert_eq!(
        changes_text(&changes),
        "- status: pending\n+ status: review\n~ tags"
    );
}

#[test]
fn quest_markdown_is_a_checklist_per_chain_with_ids() {
    let todo = |id: &str, status: &str, description: &str| api::QuestTodo {
        id: id.to_owned(),
        status: status.to_owned(),
        description: description.to_owned(),
        ..Default::default()
    };
    let quest = api::Quest {
        next_ids: vec!["t3".to_owned()],
        chains: vec![
            api::QuestChain {
                label: "Foundation".to_owned(),
                total: 2,
                completed: 1,
                todos: vec![
                    todo("t1", "review", "Sign in"),
                    todo("t2", "cancelled", "Old idea"),
                ],
                ..Default::default()
            },
            api::QuestChain {
                label: "Next".to_owned(),
                total: 2,
                todos: vec![
                    todo("t3", "pending", "Sync\nsettings"),
                    todo("t4", "blocked", "Ship"),
                    api::QuestTodo {
                        id: "t5".to_owned(),
                        missing: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        quest_markdown(&quest),
        "## Foundation (1/2)\n\
         - [x] Sign in (`t1`)\n\
         - [-] Old idea (`t2`)\n\
         \n\
         ## Next (0/2)\n\
         - [ ] Sync settings <- next (`t3`)\n\
         - [ ] Ship [blocked] (`t4`)\n\
         - [ ] (deleted) (`t5`)"
    );
}
