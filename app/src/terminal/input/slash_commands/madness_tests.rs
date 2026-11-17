use super::*;
use crate::mana_bar::NextStep;
use crate::search::slash_command_menu::static_commands::commands::{
    MADNESS_LESSON, MADNESS_MAP, MADNESS_QUEST, MADNESS_SYNC, MADNESS_TODO, madness_commands,
};
use crate::search::slash_command_menu::static_commands::{
    Availability, SlashCommandKind, SlashCommandSurfaces,
};

const CWD: &str = "/nowhere/shellomancy-test/app";

fn quest(name: &str, next: Option<&str>) -> Quest {
    Quest {
        id: format!("quest-{name}"),
        name: name.to_owned(),
        project: "shellomancy".to_owned(),
        total: 8,
        completed: 6,
        pct: 75,
        next: next.map(|description| NextStep {
            todo_id: format!("todo-{name}"),
            description: description.to_owned(),
        }),
    }
}

#[test]
fn a_leading_bang_sets_the_priority() {
    assert_eq!(
        split_priority("!high fix the chips"),
        ("High", "fix the chips")
    );
    assert_eq!(split_priority("  !LOW  tidy up "), ("Low", "tidy up"));
    assert_eq!(
        split_priority("!critical prod is down"),
        ("Critical", "prod is down")
    );
    assert_eq!(split_priority("!medium meh"), ("Medium", "meh"));
    assert_eq!(split_priority("!high"), ("High", ""));
}

#[test]
fn text_without_a_marker_is_medium_and_untouched() {
    assert_eq!(split_priority("fix the chips"), ("Medium", "fix the chips"));
    assert_eq!(
        split_priority("!important thing"),
        ("Medium", "!important thing")
    );
    assert_eq!(split_priority("ship it!high"), ("Medium", "ship it!high"));
}

#[test]
fn todo_body_files_under_the_folder_with_three_tags() {
    let body = quick_todo_body("!high wire up /quest", Some(CWD));
    assert_eq!(body["description"], "wire up /quest");
    assert_eq!(body["priority"], "High");
    assert_eq!(body["project"], "app");
    assert_eq!(body["tags"].as_array().map(Vec::len), Some(3));
    assert_eq!(body["metadata"]["source"], "shellomancy");
    assert_eq!(body["metadata"]["cwd"], CWD);
}

#[test]
fn todo_body_without_a_folder_uses_the_fallback_project() {
    let body = quick_todo_body("remember the milk", None);
    assert_eq!(body["project"], "shellomancy");
    assert_eq!(body["priority"], "Medium");
}

#[test]
fn secrets_are_redacted_from_todos_and_lessons() {
    let key = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let text = format!("rotate GITHUB_TOKEN={key}");
    let todo = quick_todo_body(&text, Some(CWD));
    assert!(!todo.to_string().contains(key), "{todo}");
    let lesson = quick_lesson_body(&text, Some(CWD));
    assert!(!lesson.to_string().contains(key), "{lesson}");
}

#[test]
fn lesson_topic_is_the_first_line() {
    let body = quick_lesson_body(
        "  Cargo locks the target dir\nTwo checks in one target wait on each other.",
        Some(CWD),
    );
    assert_eq!(body["topic"], "Cargo locks the target dir");
    assert_eq!(
        body["lesson_learned"],
        "Cargo locks the target dir\nTwo checks in one target wait on each other."
    );
    assert_eq!(body["language"], "general");
    assert_eq!(body["tags"], json!(["shellomancy", "slash", "app"]));
}

#[test]
fn long_lesson_topics_are_cut() {
    let body = quick_lesson_body(&"x".repeat(300), None);
    let topic = body["topic"].as_str().unwrap();
    assert_eq!(topic.chars().count(), LESSON_TOPIC_CHARS + 1);
    assert!(topic.ends_with('…'));
}

#[test]
fn repo_root_finds_the_git_folder() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("crates/thing");
    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(repo_root(&nested), None);
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    assert_eq!(repo_root(&nested), Some(dir.path()));
}

#[test]
fn pick_quest_prefers_one_with_a_next_step() {
    assert!(pick_quest(&[]).is_none());
    let quests = [quest("blocked", None), quest("ready", Some("write tests"))];
    assert_eq!(pick_quest(&quests).unwrap().name, "ready");
    let blocked = [quest("blocked", None)];
    assert_eq!(pick_quest(&blocked).unwrap().name, "blocked");
}

