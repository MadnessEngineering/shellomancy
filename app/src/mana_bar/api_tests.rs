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
        ..Default::default()
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
            // The rest of the metadata, for the expanded card.
            extra: vec![("quest_id".to_owned(), "q".to_owned())],
            ..Default::default()
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
            blocked: 1,
            next: Some(NextStep {
                todo_id: "t9".to_owned(),
                description: "Mana Bar".to_owned(),
            }),
            next_ids: vec!["t9".to_owned()],
            ..Default::default()
        }]
    );
}

#[test]
fn quest_chains_parse_with_their_todos_in_order() {
    let reply = json!({"data": [{
        "quest": {"id": "q1"},
        "overall": {"total": 3, "completed": 1, "blocked": 1, "pct": 33},
        "chains": [
            {"label": "Foundation", "status": "blocked", "total": 3, "completed": 1, "blocked": 1,
             "items": [
                {"id": "t1", "status": "review", "description": "Sign in", "project": "shellomancy"},
                {"id": "t2", "status": "blocked", "description": "Sync", "project": "inventorium"},
                {"id": "t3", "status": "pending", "description": null, "project": null, "missing": true},
                {"status": "pending"}
             ]},
            {"label": "Later", "status": "pending", "total": 0, "completed": 0, "blocked": 0}
        ],
        "next_actions": [{"chain": "Foundation", "todo_id": "t3", "description": null}]
    }]});
    let quest = &parse_quests(&reply)[0];

    assert_eq!(quest.next_ids, vec!["t3".to_owned()]);
    assert_eq!(
        quest.chains,
        vec![
            QuestChain {
                label: "Foundation".to_owned(),
                status: "blocked".to_owned(),
                total: 3,
                completed: 1,
                blocked: 1,
                todos: vec![
                    QuestTodo {
                        id: "t1".to_owned(),
                        status: "review".to_owned(),
                        description: "Sign in".to_owned(),
                        project: "shellomancy".to_owned(),
                        missing: false,
                    },
                    QuestTodo {
                        id: "t2".to_owned(),
                        status: "blocked".to_owned(),
                        description: "Sync".to_owned(),
                        project: "inventorium".to_owned(),
                        missing: false,
                    },
                    QuestTodo {
                        id: "t3".to_owned(),
                        status: "pending".to_owned(),
                        missing: true,
                        ..Default::default()
                    },
                ],
            },
            QuestChain {
                label: "Later".to_owned(),
                status: "pending".to_owned(),
                ..Default::default()
            },
        ]
    );
    assert!(quest.chains[0].todos[0].is_done());
    assert!(!quest.chains[0].todos[1].is_done());
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
        "https://madnessinteractive.cc/api/quests/progress?status=active&limit=50&todos=1&project=shellomancy"
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

#[test]
fn activity_entries_parse_leniently() {
    let reply = json!({"logs": [
        {"todoId": "t1", "operation": "complete", "todoTitle": "Ship it",
         "description": "Ship it, the long way", "project": "shellomancy",
         "timestamp": "2026-11-15T12:00:00.000Z", "changes": [],
         "completion_comment": "Landed in ce428e5e"},
        {"todo_id": "t2", "description": "Edit me", "project": "inventorium",
         "timestamp": 1_794_744_000_000_i64,
         "changes": [{"field": "status"}, {"field": "status"}, {"field": "priority"}]},
        {"operation": "create", "description": "no todo id"}
    ], "total": 3});

    let entries = parse_activity(&reply);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].operation, "complete");
    assert_eq!(entries[0].description, "Ship it");
    assert_eq!(entries[0].at, 1_794_744_000);
    assert_eq!(entries[0].comment.as_deref(), Some("Landed in ce428e5e"));
    // Milliseconds become seconds; repeated fields collapse; no operation means update.
    assert_eq!(entries[1].operation, "update");
    assert_eq!(entries[1].at, 1_794_744_000);
    assert_eq!(
        entries[1].fields,
        vec!["status".to_owned(), "priority".to_owned()]
    );
    assert_eq!(entries[1].comment, None);
}

#[test]
fn activity_url_filters_by_project() {
    let url = activity_url(Some("foo bar"));
    assert_eq!(url.path(), "/api/todos/activity");
    assert!(url.query().unwrap().contains("project=foo+bar"));
    assert!(!activity_url(None).query().unwrap().contains("project"));
}

#[test]
fn sort_todos_matches_the_grouped_order_without_copying() {
    let todos = vec![
        todo("low", Priority::Low, "pending", 5),
        todo("crit-old", Priority::Critical, "pending", 1),
        todo("crit-new", Priority::Critical, "pending", 9),
        todo("crit-doing", Priority::Critical, "in_progress", 2),
        todo("high", Priority::High, "pending", 3),
    ];
    let sorted: Vec<&str> = sort_todos(&todos)
        .iter()
        .map(|todo| todo.id.as_str())
        .collect();
    let grouped: Vec<String> = group_by_priority(todos.clone())
        .into_iter()
        .flat_map(|(_, group)| group.into_iter().map(|todo| todo.id))
        .collect();
    assert_eq!(sorted, grouped);
    assert_eq!(
        sorted,
        ["crit-doing", "crit-new", "crit-old", "high", "low"]
    );
}

