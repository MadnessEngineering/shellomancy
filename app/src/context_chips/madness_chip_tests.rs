use super::*;
use crate::mana_bar::{NextStep, Priority};

fn todo(status: &str) -> Todo {
    Todo {
        id: format!("t-{status}"),
        description: "x".to_owned(),
        project: "shellomancy".to_owned(),
        priority: Priority::Medium,
        status: status.to_owned(),
        created_at: 0,
        tags: vec![],
    }
}

fn quest(name: &str, completed: u64, total: u64) -> Quest {
    Quest {
        id: name.to_owned(),
        name: name.to_owned(),
        project: "shellomancy".to_owned(),
        total,
        completed,
        pct: if total == 0 {
            0
        } else {
            completed * 100 / total
        },
        next: None::<NextStep>,
    }
}

#[test]
fn omnispindle_label_counts_owed_and_doing() {
    assert_eq!(omnispindle_label(&[]), None);
    let owed = madness_themes::tf("shellomancy.chips.owed", &[("count", "2")]);
    assert_eq!(
        omnispindle_label(&[todo("pending"), todo("blocked")]),
        Some(owed.clone())
    );
    let doing = madness_themes::tf("shellomancy.chips.doing", &[("count", "1")]);
    assert_eq!(
        omnispindle_label(&[todo("pending"), todo("in_progress")]),
        Some(format!("{owed} · {doing}"))
    );
}

#[test]
fn quest_label_picks_the_least_finished_open_quest() {
    assert_eq!(quest_label(&[]), None);
    assert_eq!(
        quest_label(&[quest("Done", 3, 3), quest("Empty", 0, 0)]),
        None
    );
    let label = quest_label(&[quest("Nearly", 7, 8), quest("Integrations", 6, 8)]).unwrap();
    assert!(label.contains("Integrations"), "{label}");
    assert!(label.contains("6") && label.contains("8"), "{label}");
}

#[test]
fn long_quest_names_are_shortened() {
    let name = "Shellomancy Madness Integrations and Friends";
    let short = short_name(name);
    assert!(short.ends_with('…'), "{short}");
    assert!(short.chars().count() <= 29, "{short}");
    assert_eq!(short_name("Short"), "Short");
}
