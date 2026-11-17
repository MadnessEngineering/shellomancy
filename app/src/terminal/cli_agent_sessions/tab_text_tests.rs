use super::*;

fn touch(action: &str, todo_id: Option<&str>, status: Option<&str>) -> TodoTouch {
    TodoTouch {
        action: action.to_owned(),
        todo_id: todo_id.map(str::to_owned),
        description: None,
        status: status.map(str::to_owned),
        result_id: None,
    }
}

fn active(id: &str, description: Option<&str>, done: bool) -> Option<ActiveTodo> {
    Some(ActiveTodo {
        id: id.to_owned(),
        description: description.map(str::to_owned),
        done,
    })
}

#[test]
fn session_names_lose_spinners_and_uuids() {
    assert_eq!(
        clean_session_name("✳ be5acd31-f35f-4739-a1a3-ab73f0328b31 · Cipher"),
        Some("Cipher".to_owned())
    );
    assert_eq!(
        clean_session_name("✳ eaf21753-a3ec-45e7-816c-0f9955840a23"),
        None
    );
    assert_eq!(
        clean_session_name("✶ Fly over mcp"),
        Some("Fly over mcp".to_owned())
    );
    assert_eq!(
        clean_session_name("⠋ Release Watcher"),
        Some("Release Watcher".to_owned())
    );
    // Claude Code's generic default title says nothing about the session.
    assert_eq!(clean_session_name("✳ Claude Code"), None);
    assert_eq!(clean_session_name(""), None);
    // Hex that isn't UUID-shaped is a name, not an id.
    assert_eq!(
        clean_session_name("deadbeef work"),
        Some("deadbeef work".to_owned())
    );
}

#[test]
fn uuids_are_recognized_only_in_full() {
    assert!(is_uuid("3d94c3fc-f4b9-4b0a-9388-e20d92d55501"));
    assert!(!is_uuid("3d94c3fc"));
    assert!(!is_uuid("3d94c3fc-f4b9-4b0a-9388-e20d92d5550z"));
}

#[test]
fn add_todo_becomes_the_work_unless_another_is_open() {
    let mut add = touch("add_todo", None, None);
    add.result_id = Some("new".to_owned());
    add.description = Some("Agent tab titles".to_owned());
    assert_eq!(
        apply_todo_touch(None, &add),
        active("new", Some("Agent tab titles"), false)
    );
    // A side quest added mid-work doesn't steal the tab.
    let working = active("main", Some("Main work"), false);
    assert_eq!(apply_todo_touch(working.clone(), &add), working);
    // Once the main work is done, the next added todo takes over.
    assert_eq!(
        apply_todo_touch(active("main", None, true), &add),
        active("new", Some("Agent tab titles"), false)
    );
}

#[test]
fn update_and_complete_follow_the_status() {
    let start = touch("update_todo", Some("t1"), Some("in_progress"));
    assert_eq!(apply_todo_touch(None, &start), active("t1", None, false));
    // in_progress on another todo switches the work to it.
    assert_eq!(
        apply_todo_touch(active("t0", Some("old"), false), &start),
        active("t1", None, false)
    );

    let review = touch("update_todo", Some("t1"), Some("review"));
    assert_eq!(
        apply_todo_touch(active("t1", Some("Work"), false), &review),
        active("t1", Some("Work"), true)
    );
    // Finishing some other todo leaves the work alone.
    assert_eq!(
        apply_todo_touch(active("t2", Some("Mine"), false), &review),
        active("t2", Some("Mine"), false)
    );

    let complete = touch("complete_todo", Some("t1"), None);
    assert_eq!(
        apply_todo_touch(active("t1", Some("Work"), false), &complete),
        active("t1", Some("Work"), true)
    );

    let checkpoint = TodoTouch {
        description: Some("Better words".to_owned()),
        ..touch("update_todo", Some("t1"), None)
    };
    assert_eq!(
        apply_todo_touch(active("t1", Some("Work"), false), &checkpoint),
        active("t1", Some("Better words"), false)
    );

    // Reads and searches don't change the work.
    let search = touch("search_todos", None, None);
    assert_eq!(
        apply_todo_touch(active("t1", None, false), &search),
        active("t1", None, false)
    );
}

