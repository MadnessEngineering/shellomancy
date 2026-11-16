use serde_json::json;

use super::*;

fn todo(id: &str, priority: Priority, status: &str, created_at: i64) -> Todo {
    Todo {
        id: id.to_owned(),
        description: format!("todo {id}"),
        project: "shellomancy".to_owned(),
        priority,
        status: status.to_owned(),
        created_at,
        tags: vec![],
    }
}

#[test]
fn todos_parse_from_the_list_envelope() {
    let reply = json!({"todos": [{
        "id": "t1", "description": "Ship the Mana Bar", "project": "shellomancy",
        "priority": "High", "status": "in_progress", "created_at": 1763000000,
        "tags": ["mana-bar", "ui"], "metadata": {"quest_id": "q", "tags": ["ui", "panel"]},
        "_scope": "personal"
    }], "total_pending": 0});

    assert_eq!(
        parse_todos(&reply),
        vec![Todo {
            id: "t1".to_owned(),
            description: "Ship the Mana Bar".to_owned(),
            project: "shellomancy".to_owned(),
            priority: Priority::High,
            status: "in_progress".to_owned(),
            created_at: 1763000000,
            // Root tags first, then metadata tags, without repeats.
            tags: vec!["mana-bar".to_owned(), "ui".to_owned(), "panel".to_owned()],
        }]
    );
}

#[test]
fn todos_with_missing_fields_get_defaults() {
    let reply = json!({"todos": [{"id": "t1", "created_at": "1763000000"}]});

    let todos = parse_todos(&reply);

    assert_eq!(todos[0].priority, Priority::Medium);
    assert_eq!(todos[0].status, "pending");
    assert_eq!(todos[0].description, "");
    assert_eq!(todos[0].created_at, 1763000000);
}

#[test]
fn todos_without_an_id_are_skipped() {
    let reply = json!({"todos": [{"description": "orphan"}]});

    assert_eq!(parse_todos(&reply), vec![]);
}

#[test]
fn finished_todos_are_not_shown() {
    let reply = json!({"todos": [
        {"id": "a", "status": "pending"},
        {"id": "b", "status": "done"},
        {"id": "c", "status": "archived"},
        {"id": "d", "status": "Cancelled"},
        {"id": "e", "status": "review"}
    ]});

    let ids: Vec<String> = parse_todos(&reply).into_iter().map(|t| t.id).collect();

    assert_eq!(ids, vec!["a", "e"]);
}

#[test]
fn priority_aliases_match_inventorium() {
    assert_eq!(Priority::parse("p0"), Priority::Critical);
    assert_eq!(Priority::parse("Urgent"), Priority::Critical);
    assert_eq!(Priority::parse("hi"), Priority::High);
    assert_eq!(Priority::parse("lo"), Priority::Low);
    assert_eq!(Priority::parse("minor"), Priority::Low);
    assert_eq!(Priority::parse(""), Priority::Medium);
    assert_eq!(Priority::parse("whenever"), Priority::Medium);
}

#[test]
fn groups_run_critical_to_low_with_work_in_progress_first() {
    let todos = vec![
        todo("low", Priority::Low, "pending", 1),
        todo("high-old", Priority::High, "pending", 1),
        todo("high-new", Priority::High, "pending", 9),
        todo("high-busy", Priority::High, "in_progress", 1),
        todo("crit", Priority::Critical, "blocked", 1),
    ];

    let groups: Vec<(Priority, Vec<String>)> = group_by_priority(todos)
        .into_iter()
        .map(|(priority, group)| (priority, group.into_iter().map(|t| t.id).collect()))
        .collect();

    assert_eq!(
        groups,
        vec![
            (Priority::Critical, vec!["crit".to_owned()]),
            (
                Priority::High,
                vec![
                    "high-busy".to_owned(),
                    "high-new".to_owned(),
                    "high-old".to_owned()
                ]
            ),
            (Priority::Low, vec!["low".to_owned()]),
        ]
    );
}

#[test]
fn quests_parse_with_progress_and_next_step() {
    let reply = json!({"success": true, "data": [{
        "quest": {"id": "q1", "name": "Bigger builds", "project": "shellomancy", "chains": []},
        "overall": {"total": 6, "completed": 2, "blocked": 1, "pct": 33},
        "next_actions": [{"chain": "a", "todo_id": "t9", "description": "Mana Bar"}]
    }]});

    assert_eq!(
        parse_quests(&reply),
        vec![Quest {
            id: "q1".to_owned(),
            name: "Bigger builds".to_owned(),
            project: "shellomancy".to_owned(),
            total: 6,
            completed: 2,
            pct: 33,
            next: Some(NextStep {
                todo_id: "t9".to_owned(),
                description: "Mana Bar".to_owned(),
            }),
        }]
    );
}

