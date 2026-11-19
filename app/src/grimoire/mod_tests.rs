use chrono::TimeZone;

use super::*;

fn now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 11, 19, 14, 30, 5).unwrap()
}

fn write(dir: &Path, relative: &str, text: &str) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn a_shelf_reads_notebooks_incantations_and_scripts() {
    let shelf = tempfile::tempdir().unwrap();
    write(
        shelf.path(),
        "regression/balena-device-aliases.md",
        "Some notes first\n\n# Balena device aliases\n\n```sh\nbalena ssh x\n```\n",
    );
    write(
        shelf.path(),
        "regression/notes.md",
        "no heading in this one\n",
    );
    write(
        shelf.path(),
        "workflows.yaml",
        "name: Kill the port\ncommand: lsof -t -i:{{port}} | xargs kill\n---\n\
         name: Undo last commit\ncommand: git reset HEAD~\ndescription: keeps the changes\n",
    );
    write(
        shelf.path(),
        "purge.sh",
        "#!/usr/bin/env bash\necho purge\n",
    );
    write(shelf.path(), ".hidden/secret.md", "# nope\n");
    write(shelf.path(), ".DS_Store", "junk");
    std::fs::write(shelf.path().join("blob.bin"), [0u8, 1, 2, 3]).unwrap();

    let mut pages = read_shelf(shelf.path(), Shelf::Global);
    sort_pages(&mut pages);
    let titles: Vec<(&str, &str)> = pages
        .iter()
        .map(|page| (page.folder.as_str(), page.title.as_str()))
        .collect();
    assert_eq!(
        titles,
        [
            ("", "Kill the port"),
            ("", "purge"),
            ("", "Undo last commit"),
            ("regression", "Balena device aliases"),
            ("regression", "notes"),
        ]
    );

    let kill = &pages[0];
    let PageKind::Incantation(workflow) = &kill.kind else {
        panic!("expected an incantation, got {:?}", kill.kind);
    };
    // The undeclared {{port}} became an argument, so casting asks for it.
    assert_eq!(
        workflow
            .arguments()
            .iter()
            .map(|argument| argument.name())
            .collect::<Vec<_>>(),
        ["port"]
    );
    // Two incantations in one file: one path, different keys.
    assert_eq!(kill.path, pages[2].path);
    assert_ne!(kill.key(), pages[2].key());
    assert!(pages[2].text.contains("keeps the changes"));

    let PageKind::Script { runner } = &pages[1].kind else {
        panic!("expected a script");
    };
    assert!(runner.as_deref().unwrap().starts_with("bash "));
    assert_eq!(pages[3].kind, PageKind::Notebook);
}

#[test]
fn a_missing_shelf_is_empty() {
    let shelf = tempfile::tempdir().unwrap();
    assert!(read_shelf(&shelf.path().join("nope"), Shelf::Repo).is_empty());
}

#[test]
fn yaml_without_a_workflow_is_still_listed_to_fix() {
    let shelf = tempfile::tempdir().unwrap();
    write(shelf.path(), "broken.yaml", "name: [unclosed\n");
    let pages = read_shelf(shelf.path(), Shelf::Global);
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].kind, PageKind::Script { runner: None });
}

#[test]
fn a_document_that_is_not_a_workflow_is_skipped() {
    let names: Vec<String> = parse_incantations(
        "name: a\ncommand: echo a\n---\njust a string\n---\nname: b\ncommand: echo b\n",
    )
    .iter()
    .map(|workflow| workflow.name().to_owned())
    .collect();
    assert_eq!(names, ["a", "b"]);
}

#[test]
fn placeholders_are_found_in_order_once() {
    assert_eq!(
        placeholders("ssh {{host}} -p {{ port }} && echo {{host}} {{not valid}} {{"),
        ["host", "port"]
    );
    assert!(placeholders("echo plain").is_empty());
}

#[test]
fn declared_arguments_are_kept() {
    let workflow = Workflow::new("x", "echo {{a}} {{b}}").with_arguments(vec![
        Argument::new("a", ArgumentType::Text).with_default("1"),
    ]);
    let workflow = with_placeholder_arguments(workflow);
    let arguments = workflow.arguments();
    assert_eq!(arguments.len(), 2);
    assert_eq!(arguments[0].default_value().as_deref(), Some("1"));
    assert_eq!(arguments[1].name(), "b");
}

#[test]
fn runners_come_from_the_shebang_then_the_extension() {
    let path = Path::new("/tmp/grimoire/x.py");
    assert_eq!(
        script_runner(path, "#!/usr/bin/env -S python3 -u\n"),
        Some("python3 /tmp/grimoire/x.py".to_owned())
    );
    assert_eq!(
        script_runner(path, "#!/bin/zsh -e\n"),
        Some("zsh /tmp/grimoire/x.py".to_owned())
    );
    assert_eq!(
        script_runner(path, "print('hi')\n"),
        Some("python3 /tmp/grimoire/x.py".to_owned())
    );
    assert_eq!(script_runner(Path::new("/tmp/notes.txt"), "hi"), None);
    assert_eq!(
        script_runner(Path::new("/tmp/my notes/it's.sh"), ""),
        Some(r"bash '/tmp/my notes/it'\''s.sh'".to_owned())
    );
}

