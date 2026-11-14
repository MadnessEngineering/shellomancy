use super::*;

fn spell(command: &str, exit_code: Option<i32>) -> Spell {
    Spell {
        command: command.to_owned(),
        output: "line one\nline two".to_owned(),
        cwd: Some("/nowhere/shellomancy-test/app".to_owned()),
        git_branch: Some("main".to_owned()),
        exit_code,
    }
}

#[test]
fn failed_spells_become_high_priority_failures() {
    let body = todo_body(&spell("cargo test -p warp", Some(101)));
    assert_eq!(
        body["description"],
        "`cargo test -p warp` failed (exit 101)"
    );
    assert_eq!(body["priority"], "High");
    assert_eq!(body["tags"], json!(["shellomancy", "spell", "failed"]));
    assert_eq!(body["metadata"]["exit_code"], 101);
    assert_eq!(body["metadata"]["output_tail"], "line one\nline two");
    assert_eq!(body["metadata"]["git_branch"], "main");
}

#[test]
fn successful_spells_become_follow_ups() {
    let body = todo_body(&spell("ls", Some(0)));
    assert_eq!(body["description"], "Follow up on `ls`");
    assert_eq!(body["priority"], "Medium");
    assert_eq!(body["tags"][2], "follow-up");
}

#[test]
fn long_and_multiline_commands_are_shortened_in_the_description() {
    let long = format!("echo {}\nsecond line", "x".repeat(300));
    let body = todo_body(&spell(&long, None));
    let description = body["description"].as_str().unwrap();
    assert!(!description.contains("second line"));
    assert!(description.chars().count() < 140, "{description}");
    // The whole command is still kept in the metadata.
    assert!(
        body["metadata"]["command"]
            .as_str()
            .unwrap()
            .contains("second line")
    );
}

#[test]
fn only_the_tail_of_long_output_is_sent() {
    let mut s = spell("make", Some(2));
    s.output = format!("{}END", "a".repeat(OUTPUT_TAIL_CHARS * 2));
    let body = todo_body(&s);
    let output = body["metadata"]["output_tail"].as_str().unwrap();
    assert!(output.ends_with("END"));
    assert_eq!(output.chars().count(), OUTPUT_TAIL_CHARS + 1);
}

#[test]
fn secrets_are_redacted_from_command_and_output() {
    let key = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let mut s = spell(&format!("export GITHUB_TOKEN={key}"), Some(0));
    s.output = format!("token is {key}");
    let body = todo_body(&s);
    assert!(!body.to_string().contains(key), "{body}");
}

#[test]
fn project_is_the_lowercase_folder_name_without_a_repo() {
    assert_eq!(project_for(Some("/nowhere/My-Project")), "my-project");
    assert_eq!(project_for(None), "shellomancy");
    assert_eq!(project_for(Some("/")), "shellomancy");
}

#[test]
fn project_is_the_repo_root_inside_a_repo() {
    // This test file lives in the shellomancy repo.
    let here = env!("CARGO_MANIFEST_DIR");
    let src = Path::new(here).join("src");
    assert_eq!(project_for(src.to_str()), "shellomancy");
}

#[test]
fn replies_are_read() {
    let body = todo_body(&spell("ls", Some(0)));
    let created = created_from_reply(
        201,
        &json!({"todo_id": "abc", "success": true, "todo": {"id": "abc"}}),
        &body,
    )
    .unwrap();
    assert_eq!(created.id, "abc");
    assert_eq!(created.description, "Follow up on `ls`");

    let err = created_from_reply(403, &json!({"message": "limit reached"}), &body).unwrap_err();
    assert!(err.contains("limit reached (HTTP 403)"), "{err}");
    let err = created_from_reply(500, &Value::Null, &body).unwrap_err();
    assert!(err.contains("HTTP 500"), "{err}");
}
