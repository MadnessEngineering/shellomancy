//! Shellomancy: the Grimoire, Drive's notebooks and workflows reborn as plain
//! files you own.
//!
//! A shelf is a folder of pages. In a repo there are two of its own: the
//! committed `<repo>/.mad/grimoire`, which travels with the repo, and the
//! private `~/.mad/grimoire/<repo>` (folder name lowercased), which follows
//! it on this machine only. Then the global `~/.mad/grimoire`: this machine
//! only, never pushed, so a page can hold a password. A repo's private shelf
//! isn't listed again as a folder of the global one. Pages are read from disk
//! on every scan; nothing is cached or synced.
//!
//! - `.md` is a notebook: it opens in the Markdown viewer, code blocks runnable.
//! - `.yaml` / `.yml` holds incantations: Warp's workflow format, one per YAML
//!   document. Casting one fills in its `{{arguments}}` in the input.
//! - Any other text file is a script: casting it puts the command that runs
//!   it in the input.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use serde::Deserialize;
use walkdir::WalkDir;

use crate::workflows::workflow::{Argument, ArgumentType, Workflow};

/// Where a shelf lives, under the home folder or a repo's root.
pub const GRIMOIRE_DIR: &str = ".mad/grimoire";
/// Bigger files aren't scratch pages; they're skipped.
const MAX_PAGE_BYTES: u64 = 512 * 1024;
/// Pages read per shelf, so a stray huge folder can't stall a scan.
const MAX_PAGES: usize = 1000;
const MAX_DEPTH: usize = 8;
/// Documents read per YAML file, a guard on serde_yaml's document iterator.
const MAX_YAML_DOCUMENTS: usize = 256;
/// Where inscribed spells go, under a shelf.
const INSCRIBED_FOLDER: &str = "inscribed";
const MAX_SLUG_CHARS: usize = 48;
const MAX_NAME_WORDS: usize = 6;
const MAX_NAME_CHARS: usize = 60;
const MAX_HIT_LINE_CHARS: usize = 120;

/// Which shelf a page is on, in listing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Shelf {
    /// `<repo>/.mad/grimoire`, committed with the repo.
    Repo,
    /// `~/.mad/grimoire/<repo>`, the repo's pages on this machine only.
    Private,
    /// `~/.mad/grimoire`.
    Global,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PageKind {
    /// Markdown: read in the Markdown viewer, its code blocks runnable.
    Notebook,
    /// One workflow from a YAML file.
    Incantation(Workflow),
    /// Any other text file. `runner` is the command line that runs it, when
    /// we can tell (an extension or shebang we know).
    Script { runner: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub path: PathBuf,
    pub shelf: Shelf,
    /// The folder under the shelf (`ops/devices`), empty at its top.
    pub folder: String,
    pub title: String,
    pub kind: PageKind,
    /// What search reads: a notebook's or script's text, an incantation's
    /// command and description.
    pub text: String,
    /// Unix seconds.
    pub modified: Option<i64>,
}

impl Page {
    /// Unique per page: a YAML file can hold several incantations.
    pub fn key(&self) -> String {
        match &self.kind {
            PageKind::Incantation(workflow) => {
                format!("{}#{}", self.path.display(), workflow.name())
            }
            _ => self.path.display().to_string(),
        }
    }

    pub fn is_incantation(&self) -> bool {
        matches!(self.kind, PageKind::Incantation(_))
    }
}

/// `~/.mad/grimoire`.
pub fn global_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(GRIMOIRE_DIR))
}

/// `<repo>/.mad/grimoire`.
pub fn repo_dir(root: &Path) -> PathBuf {
    root.join(GRIMOIRE_DIR)
}

/// `~/.mad/grimoire/<repo folder name, lowercased>`. The `jot` and `ticket`
/// incantations name it the same way from a shell.
pub fn private_dir(root: &Path) -> Option<PathBuf> {
    let name = root.file_name()?.to_string_lossy().to_lowercase();
    Some(global_dir()?.join(name))
}