#[test]
fn quest_message_names_progress_next_step_and_link() {
    let (message, link) = quest_message(&quest("Integrations", Some("ship /quest")), 0);
    assert_eq!(
        message,
        madness_themes::tf(
            "shellomancy.slash.quest.next",
            &[
                ("quest", "Integrations"),
                ("progress", "6/8"),
                ("next", "ship /quest")
            ],
        )
    );
    assert_eq!(
        link,
        Some(mana_bar::api::todo_page_url("todo-Integrations"))
    );
}

#[test]
fn quest_message_without_next_has_no_link_and_counts_others() {
    let (message, link) = quest_message(&quest("Blocked", None), 2);
    assert!(message.contains("Blocked"), "{message}");
    assert!(message.contains("6/8"), "{message}");
    assert!(
        message.ends_with(&madness_themes::tf(
            "shellomancy.slash.quest.more",
            &[("count", "2")]
        )),
        "{message}"
    );
    assert_eq!(link, None);
}

#[test]
fn madness_commands_are_gui_only_and_named() {
    let names: Vec<_> = madness_commands().iter().map(|c| c.name).collect();
    assert_eq!(names, ["/todo", "/lesson", "/map", "/quest", "/sync"]);
    for command in madness_commands() {
        assert!(
            matches!(
                command.supported_surfaces,
                SlashCommandSurfaces::GuiOnly { .. }
            ),
            "{}",
            command.name
        );
        assert!(matches!(command.kind, SlashCommandKind::Madness(_)));
        assert!(!command.auto_enter_ai_mode, "{}", command.name);
        assert!(
            !command.availability.contains(Availability::AI_ENABLED),
            "{} must work with AI off",
            command.name
        );
        assert!(!command.description.contains("Warp"), "{}", command.name);
    }
}

#[test]
fn todo_and_lesson_take_text_the_rest_run_on_selection() {
    for command in [&*MADNESS_TODO, &*MADNESS_LESSON] {
        let argument = command.argument.as_ref().expect(command.name);
        assert!(!argument.is_optional, "{}", command.name);
        assert!(argument.hint_text.is_some(), "{}", command.name);
    }
    for command in [&*MADNESS_MAP, &*MADNESS_QUEST, &*MADNESS_SYNC] {
        assert!(command.argument.is_none(), "{}", command.name);
    }
    assert!(MADNESS_MAP.availability.contains(Availability::LOCAL));
}

fn mapped(upload: Option<UploadOutcome>, error: Option<&str>) -> MappedFolder {
    MappedFolder {
        path: PathBuf::from("/nowhere/shellomancy"),
        name: "shellomancy".to_owned(),
        map_file: PathBuf::from("/nowhere/maps/shellomancy-uml.json"),
        mapped_at: Some(1_794_883_435),
        summary: Some(crate::code_maps::MapSummary {
            classes: 120,
            packages: 14,
            critical: 3,
            high: 9,
            unused_exports: 42,
        }),
        upload,
        error: error.map(str::to_owned),
        git: None,
    }
}

#[test]
fn an_uploaded_map_reports_its_numbers_and_links_to_inventorium() {
    let folder = mapped(
        Some(UploadOutcome::Uploaded {
            version: 7,
            view_url: Some("https://madnessinteractive.cc/swarmdesk/shellomancy".to_owned()),
        }),
        None,
    );
    let (message, link, worked) = map_done_message(&folder);
    assert!(worked);
    assert!(message.contains("shellomancy"), "{message}");
    assert!(message.contains("120"), "{message}");
    assert!(
        message.contains(&madness_themes::tf(
            "shellomancy.codeMaps.uploaded",
            &[("version", "7")]
        )),
        "{message}"
    );
    assert_eq!(
        link.as_deref(),
        Some("https://madnessinteractive.cc/swarmdesk/shellomancy")
    );
}

#[test]
fn a_local_map_has_no_link() {
    let (message, link, worked) = map_done_message(&mapped(Some(UploadOutcome::KeptLocal), None));
    assert!(worked);
    assert!(
        message.contains(madness_themes::t("shellomancy.codeMaps.keptLocal")),
        "{message}"
    );
    assert_eq!(link, None);
}

#[test]
fn a_failed_map_reports_the_error_even_with_an_old_summary() {
    let folder = mapped(Some(UploadOutcome::KeptLocal), Some("npx not found"));
    let (message, link, worked) = map_done_message(&folder);
    assert!(!worked);
    assert!(message.contains("npx not found"), "{message}");
    assert!(!message.contains("120"), "{message}");
    assert_eq!(link, None);
}