#[test]
fn todo_details_parse_for_the_expanded_card() {
    let reply = json!({"todos": [{
        "id": "t1", "description": "d", "status": "pending",
        "created_at": 1763000000, "updated_at": "2026-11-15T12:00:00.000Z",
        "notes": "  see the log  ", "target_agent": "claude", "ticket": "",
        "metadata": {"tags": ["x"], "quest_id": "q-1", "context": null,
                     "files": ["a.rs", "b.rs"], "landed": {"sha": "abc"}, "empty": {}}
    }]});
    let todo = &parse_todos(&reply)[0];
    assert_eq!(todo.updated_at, 1_794_744_000);
    assert_eq!(todo.notes.as_deref(), Some("see the log"));
    assert_eq!(todo.target_agent.as_deref(), Some("claude"));
    assert_eq!(todo.ticket, None);
    // Sorted by key; tags, nulls and empty objects are left out.
    assert_eq!(
        todo.extra,
        vec![
            ("files".to_owned(), "a.rs, b.rs".to_owned()),
            ("landed".to_owned(), r#"{"sha":"abc"}"#.to_owned()),
            ("quest_id".to_owned(), "q-1".to_owned()),
        ]
    );
}

#[test]
fn quest_description_and_status_parse() {
    let reply = json!({"data": [{
        "quest": {"id": "q1", "description": "Make it ours", "status": "active"},
        "overall": {}, "next_actions": []
    }]});
    let quest = &parse_quests(&reply)[0];
    assert_eq!(quest.description.as_deref(), Some("Make it ours"));
    assert_eq!(quest.status, "active");
}

#[test]
fn activity_changes_keep_old_and_new_values_and_the_actor() {
    let reply = json!({"logs": [{
        "todoId": "t1", "operation": "update", "timestamp": 1_794_744_000,
        "userAgent": "Omnispindle/1.4",
        "changes": [
            {"field": "status", "old_value": "pending", "new_value": "in_progress"},
            {"field": "notes", "old_value": null, "new_value": "first note"},
            {"field": "priority", "old_value": 3}
        ]
    }]});
    let entry = &parse_activity(&reply)[0];
    assert_eq!(entry.actor.as_deref(), Some("Omnispindle/1.4"));
    assert_eq!(
        entry.changes,
        vec![
            Change {
                field: "status".to_owned(),
                old: Some("pending".to_owned()),
                new: Some("in_progress".to_owned()),
            },
            Change {
                field: "notes".to_owned(),
                old: None,
                new: Some("first note".to_owned()),
            },
            Change {
                field: "priority".to_owned(),
                old: Some("3".to_owned()),
                new: None,
            },
        ]
    );
    assert_eq!(entry.fields, vec!["status", "notes", "priority"]);
}

#[test]
fn submodule_paths_become_projects() {
    let gitmodules = "[submodule \"projects/common/Inventorium\"]\n\tpath = projects/common/Inventorium\n\turl = git@github.com:x/Inventorium.git\n[submodule \"Omnispindle\"]\n\tpath = projects/common/Omnispindle/\n[submodule \"dup\"]\n\tpath = other/inventorium\n";
    assert_eq!(
        submodule_projects(gitmodules),
        vec!["inventorium".to_owned(), "omnispindle".to_owned()]
    );
    assert!(submodule_projects("").is_empty());
}

#[test]
fn a_repo_with_submodules_scopes_to_all_of_them() {
    let subs = vec!["inventorium".to_owned(), "madness_interactive".to_owned()];
    let scope = project_scope("madness_interactive", &subs);
    assert_eq!(scope, "madness_interactive,inventorium");
    assert_eq!(project_label(&scope), "madness_interactive +1");
    assert_eq!(project_scope("solo", &[]), "solo");
    assert_eq!(project_label("solo"), "solo");
    // The list goes to Inventorium as one comma-separated value.
    assert!(
        todos_url(Some(&scope))
            .query()
            .unwrap()
            .contains("project=madness_interactive%2Cinventorium")
    );
}

#[test]
fn project_for_path_finds_the_nearest_repo() {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert_eq!(project_for_path(&here), Some("shellomancy".to_owned()));
    assert_eq!(project_for_path(std::path::Path::new("/")), None);
}

#[test]
fn a_todo_marked_done_counts_in_every_quest_that_lists_it() {
    let chain = |todos: Vec<QuestTodo>| QuestChain {
        label: "main".to_owned(),
        total: todos.len() as u64,
        todos,
        ..Default::default()
    };
    let todo = |id: &str, status: &str| QuestTodo {
        id: id.to_owned(),
        status: status.to_owned(),
        ..Default::default()
    };
    let mut quests = vec![
        Quest {
            id: "q1".to_owned(),
            total: 3,
            completed: 1,
            pct: 33,
            blocked: 1,
            next: Some(NextStep {
                todo_id: "t2".to_owned(),
                description: "Sync".to_owned(),
            }),
            next_ids: vec!["t2".to_owned()],
            chains: vec![chain(vec![
                todo("t1", "completed"),
                todo("t2", "blocked"),
                todo("t3", "pending"),
            ])],
            ..Default::default()
        },
        Quest {
            id: "q2".to_owned(),
            total: 1,
            chains: vec![chain(vec![todo("t9", "pending")])],
            ..Default::default()
        },
    ];

    quest_todo_done(&mut quests, "t2");
    // Done already: nothing moves.
    quest_todo_done(&mut quests, "t1");

    let quest = &quests[0];
    assert_eq!((quest.completed, quest.pct, quest.blocked), (2, 66, 0));
    assert_eq!(quest.chains[0].todos[1].status, "review");
    assert_eq!(quest.chains[0].completed, 1);
    assert_eq!(quest.next, None);
    assert!(quest.next_ids.is_empty());
    assert_eq!(quests[1].completed, 0);
}