/// The repo a folder is in: the nearest folder at or above it with a `.git`.
pub fn repo_root_of(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

/// The shelves to read for a terminal in `repo` (if any): the repo's two,
/// then the global one. A repo at the home folder has none of its own.
pub fn shelves(repo: Option<&Path>) -> Vec<(Shelf, PathBuf)> {
    let global = global_dir();
    let mut shelves = Vec::new();
    if let Some(repo) = repo {
        let dir = repo_dir(repo);
        if Some(&dir) != global.as_ref() {
            shelves.push((Shelf::Repo, dir));
            if let Some(private) = private_dir(repo) {
                shelves.push((Shelf::Private, private));
            }
        }
    }
    if let Some(global) = global {
        shelves.push((Shelf::Global, global));
    }
    shelves
}

/// Reads every shelf, in listing order, off the main thread.
pub async fn read_shelves(shelves: Vec<(Shelf, PathBuf)>) -> Vec<Page> {
    read_all(&shelves)
}

/// Reads every shelf, in listing order. Missing shelves are just empty, and a
/// shelf inside another (a repo's private one in the global one) is only read
/// as itself.
pub fn read_all(shelves: &[(Shelf, PathBuf)]) -> Vec<Page> {
    let mut pages: Vec<Page> = shelves
        .iter()
        .flat_map(|(shelf, dir)| {
            let nested: Vec<&Path> = shelves
                .iter()
                .map(|(_, other)| other.as_path())
                .filter(|other| *other != dir.as_path() && other.starts_with(dir))
                .collect();
            read_shelf_without(dir, *shelf, &nested)
        })
        .collect();
    sort_pages(&mut pages);
    pages
}

/// Repo pages first, then by folder and title.
pub fn sort_pages(pages: &mut [Page]) {
    pages.sort_by(|a, b| {
        (a.shelf, a.folder.to_lowercase(), a.title.to_lowercase()).cmp(&(
            b.shelf,
            b.folder.to_lowercase(),
            b.title.to_lowercase(),
        ))
    });
}

/// Every page on one shelf. Hidden files and folders are skipped.
pub fn read_shelf(dir: &Path, shelf: Shelf) -> Vec<Page> {
    read_shelf_without(dir, shelf, &[])
}

/// Every page on one shelf except under the `skip` folders.
fn read_shelf_without(dir: &Path, shelf: Shelf, skip: &[&Path]) -> Vec<Page> {
    let mut pages = Vec::new();
    let walker = WalkDir::new(dir)
        .max_depth(MAX_DEPTH)
        .follow_links(true)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !(is_hidden(entry.file_name())
                    || skip.iter().any(|skip| same_path(entry.path(), skip)))
        });
    for entry in walker.filter_map(Result::ok) {
        if pages.len() >= MAX_PAGES {
            break;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.len() > MAX_PAGE_BYTES {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            // Not UTF-8: not a page.
            continue;
        };
        if text.contains('\0') {
            continue;
        }
        let modified = metadata
            .modified()
            .ok()
            .map(|at| DateTime::<Local>::from(at).timestamp());
        let folder = entry
            .path()
            .parent()
            .and_then(|parent| parent.strip_prefix(dir).ok())
            .map(|folder| folder.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        pages.extend(pages_in_file(entry.path(), &text, shelf, folder, modified));
    }
    pages
}

fn is_hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// Whether two paths name the same folder. macOS and Windows file systems
/// ignore case by default, so `Omnispindle` there is the `omnispindle` shelf.
fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// The pages one file holds: a notebook, a script, or the incantations in a
/// YAML file. A YAML file with none that parse is listed as a script, so it
/// can still be opened and fixed.
pub fn pages_in_file(
    path: &Path,
    text: &str,
    shelf: Shelf,
    folder: String,
    modified: Option<i64>,
) -> Vec<Page> {
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let page = |title: String, kind: PageKind, text: String| Page {
        path: path.to_path_buf(),
        shelf,
        folder: folder.clone(),
        title,
        kind,
        text,
        modified,
    };
    match extension.as_str() {
        "md" | "markdown" => vec![page(
            notebook_title(text).unwrap_or_else(|| title_from_path(path)),
            PageKind::Notebook,
            text.to_owned(),
        )],
        "yaml" | "yml" => {
            let incantations = parse_incantations(text);
            if incantations.is_empty() {
                return vec![page(
                    title_from_path(path),
                    PageKind::Script { runner: None },
                    text.to_owned(),
                )];
            }
            incantations
                .into_iter()
                .map(|workflow| {
                    let mut text = workflow.content().to_owned();
                    if let Some(description) = workflow.description() {
                        text.push('\n');
                        text.push_str(description);
                    }
                    page(
                        workflow.name().to_owned(),
                        PageKind::Incantation(workflow),
                        text,
                    )
                })
                .collect()
        }
        _ => vec![page(
            title_from_path(path),
            PageKind::Script {
                runner: script_runner(path, text),
            },
            text.to_owned(),
        )],
    }
}

/// Every workflow in a (possibly multi-document) YAML file, each with an
/// argument for every `{{placeholder}}` its command uses. A document that's
/// YAML but not a workflow is skipped; a syntax error ends the file, since
/// serde_yaml's document iterator doesn't end after one (it hung a test).
pub fn parse_incantations(text: &str) -> Vec<Workflow> {
    let mut workflows = Vec::new();
    for document in serde_yaml::Deserializer::from_str(text).take(MAX_YAML_DOCUMENTS) {
        let Ok(value) = serde_yaml::Value::deserialize(document) else {
            break;
        };
        if let Ok(workflow) = serde_yaml::from_value::<Workflow>(value) {
            workflows.push(with_placeholder_arguments(workflow));
        }
    }
    workflows
}

/// Adds an argument for each `{{name}}` the workflow's command uses but
/// doesn't declare, so casting it prompts for every one.
pub fn with_placeholder_arguments(workflow: Workflow) -> Workflow {
    let declared: HashSet<String> = workflow
        .arguments()
        .iter()
        .map(|argument| argument.name().to_owned())
        .collect();
    let missing: Vec<Argument> = placeholders(workflow.content())
        .into_iter()
        .filter(|name| !declared.contains(name))
        .map(|name| Argument::new(name, ArgumentType::Text))
        .collect();
    if missing.is_empty() {
        return workflow;
    }
    let mut arguments = workflow.arguments().clone();
    arguments.extend(missing);
    workflow.with_arguments(arguments)
}

/// The `{{name}}` placeholders in a command, in order, each once.
pub fn placeholders(command: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = command;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        let name = after[..end].trim();
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && !names.iter().any(|known| known == name)
        {
            names.push(name.to_owned());
        }
        rest = &after[end + 2..];
    }
    names
}