#[test]
fn quests_without_next_actions_have_no_next_step() {
    let reply = json!({"data": [{"quest": {"id": "q1"}, "overall": {}, "next_actions": []}]});

    assert_eq!(parse_quests(&reply)[0].next, None);
}

#[test]
fn lessons_parse_without_their_embeddings() {
    let reply = json!({"success": true, "data": [{
        "id": "l1", "topic": "Redact first", "lesson_learned": "Build titles after redacting.",
        "language": "rust", "tags": ["shellomancy", "spell"], "embedding": [0.1, 0.2]
    }]});

    assert_eq!(
        parse_lessons(&reply),
        vec![Lesson {
            id: "l1".to_owned(),
            topic: "Redact first".to_owned(),
            body: "Build titles after redacting.".to_owned(),
            tags: vec!["shellomancy".to_owned(), "spell".to_owned()],
            language: "rust".to_owned(),
            created_at: 0,
        }]
    );
}

#[test]
fn lesson_filter_matches_topic_body_and_tags_ignoring_case() {
    let lessons = vec![
        Lesson {
            id: "a".to_owned(),
            topic: "Keychain writes".to_owned(),
            body: String::new(),
            tags: vec![],
            language: "general".to_owned(),
            created_at: 0,
        },
        Lesson {
            id: "b".to_owned(),
            topic: "Other".to_owned(),
            body: "the KEYCHAIN again".to_owned(),
            tags: vec![],
            language: "general".to_owned(),
            created_at: 0,
        },
        Lesson {
            id: "c".to_owned(),
            topic: "Third".to_owned(),
            body: String::new(),
            tags: vec!["keychain".to_owned()],
            language: "general".to_owned(),
            created_at: 0,
        },
        Lesson {
            id: "d".to_owned(),
            topic: "Unrelated".to_owned(),
            body: String::new(),
            tags: vec![],
            language: "general".to_owned(),
            created_at: 0,
        },
    ];

    let ids: Vec<&str> = filter_lessons(&lessons, " keyChain ")
        .iter()
        .map(|lesson| lesson.id.as_str())
        .collect();

    assert_eq!(ids, vec!["a", "b", "c"]);
    assert_eq!(filter_lessons(&lessons, "").len(), 4);
}

#[test]
fn project_is_the_repo_folder_and_nothing_outside_a_repo() {
    assert_eq!(
        project_name(Some("Shellomancy")),
        Some("shellomancy".to_owned())
    );
    // The home folder (or any non-repo folder) isn't a project.
    assert_eq!(project_name(None), None);
    assert_eq!(project_name(Some(" ")), None);
}

#[test]
fn ages_are_short() {
    let now = 1_800_000_000;
    assert_eq!(age_label(0, now), "");
    assert_eq!(age_label(now - 10, now), "now");
    assert_eq!(age_label(now - 5 * 60, now), "5m");
    assert_eq!(age_label(now - 3 * 3600, now), "3h");
    assert_eq!(age_label(now - 27 * 86_400, now), "27d");
    assert_eq!(age_label(now - 240 * 86_400, now), "8mo");
    assert_eq!(age_label(now - 800 * 86_400, now), "2y");
    assert_eq!(age_label(now + 100, now), "now");
}

#[test]
fn urls_encode_the_project() {
    assert_eq!(
        todos_url(Some("my project&co")).as_str(),
        "https://madnessinteractive.cc/api/todos?exclude_completed=true&limit=200&project=my+project%26co"
    );
    assert_eq!(
        todos_url(None).as_str(),
        "https://madnessinteractive.cc/api/todos?exclude_completed=true&limit=300"
    );
    assert_eq!(
        quests_url(Some("shellomancy")).as_str(),
        "https://madnessinteractive.cc/api/quests/progress?status=active&limit=50&project=shellomancy"
    );
}

#[test]
fn complete_and_page_urls_put_the_id_in_the_path() {
    assert_eq!(
        complete_url("abc-123").as_str(),
        "https://madnessinteractive.cc/api/todos/abc-123/complete"
    );
    assert_eq!(
        todo_page_url("abc-123"),
        "https://madnessinteractive.cc/todo/abc-123"
    );
}

#[test]
fn every_mana_bar_string_is_in_the_voices() {
    for key in [
        "shellomancy.manaBar.title",
        "shellomancy.manaBar.signedOutTitle",
        "shellomancy.manaBar.todosHeading",
        "shellomancy.manaBar.lessonsHeading",
        Priority::Critical.voice_key(),
        Priority::High.voice_key(),
        Priority::Medium.voice_key(),
        Priority::Low.voice_key(),
    ] {
        assert_ne!(madness_themes::t(key), key, "{key} has no wording");
    }
}
