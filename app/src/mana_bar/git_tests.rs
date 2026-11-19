use super::*;

#[test]
fn log_output_parses_into_commits() {
    let output = "f7f47999aa\u{1f}TheMadTinker\u{1f}1795056820\u{1f}Mana Bar: open targets\u{1f}A new setting.\n\nVerified.\u{1e}\n\
                  c2831d99bb\u{1f}TheMadTinker\u{1f}1795040000\u{1f}Search panel\u{1f}\u{1e}\n";
    assert_eq!(
        parse_log(output),
        vec![
            Commit {
                sha: "f7f47999aa".to_owned(),
                author: "TheMadTinker".to_owned(),
                at: 1_795_056_820,
                subject: "Mana Bar: open targets".to_owned(),
                body: "A new setting.\n\nVerified.".to_owned(),
            },
            Commit {
                sha: "c2831d99bb".to_owned(),
                author: "TheMadTinker".to_owned(),
                at: 1_795_040_000,
                subject: "Search panel".to_owned(),
                body: String::new(),
            },
        ]
    );
    assert!(parse_log("").is_empty());
    assert!(parse_log("fatal: not a git repository").is_empty());
}

#[test]
fn github_remotes_become_web_addresses() {
    let web = Some("https://github.com/MadnessEngineering/shellomancy".to_owned());
    assert_eq!(
        github_web("git@github.com:MadnessEngineering/shellomancy.git"),
        web
    );
    assert_eq!(
        github_web("https://github.com/MadnessEngineering/shellomancy\n"),
        web
    );
    assert_eq!(
        github_web("ssh://git@github.com/MadnessEngineering/shellomancy.git"),
        web
    );
    assert_eq!(github_web("git@gitlab.com:a/b.git"), None);
    assert_eq!(github_web("https://github.com/only-owner"), None);
}

#[test]
fn notes_name_commits_by_short_sha() {
    let sha = "f7f47999aa00bb";
    assert!(mentions("Landed: shellomancy f7f47999 (pushed)", sha));
    assert!(mentions("commit F7F4799.", sha));
    // Too short, not a prefix, or inside a longer word.
    assert!(!mentions("f7f479", sha));
    assert!(!mentions("Landed c2831d99", sha));
    assert!(!mentions("xf7f47999", sha));
}

#[test]
fn short_is_eight_characters() {
    let commit = Commit {
        sha: "f7f47999aa00".to_owned(),
        ..Default::default()
    };
    assert_eq!(commit.short(), "f7f47999");
}