/// A notebook's first `# ` heading.
fn notebook_title(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("# "))
        .map(|title| title.trim().to_owned())
        .filter(|title| !title.is_empty())
}

/// `device-aliases.md` -> `device aliases`.
fn title_from_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = stem.replace(['-', '_'], " ");
    let title = title.trim();
    if title.is_empty() {
        stem
    } else {
        title.to_owned()
    }
}

/// The command line that runs a script: its shebang's interpreter, else one
/// its extension implies.
pub fn script_runner(path: &Path, text: &str) -> Option<String> {
    let interpreter = shebang_interpreter(text).or_else(|| {
        let extension = path.extension()?.to_string_lossy().to_lowercase();
        let interpreter = match extension.as_str() {
            "sh" | "bash" => "bash",
            "zsh" => "zsh",
            "fish" => "fish",
            "py" => "python3",
            "js" | "mjs" | "cjs" => "node",
            "rb" => "ruby",
            "pl" => "perl",
            "ps1" => "pwsh -File",
            _ => return None,
        };
        Some(interpreter.to_owned())
    })?;
    Some(format!("{interpreter} {}", shell_word(path)))
}

/// `#!/usr/bin/env python3` -> `python3`; `#!/bin/bash -e` -> `bash`.
fn shebang_interpreter(text: &str) -> Option<String> {
    let line = text.lines().next()?.strip_prefix("#!")?;
    let mut words = line.split_whitespace();
    let program = words.next()?;
    let name = program.rsplit('/').next().unwrap_or(program);
    if name == "env" {
        return words.find(|word| !word.starts_with('-')).map(str::to_owned);
    }
    (!name.is_empty()).then(|| name.to_owned())
}

/// A path as one shell word: `~/x` under the home folder when that needs no
/// quoting, else single-quoted.
pub fn shell_word(path: &Path) -> String {
    let full = path.to_string_lossy().into_owned();
    let home = dirs::home_dir().map(|home| home.to_string_lossy().into_owned());
    let short = match &home {
        Some(home) if full.starts_with(&format!("{home}/")) => format!("~{}", &full[home.len()..]),
        _ => full.clone(),
    };
    let plain = |word: &str| {
        word.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+@%:,=~".contains(c))
    };
    if plain(&short) {
        short
    } else {
        format!("'{}'", full.replace('\'', r"'\''"))
    }
}

/// The pages matching every word of `query`, ignoring case, across title,
/// folder and text. Title matches come first; otherwise listing order holds.
/// A blank query matches everything.
pub fn search(pages: &[Page], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut hits: Vec<(bool, usize)> = pages
        .iter()
        .enumerate()
        .filter_map(|(index, page)| {
            let title = page.title.to_lowercase();
            let haystack = format!("{title}\n{}\n{}", page.folder, page.text).to_lowercase();
            words
                .iter()
                .all(|word| haystack.contains(word.as_str()))
                .then(|| {
                    let in_title = words.iter().all(|word| title.contains(word.as_str()));
                    (!in_title, index)
                })
        })
        .collect();
    hits.sort();
    hits.into_iter().map(|(_, index)| index).collect()
}

/// The first line of the page's text that holds a word of `query`, trimmed
/// and cut short: why a full-text hit matched.
pub fn hit_line(page: &Page, query: &str) -> Option<String> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return None;
    }
    let line = page.text.lines().map(str::trim).find(|line| {
        let line = line.to_lowercase();
        words.iter().any(|word| line.contains(word.as_str()))
    })?;
    Some(match line.char_indices().nth(MAX_HIT_LINE_CHARS) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_owned(),
    })
}