#[test]
fn search_needs_every_word_and_ranks_titles_first() {
    let page = |title: &str, text: &str| Page {
        path: PathBuf::from(format!("/g/{title}.md")),
        shelf: Shelf::Global,
        folder: String::new(),
        title: title.to_owned(),
        kind: PageKind::Notebook,
        text: text.to_owned(),
        modified: None,
    };
    let pages = vec![
        page("Regression desknote", "balena tunnel to the device"),
        page("Balena device aliases", "alias edens6=..."),
        page("Neovim", "nothing here"),
    ];
    assert_eq!(search(&pages, "balena"), [1, 0]);
    assert_eq!(search(&pages, "BALENA tunnel"), [0]);
    assert_eq!(search(&pages, "  "), [0, 1, 2]);
    assert!(search(&pages, "nowhere").is_empty());
    assert_eq!(
        hit_line(&pages[0], "tunnel").as_deref(),
        Some("balena tunnel to the device")
    );
    assert_eq!(hit_line(&pages[0], ""), None);
}

#[test]
fn repo_shelves_list_first_and_home_is_not_a_repo_shelf() {
    let repo = Path::new("/work/Omnispindle");
    let shelves = shelves(Some(repo));
    let global = global_dir().unwrap();
    assert_eq!(
        shelves,
        [
            (Shelf::Repo, repo.join(GRIMOIRE_DIR)),
            // Lowercased, the way `jot` and `ticket` name it from a shell.
            (Shelf::Private, global.join("omnispindle")),
            (Shelf::Global, global),
        ]
    );
    if let Some(home) = dirs::home_dir() {
        let at_home = super::shelves(Some(&home));
        assert_eq!(at_home.len(), 1);
        assert_eq!(at_home[0].0, Shelf::Global);
    }
}

#[test]
fn a_private_shelf_is_not_listed_again_in_the_global_one() {
    let global = tempfile::tempdir().unwrap();
    write(global.path(), "shellomancy/desknote.md", "# Desknote\n");
    write(global.path(), "eaws/triage.md", "# eaws triage\n");
    let pages = read_all(&[
        (Shelf::Private, global.path().join("shellomancy")),
        (Shelf::Global, global.path().to_path_buf()),
    ]);
    let listed: Vec<(Shelf, &str, &str)> = pages
        .iter()
        .map(|page| (page.shelf, page.folder.as_str(), page.title.as_str()))
        .collect();
    assert_eq!(
        listed,
        [
            (Shelf::Private, "", "Desknote"),
            (Shelf::Global, "eaws", "eaws triage"),
        ]
    );
}

#[test]
fn inscribing_writes_a_castable_incantation() {
    let shelf = tempfile::tempdir().unwrap();
    let command = "balena device ssh {{RTK_UUID}} --port 22222 --verbose --tty extra";
    let first = inscribe(shelf.path(), command, Some("/tmp/rtk"), now()).unwrap();
    let second = inscribe(shelf.path(), command, None, now()).unwrap();
    assert_eq!(first.parent().unwrap(), shelf.path().join("inscribed"));
    assert_ne!(first, second, "a second inscription gets its own file");
    assert!(
        second
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("-2.yaml")
    );

    let pages = read_shelf(shelf.path(), Shelf::Global);
    let page = pages.iter().find(|page| page.path == first).unwrap();
    let PageKind::Incantation(workflow) = &page.kind else {
        panic!("expected an incantation");
    };
    assert_eq!(workflow.content(), command);
    assert_eq!(
        workflow.name(),
        "balena device ssh {{RTK_UUID}} --port 22222…"
    );
    assert_eq!(
        workflow.description().map(String::as_str),
        Some("Inscribed from /tmp/rtk on 2026-11-19")
    );
    assert_eq!(workflow.arguments()[0].name(), "RTK_UUID");
}

#[test]
fn new_pages_parse_back() {
    let shelf = tempfile::tempdir().unwrap();
    let notebook = new_notebook(shelf.path(), now()).unwrap();
    let incantation = new_incantation(shelf.path(), now()).unwrap();
    assert!(notebook.ends_with("scratch-2026-11-19-143005.md"));
    let pages = read_shelf(shelf.path(), Shelf::Global);
    assert!(
        pages
            .iter()
            .any(|page| page.path == notebook && page.title == "Scratch, 2026-11-19 14:30")
    );
    let page = pages.iter().find(|page| page.path == incantation).unwrap();
    let PageKind::Incantation(workflow) = &page.kind else {
        panic!("the starter incantation should parse");
    };
    assert_eq!(workflow.arguments().len(), 1);
    assert_eq!(
        workflow.arguments()[0].default_value().as_deref(),
        Some("world")
    );
}

#[test]
fn slugs_and_names() {
    assert_eq!(slug("git push origin main"), "git-push-origin-main");
    assert_eq!(slug("  --!!  "), "spell");
    assert_eq!(slug(&"a ".repeat(100)).len(), MAX_SLUG_CHARS - 1);
    assert_eq!(spell_name("ls -la"), "ls -la");
    assert_eq!(spell_name("a b c d e f g"), "a b c d e f…");
}

#[test]
fn repo_root_is_the_nearest_git_folder() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join(".git")).unwrap();
    std::fs::create_dir_all(repo.path().join("app/src")).unwrap();
    assert_eq!(
        repo_root_of(&repo.path().join("app/src")),
        Some(repo.path().to_path_buf())
    );
}