#[test]
fn work_prefers_the_todo_then_the_prompt() {
    let todo = ActiveTodo {
        id: "3d94c3fc-f4b9-4b0a-9388-e20d92d55501".to_owned(),
        description: Some("Agent tab titles".to_owned()),
        done: false,
    };
    assert_eq!(
        work_text(Some(&todo), Some("prompt")),
        Some("Agent tab titles".to_owned())
    );
    let done = ActiveTodo {
        done: true,
        ..todo.clone()
    };
    assert_eq!(
        work_text(Some(&done), None),
        Some("✓ Agent tab titles".to_owned())
    );
    let unnamed = ActiveTodo {
        description: None,
        ..todo
    };
    assert!(
        work_text(Some(&unnamed), None)
            .unwrap()
            .contains("3d94c3fc")
    );

    let long = format!("{}\nsecond line", "word ".repeat(30));
    let work = work_text(None, Some(&long)).unwrap();
    assert!(work.ends_with('…'), "{work}");
    assert!(work.chars().count() <= WORK_CHARS + 1);
    assert!(!work.contains("second line"));
    assert_eq!(work_text(None, Some("   ")), None);
    assert_eq!(work_text(None, None), None);
}

#[test]
fn titles_join_name_and_work() {
    assert_eq!(
        agent_tab_title("Cipher", Some("Agent tabs")),
        "Cipher · Agent tabs"
    );
    assert_eq!(agent_tab_title("Cipher", None), "Cipher");
}

fn tool(name: &str, target: Option<&str>, running: bool) -> ToolActivity {
    ToolActivity {
        name: name.to_owned(),
        target: target.map(str::to_owned),
        running,
    }
}

#[test]
fn activity_follows_status_then_the_running_tool() {
    let in_progress = CLIAgentSessionStatus::InProgress;
    assert_eq!(
        agent_activity(
            &in_progress,
            Some(&tool("Bash", Some("cargo test -p warp"), true))
        ),
        AgentActivity::Running("cargo test -p warp".to_owned())
    );
    assert_eq!(
        agent_activity(
            &in_progress,
            Some(&tool("Edit", Some("/a/b/mana_bar_view.rs"), true))
        ),
        AgentActivity::Editing("mana_bar_view.rs".to_owned())
    );
    assert_eq!(
        agent_activity(
            &in_progress,
            Some(&tool("mcp__omnispindle__update_todo", None, true))
        ),
        AgentActivity::Omnispindle
    );
    assert_eq!(
        agent_activity(
            &in_progress,
            Some(&tool("mcp__context7__query-docs", None, true))
        ),
        AgentActivity::Calling("context7".to_owned())
    );
    assert_eq!(
        agent_activity(&in_progress, Some(&tool("Grep", None, true))),
        AgentActivity::Searching
    );
    // A finished tool means it's thinking about the next step.
    assert_eq!(
        agent_activity(&in_progress, Some(&tool("Bash", Some("ls"), false))),
        AgentActivity::Thinking
    );
    assert_eq!(agent_activity(&in_progress, None), AgentActivity::Thinking);

    let permission = CLIAgentSessionStatus::Blocked {
        message: None,
        source: BlockedSource::PermissionRequest,
    };
    assert_eq!(
        agent_activity(&permission, Some(&tool("Bash", Some("rm"), true))),
        AgentActivity::NeedsPermission
    );
    let question = CLIAgentSessionStatus::Blocked {
        message: None,
        source: BlockedSource::QuestionAsked,
    };
    assert_eq!(
        agent_activity(&question, None),
        AgentActivity::WaitingForYou
    );
    assert_eq!(
        agent_activity(&CLIAgentSessionStatus::Success, None),
        AgentActivity::Done
    );
    assert_eq!(
        agent_activity(&CLIAgentSessionStatus::Cancelled, None),
        AgentActivity::Cancelled
    );
}

#[test]
fn long_commands_are_cut_in_the_activity_line() {
    let command = "cargo test -p warp --lib -- mana_bar madness_sync settings_view workspace";
    match agent_activity(
        &CLIAgentSessionStatus::InProgress,
        Some(&tool("Bash", Some(command), true)),
    ) {
        AgentActivity::Running(shown) => {
            assert!(shown.ends_with('…'), "{shown}");
            assert!(shown.chars().count() <= COMMAND_CHARS + 1);
        }
        other => panic!("expected Running, got {other:?}"),
    }
}

#[test]
fn every_activity_has_words() {
    for activity in [
        AgentActivity::Running("ls".to_owned()),
        AgentActivity::Editing("a.rs".to_owned()),
        AgentActivity::Reading("a.rs".to_owned()),
        AgentActivity::Searching,
        AgentActivity::Browsing,
        AgentActivity::Delegating,
        AgentActivity::Omnispindle,
        AgentActivity::Calling("x".to_owned()),
        AgentActivity::Using("x".to_owned()),
        AgentActivity::Thinking,
        AgentActivity::NeedsPermission,
        AgentActivity::WaitingForYou,
        AgentActivity::Done,
        AgentActivity::Failed,
        AgentActivity::Cancelled,
    ] {
        let text = activity_text(&activity);
        assert!(
            !text.starts_with("shellomancy."),
            "{activity:?} has no voice key: {text}"
        );
    }
}
