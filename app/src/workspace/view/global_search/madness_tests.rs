use chrono::TimeZone;

use super::*;

fn ran(command: &str, minute: u32, exit: i32, pwd: &str) -> HistoryEntry {
    let mut entry = HistoryEntry::command_only(command);
    entry.start_ts = Some(Local.with_ymd_and_hms(2026, 11, 18, 9, minute, 0).unwrap());
    entry.exit_code = Some(warp_core::command::ExitCode::from(exit));
    entry.pwd = Some(pwd.to_owned());
    entry
}

#[test]
fn history_merges_newest_first_without_repeats() {
    // A history-file line (no time), then two timed runs in one session.
    let file_line = HistoryEntry::command_only("ls -la");
    let early = ran("cargo test", 1, 101, "/a");
    let late = ran("cargo check", 5, 0, "/a");
    // Another session ran `cargo test` again, later and successfully.
    let again = ran("cargo test", 9, 0, "/b");
    let spells = merge_history([vec![&file_line, &early, &late], vec![&again]]);
    let commands: Vec<&str> = spells.iter().map(|s| s.command.as_str()).collect();
    assert_eq!(commands, ["cargo test", "cargo check", "ls -la"]);
    // The newest run of a command is the one kept.
    assert_eq!(spells[0].exit_code, Some(0));
    assert_eq!(spells[0].pwd.as_deref(), Some("/b"));
}

#[test]
fn untimed_lines_keep_their_history_order() {
    let lines = [
        HistoryEntry::command_only("first"),
        HistoryEntry::command_only("second"),
        HistoryEntry::command_only("  "),
        HistoryEntry::command_only("first"),
    ];
    let spells = merge_history([lines.iter().collect()]);
    let commands: Vec<&str> = spells.iter().map(|s| s.command.as_str()).collect();
    // The later `first` is the newer one; blank lines are dropped.
    assert_eq!(commands, ["first", "second"]);
}

#[test]
fn every_word_must_match_ignoring_case() {
    let spells: Vec<SpellHit> = ["git push origin main", "cargo test -p warp", "Git Status"]
        .iter()
        .map(|command| SpellHit {
            command: command.to_string(),
            ..Default::default()
        })
        .collect();
    assert_eq!(matching_spells(&spells, "git", 10), [0, 2]);
    assert_eq!(matching_spells(&spells, "GIT main", 10), [0]);
    assert_eq!(matching_spells(&spells, "  ", 10), [0, 1, 2]);
    assert_eq!(matching_spells(&spells, "", 2), [0, 1]);
    assert!(matching_spells(&spells, "npm", 10).is_empty());
}

#[test]
fn lesson_search_escapes_the_query_for_the_servers_regex() {
    let blank = lessons_search_url("  ");
    assert!(blank.ends_with("/lessons?limit=50"), "{blank}");
    let url = lessons_search_url("a.b (c)");
    assert!(url.contains("&search=a%5C.b%20%5C%28c%5C%29"), "{url}");
}

#[test]
fn snippets_are_one_line_and_cut() {
    assert_eq!(snippet("line one\n\n  line two "), "line one line two");
    let long = "word ".repeat(100);
    let cut = snippet(&long);
    assert_eq!(cut.chars().count(), SNIPPET_CHARS + 1);
    assert!(cut.ends_with('…'));
}

#[test]
fn home_paths_get_a_tilde() {
    let home = dirs::home_dir().unwrap();
    let home = home.to_str().unwrap();
    assert_eq!(tilde(home), "~");
    assert_eq!(tilde(&format!("{home}/lab/x")), "~/lab/x");
    assert_eq!(tilde("/etc/hosts"), "/etc/hosts");
    assert_eq!(tilde(&format!("{home}x/y")), format!("{home}x/y"));
}

#[test]
fn scopes_have_wording_keys() {
    for scope in SearchScope::ALL {
        assert!(scope.voice_key().starts_with("shellomancy.search."));
        assert!(scope.placeholder_key().starts_with("shellomancy.search."));
    }
}