/// Creates a shelf's folder. A new global shelf (made first when a private
/// one inside it is) is private to its owner, since pages there may hold
/// secrets.
pub fn ensure_shelf(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    if let Some(global) = global_dir()
        && dir.starts_with(&global)
        && !global.is_dir()
    {
        std::fs::create_dir_all(&global)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&global, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    std::fs::create_dir_all(dir)
}

/// Writes a command from history into the shelf as an incantation, under
/// `inscribed/`. Returns the new file.
pub fn inscribe(
    shelf_dir: &Path,
    command: &str,
    pwd: Option<&str>,
    now: DateTime<Local>,
) -> io::Result<PathBuf> {
    let command = command.trim();
    let folder = shelf_dir.join(INSCRIBED_FOLDER);
    ensure_shelf(shelf_dir)?;
    std::fs::create_dir_all(&folder)?;
    let path = unused_path(&folder, &slug(command), "yaml");
    std::fs::write(&path, inscription_yaml(command, pwd, now))?;
    Ok(path)
}

/// The YAML an inscribed command becomes.
pub fn inscription_yaml(command: &str, pwd: Option<&str>, now: DateTime<Local>) -> String {
    let date = now.format("%Y-%m-%d");
    let description = match pwd {
        Some(pwd) => format!("Inscribed from {} on {date}", tilde(pwd)),
        None => format!("Inscribed on {date}"),
    };
    let mut mapping = serde_yaml::Mapping::new();
    mapping.insert("name".into(), spell_name(command).into());
    mapping.insert("command".into(), command.into());
    mapping.insert("description".into(), description.into());
    mapping.insert("tags".into(), vec!["inscribed".to_owned()].into());
    let arguments: Vec<serde_yaml::Value> = placeholders(command)
        .into_iter()
        .map(|name| {
            let mut argument = serde_yaml::Mapping::new();
            argument.insert("name".into(), name.into());
            serde_yaml::Value::Mapping(argument)
        })
        .collect();
    if !arguments.is_empty() {
        mapping.insert("arguments".into(), arguments.into());
    }
    serde_yaml::to_string(&mapping).unwrap_or_default()
}

/// A blank notebook in the shelf. Returns the new file.
pub fn new_notebook(shelf_dir: &Path, now: DateTime<Local>) -> io::Result<PathBuf> {
    ensure_shelf(shelf_dir)?;
    let stamp = now.format("%Y-%m-%d-%H%M%S");
    let path = unused_path(shelf_dir, &format!("scratch-{stamp}"), "md");
    let text = format!(
        "# Scratch, {}\n\nNotes, then commands to run:\n\n```sh\necho hello\n```\n",
        now.format("%Y-%m-%d %H:%M")
    );
    std::fs::write(&path, text)?;
    Ok(path)
}

/// A starter incantation in the shelf. Returns the new file.
pub fn new_incantation(shelf_dir: &Path, now: DateTime<Local>) -> io::Result<PathBuf> {
    ensure_shelf(shelf_dir)?;
    let stamp = now.format("%Y-%m-%d-%H%M%S");
    let path = unused_path(shelf_dir, &format!("incantation-{stamp}"), "yaml");
    let text = "name: New incantation\n\
                command: echo \"hello, {{who}}\"\n\
                description: Say what it does. {{who}} is asked for when it's cast.\n\
                tags: []\n\
                arguments:\n  - name: who\n    default_value: world\n";
    std::fs::write(&path, text)?;
    Ok(path)
}

/// `dir/stem.ext`, or `dir/stem-2.ext` and up if that's taken.
fn unused_path(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    let mut path = dir.join(format!("{stem}.{extension}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.{extension}"));
        n += 1;
    }
    path
}

/// `git push origin main` -> `git-push-origin-main`.
pub fn slug(text: &str) -> String {
    let mut slug = String::new();
    for c in text.chars() {
        if slug.len() >= MAX_SLUG_CHARS {
            break;
        }
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "spell".to_owned()
    } else {
        slug.to_owned()
    }
}

/// An incantation's name from its command: its first few words.
pub fn spell_name(command: &str) -> String {
    let words: Vec<&str> = command.split_whitespace().collect();
    let mut name = words
        .iter()
        .take(MAX_NAME_WORDS)
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    let mut cut = words.len() > MAX_NAME_WORDS;
    if let Some((end, _)) = name.char_indices().nth(MAX_NAME_CHARS) {
        name.truncate(end);
        cut = true;
    }
    if cut {
        name.push('…');
    }
    name
}

/// `~/lab/x` for a path under the home folder.
pub fn tilde(path: &str) -> String {
    match dirs::home_dir().and_then(|home| home.to_str().map(str::to_owned)) {
        Some(home) if path == home => "~".to_owned(),
        Some(home) if path.starts_with(&format!("{home}/")) => format!("~{}", &path[home.len()..]),
        _ => path.to_owned(),
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
