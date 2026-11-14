//! Shellomancy: Code Maps — cartogomancy maps of local folders.
//!
//! Mapping a folder runs the cartogomancy CLI (`cartogomancy` from the user's
//! interactive-shell PATH, else `npx --yes @madnessengineering/cartogomancy`)
//! and keeps the map JSON in `~/.mad/shellomancy/maps/`. When the user is
//! signed in to a Madness account and hasn't turned uploads off, the new map is
//! sent to Inventorium (`POST /api/uml-data/upload`), where each upload becomes
//! a new version that SwarmDesk builds a code city from.
//!
//! A mapped folder inside a git repo is mapped again on its own when the
//! repo's default branch moves (a merge or pull onto main) or its tags change
//! (a new version), at most once an hour; plain commits on other branches
//! never trigger it. The repo's refs are watched directly, since Warp's repo
//! watcher leaves tags and `packed-refs` out.
//!
//! Nothing here touches the network except that upload. The list of mapped
//! folders lives next to the maps in `index.json`.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use command::r#async::Command;
use futures::FutureExt as _;
use futures::future::BoxFuture;
use notify_debouncer_full::notify::{RecursiveMode, WatchFilter};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use warp_core::paths::SHELLOMANCY_CONFIG_DIR;
use warp_util::git::run_git_command;
use warp_util::path::resolve_executable_in_path;
use warpui::r#async::Timer;
use warpui::{Entity, ModelContext, ModelHandle, SingletonEntity};
use watcher::{BulkFilesystemWatcher, BulkFilesystemWatcherEvent};

use crate::madness_account::{MADNESS_API_ROOT, MadnessAccount};
use crate::settings::code_maps::CodeMapsSettings;
#[cfg(feature = "local_tty")]
use crate::terminal::local_shell::LocalShellState;

const INDEX_FILE: &str = "index.json";
const NPM_PACKAGE: &str = "@madnessengineering/cartogomancy";
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// How much of a failed run's output to show.
const ERROR_TAIL_CHARS: usize = 300;
/// The least time between two maps of a folder made on their own.
const AUTO_REMAP_INTERVAL_SECS: i64 = 60 * 60;
/// How long after startup to look for refs that moved while we were closed.
const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(30);
const REFS_WATCH_DEBOUNCE: Duration = Duration::from_secs(2);

/// The numbers shown for a map.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MapSummary {
    pub classes: u64,
    pub packages: u64,
    pub critical: u64,
    pub high: u64,
    pub unused_exports: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UploadOutcome {
    Uploaded {
        version: u64,
        view_url: Option<String>,
    },
    /// Signed out, or uploads are turned off.
    KeptLocal,
    Failed {
        error: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MappedFolder {
    pub path: PathBuf,
    /// The folder's name; also the project name the map is uploaded under, the
    /// same one `cartogomancy --upload` uses, so the versions stack together.
    pub name: String,
    pub map_file: PathBuf,
    /// Unix seconds of the last successful map.
    #[serde(default)]
    pub mapped_at: Option<i64>,
    #[serde(default)]
    pub summary: Option<MapSummary>,
    #[serde(default)]
    pub upload: Option<UploadOutcome>,
    /// Why the last attempt failed, ready to show.
    #[serde(default)]
    pub error: Option<String>,
    /// The repo's refs as of the last map; `None` outside a git repo.
    #[serde(default)]
    pub git: Option<GitRefs>,
}

/// What a folder's repo looked like when it was last mapped.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GitRefs {
    /// The repo's common git dir, where `refs/` and `packed-refs` live.
    pub git_dir: PathBuf,
    /// main, master, or whatever `origin/HEAD` points at.
    pub default_branch: String,
    /// A hash of the default branch's commit and every tag.
    pub signature: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    folders: Vec<MappedFolder>,
}

#[derive(Clone, Copy, Debug)]
pub enum CodeMapsEvent {
    Changed,
}

pub struct CodeMaps {
    dir: PathBuf,
    folders: Vec<MappedFolder>,
    mapping: HashSet<PathBuf>,
    refs_watcher: ModelHandle<BulkFilesystemWatcher>,
    watched_git_dirs: HashSet<PathBuf>,
    /// Folders with a re-map waiting out the hourly limit.
    remap_scheduled: HashSet<PathBuf>,
}

impl Entity for CodeMaps {
    type Event = CodeMapsEvent;
}

impl SingletonEntity for CodeMaps {}

impl CodeMaps {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let dir = maps_dir();
        let folders = std::fs::read_to_string(dir.join(INDEX_FILE))
            .ok()
            .and_then(|raw| serde_json::from_str::<Index>(&raw).ok())
            .map(|index| index.folders)
            .unwrap_or_default();
        let refs_watcher =
            ctx.add_model(|ctx| BulkFilesystemWatcher::new(REFS_WATCH_DEBOUNCE, ctx));
        ctx.subscribe_to_model(&refs_watcher, Self::on_refs_changed);
        let mut me = Self {
            dir,
            folders,
            mapping: HashSet::new(),
            refs_watcher,
            watched_git_dirs: HashSet::new(),
            remap_scheduled: HashSet::new(),
        };
        let git_dirs: Vec<PathBuf> = me
            .folders
            .iter()
            .filter_map(|f| f.git.as_ref().map(|git| git.git_dir.clone()))
            .collect();
        for git_dir in git_dirs {
            me.watch_refs(git_dir, ctx);
        }
        // Catch up on merges and tags made while Shellomancy was closed.
        let _ = ctx.spawn(Timer::after(STARTUP_CHECK_DELAY), |me, _, ctx| {
            let paths: Vec<PathBuf> = me
                .folders
                .iter()
                .filter(|f| f.git.is_some())
                .map(|f| f.path.clone())
                .collect();
            for path in paths {
                me.check_refs(path, ctx);
            }
        });
        me
    }

    pub fn folders(&self) -> &[MappedFolder] {
        &self.folders
    }

    pub fn is_mapping(&self, path: &Path) -> bool {
        self.mapping.contains(path)
    }

    /// Maps `path` (adding it to the list if it's new) and, when signed in with
    /// uploads on, sends the map to Inventorium.
    pub fn map(&mut self, path: PathBuf, ctx: &mut ModelContext<Self>) {
        let path = path.canonicalize().unwrap_or(path);
        if self.mapping.contains(&path) {
            return;
        }
        let index = match self.folders.iter().position(|f| f.path == path) {
            Some(index) => index,
            None => {
                self.folders.push(MappedFolder {
                    name: folder_name(&path),
                    map_file: self.dir.join(map_file_name(&path)),
                    path: path.clone(),
                    mapped_at: None,
                    summary: None,
                    upload: None,
                    error: None,
                    git: None,
                });
                self.folders.len() - 1
            }
        };
        let folder = &mut self.folders[index];
        folder.error = None;
        let name = folder.name.clone();
        let map_file = folder.map_file.clone();
        self.mapping.insert(path.clone());
        self.changed(ctx);

        let upload_key = upload_key(ctx);
        let path_env = interactive_path(ctx);
        let run_path = path.clone();
        let _ = ctx.spawn(
            async move {
                // Read the refs before mapping, so a merge that lands while
                // cartogomancy runs still counts as unmapped.
                let git = git_refs(&run_path).await;
                let path_env = path_env.await;
                run_cartogomancy(path_env, &run_path, &map_file).await?;
                let map = read_map(&map_file)?;
                let summary = summarize(&map);
                let upload = match upload_key {
                    Some(key) => upload(&key, &name, map).await,
                    None => UploadOutcome::KeptLocal,
                };
                Ok::<_, String>((summary, upload, git))
            },
            move |me, result, ctx| {
                me.mapping.remove(&path);
                let mut watch = None;
                if let Some(folder) = me.folders.iter_mut().find(|f| f.path == path) {
                    match result {
                        Ok((summary, upload, git)) => {
                            folder.mapped_at = Some(chrono::Utc::now().timestamp());
                            folder.summary = Some(summary);
                            folder.upload = Some(upload);
                            watch = git.as_ref().map(|git| git.git_dir.clone());
                            folder.git = git;
                        }
                        Err(error) => folder.error = Some(error),
                    }
                }
                if let Some(git_dir) = watch {
                    me.watch_refs(git_dir, ctx);
                }
                me.changed(ctx);
            },
        );
    }

    /// Drops `path` from the list and deletes its local map. Uploaded versions
    /// stay in Inventorium.
    pub fn forget(&mut self, path: &Path, ctx: &mut ModelContext<Self>) {
        let Some(index) = self.folders.iter().position(|f| f.path == path) else {
            return;
        };
        let folder = self.folders.remove(index);
        if let Err(err) = std::fs::remove_file(&folder.map_file)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("Could not delete code map {:?}: {err}", folder.map_file);
        }
        self.changed(ctx);
    }

    /// Watches a repo's branch heads, tags and `packed-refs`.
    fn watch_refs(&mut self, git_dir: PathBuf, ctx: &mut ModelContext<Self>) {
        if !self.watched_git_dirs.insert(git_dir.clone()) {
            return;
        }
        let refs_dir = git_dir.join("refs");
        let root = git_dir.clone();
        // Only consulted by inotify: keep it out of objects/ and friends.
        let watch_dir = Arc::new(move |dir: &Path| dir == root || dir.starts_with(&refs_dir));
        let filter = WatchFilter::with_filter(watch_dir, Arc::new(|path: &Path| is_ref_path(path)));
        self.refs_watcher.update(ctx, |watcher, _| {
            std::mem::drop(watcher.register_path(&git_dir, filter, RecursiveMode::Recursive));
        });
    }

    fn on_refs_changed(
        &mut self,
        _: ModelHandle<BulkFilesystemWatcher>,
        event: &BulkFilesystemWatcherEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        let changed: Vec<&PathBuf> = event
            .added
            .iter()
            .chain(&event.modified)
            .chain(&event.deleted)
            .chain(event.moved.keys())
            .chain(event.moved.values())
            .collect();
        let paths: Vec<PathBuf> = self
            .folders
            .iter()
            .filter(|f| {
                f.git
                    .as_ref()
                    .is_some_and(|git| changed.iter().any(|p| p.starts_with(&git.git_dir)))
            })
            .map(|f| f.path.clone())
            .collect();
        for path in paths {
            self.check_refs(path, ctx);
        }
    }

    /// Maps `path` again if its default branch or tags moved since the last
    /// map, waiting out the hourly limit first.
    fn check_refs(&mut self, path: PathBuf, ctx: &mut ModelContext<Self>) {
        if !*CodeMapsSettings::as_ref(ctx).auto_remap_enabled
            || self.mapping.contains(&path)
            || self.remap_scheduled.contains(&path)
        {
            return;
        }
        let Some(folder) = self.folders.iter().find(|f| f.path == path) else {
            return;
        };
        let Some(known) = folder.git.clone() else {
            return;
        };
        let mapped_at = folder.mapped_at;
        let check_path = path.clone();
        let _ = ctx.spawn(
            async move { refs_signature(&check_path, &known.default_branch).await },
            move |me, signature, ctx| {
                let Some(signature) = signature else { return };
                if signature == known.signature {
                    return;
                }
                match remap_wait(mapped_at, chrono::Utc::now().timestamp()) {
                    None => me.map(path, ctx),
                    Some(wait) => {
                        if !me.remap_scheduled.insert(path.clone()) {
                            return;
                        }
                        let _ = ctx.spawn(Timer::after(wait), move |me, _, ctx| {
                            me.remap_scheduled.remove(&path);
                            me.check_refs(path, ctx);
                        });
                    }
                }
            },
        );
    }

    fn changed(&mut self, ctx: &mut ModelContext<Self>) {
        if let Err(err) = self.save() {
            log::warn!("Could not save the code maps index: {err}");
        }
        ctx.emit(CodeMapsEvent::Changed);
        ctx.notify();
    }

    fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let raw = serde_json::to_string_pretty(&Index {
            folders: self.folders.clone(),
        })?;
        let tmp = self.dir.join(format!("{INDEX_FILE}.tmp"));
        std::fs::write(&tmp, raw)?;
        std::fs::rename(tmp, self.dir.join(INDEX_FILE))
    }
}

/// Whether a path under a git dir is a branch head, a tag or `packed-refs`.
fn is_ref_path(path: &Path) -> bool {
    let path = path.to_string_lossy();
    path.ends_with("packed-refs") || path.contains("/refs/heads/") || path.contains("/refs/tags/")
}

/// How long to wait before mapping again, given when the last map was made;
/// `None` means now.
fn remap_wait(mapped_at: Option<i64>, now: i64) -> Option<Duration> {
    let next = mapped_at? + AUTO_REMAP_INTERVAL_SECS;
    (next > now).then(|| Duration::from_secs((next - now) as u64))
}

/// The repo `folder` is in, its default branch and the current signature of
/// that branch and the tags; `None` outside a git repo.
async fn git_refs(folder: &Path) -> Option<GitRefs> {
    let git_dir = run_git_command(
        folder,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .await
    .ok()?;
    let git_dir = PathBuf::from(git_dir.trim());
    let default_branch = default_branch(folder).await?;
    let signature = refs_signature(folder, &default_branch).await?;
    Some(GitRefs {
        git_dir,
        default_branch,
        signature,
    })
}

/// What `origin/HEAD` points at, else main, else master.
async fn default_branch(folder: &Path) -> Option<String> {
    if let Ok(origin_head) = run_git_command(
        folder,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .await
        && let Some(branch) = origin_head.trim().strip_prefix("origin/")
        && !branch.is_empty()
    {
        return Some(branch.to_owned());
    }
    for candidate in ["main", "master"] {
        let head = format!("refs/heads/{candidate}");
        if run_git_command(folder, &["rev-parse", "--verify", "--quiet", &head])
            .await
            .is_ok()
        {
            return Some(candidate.to_owned());
        }
    }
    None
}

/// A hash of the default branch's commit and every tag's name and target.
async fn refs_signature(folder: &Path, default_branch: &str) -> Option<String> {
    let head = format!("refs/heads/{default_branch}");
    let refs = run_git_command(
        folder,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            &head,
            "refs/tags",
        ],
    )
    .await
    .ok()?;
    Some(format!("{:08x}", fnv1a(refs.as_bytes())))
}

/// FNV-1a: stable across runs and Rust versions, unlike DefaultHasher.
fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c9dc5u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x01000193)
    })
}

/// `~/.mad/shellomancy/maps/`, whatever the platform's data dir is.
fn maps_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(SHELLOMANCY_CONFIG_DIR)
        .join("maps")
}

/// The API key to upload with, if the user is signed in and uploads are on.
fn upload_key(ctx: &mut ModelContext<CodeMaps>) -> Option<String> {
    if !*CodeMapsSettings::as_ref(ctx).upload_enabled
        || !ctx.has_singleton_model::<MadnessAccount>()
    {
        return None;
    }
    MadnessAccount::as_ref(ctx).api_key().map(str::to_owned)
}

/// The PATH from the user's interactive shell, where nvm-installed tools live;
/// a GUI app's own PATH usually doesn't have them.
fn interactive_path(ctx: &mut ModelContext<CodeMaps>) -> BoxFuture<'static, Option<String>> {
    #[cfg(feature = "local_tty")]
    if ctx.has_singleton_model::<LocalShellState>() {
        return LocalShellState::handle(ctx)
            .update(ctx, |shell, ctx| shell.get_interactive_path_env_var(ctx));
    }
    let _ = ctx;
    futures::future::ready(None).boxed()
}

async fn run_cartogomancy(
    path_env: Option<String>,
    folder: &Path,
    map_file: &Path,
) -> Result<(), String> {
    let failed =
        |error: String| madness_themes::tf("shellomancy.codeMaps.failed", &[("error", &error)]);
    let search: OsString = path_env
        .map(OsString::from)
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    let (program, prefix): (PathBuf, &[&str]) =
        if let Some(found) = resolve_executable_in_path("cartogomancy", &search) {
            (found.into_owned(), &[])
        } else if let Some(found) = resolve_executable_in_path("npx", &search) {
            (found.into_owned(), &["--yes", NPM_PACKAGE])
        } else {
            return Err(madness_themes::t("shellomancy.codeMaps.notFound").to_owned());
        };
    if let Some(dir) = map_file.parent() {
        std::fs::create_dir_all(dir).map_err(|err| failed(err.to_string()))?;
    }
    let output = Command::new(&program)
        .args(prefix)
        .arg(folder)
        .arg("--output")
        .arg(map_file)
        .current_dir(folder)
        .env("PATH", &search)
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|err| failed(err.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(failed(error_tail(&output.stderr, &output.stdout)))
    }
}

/// The last non-empty line of stderr, else of stdout, trimmed for display.
fn error_tail(stderr: &[u8], stdout: &[u8]) -> String {
    let last_line = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .lines()
            .map(str::trim)
            .rfind(|line| !line.is_empty())
            .map(str::to_owned)
    };
    let line = last_line(stderr)
        .or_else(|| last_line(stdout))
        .unwrap_or_else(|| "cartogomancy exited with an error".to_owned());
    let line = line.trim_start_matches(['❌', ' ']);
    match line.char_indices().nth(ERROR_TAIL_CHARS) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_owned(),
    }
}

fn read_map(map_file: &Path) -> Result<Value, String> {
    let failed =
        |error: String| madness_themes::tf("shellomancy.codeMaps.failed", &[("error", &error)]);
    let raw = std::fs::read_to_string(map_file).map_err(|err| failed(err.to_string()))?;
    let map: Value = serde_json::from_str(&raw).map_err(|err| failed(err.to_string()))?;
    if !(map["classes"].is_array() && map["packages"].is_array()) {
        return Err(failed("the map has no classes or packages".to_owned()));
    }
    Ok(map)
}

fn summarize(map: &Value) -> MapSummary {
    let count = |value: &Value| value.as_array().map_or(0, |items| items.len() as u64);
    let threats = &map["complexityAnalysis"]["threatLevelDistribution"];
    MapSummary {
        classes: count(&map["classes"]),
        packages: count(&map["packages"]),
        critical: threats["CRITICAL"].as_u64().unwrap_or(0),
        high: threats["HIGH"].as_u64().unwrap_or(0),
        unused_exports: count(&map["importAnalysis"]["unusedExports"]),
    }
}

async fn upload(key: &str, name: &str, map: Value) -> UploadOutcome {
    let body = json!({ "umlData": map, "name": name });
    let response = http_client::Client::new_external()
        .post(format!("{MADNESS_API_ROOT}/uml-data/upload"))
        .bearer_auth(key)
        .timeout(UPLOAD_TIMEOUT)
        .json(&body)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(err) => {
            return UploadOutcome::Failed {
                error: err.to_string(),
            };
        }
    };
    let status = response.status().as_u16();
    let body: Value = response.json().await.unwrap_or_default();
    upload_outcome(status, &body)
}

/// Reads Inventorium's upload response.
fn upload_outcome(status: u16, body: &Value) -> UploadOutcome {
    if (200..300).contains(&status) && body["success"] == true {
        return UploadOutcome::Uploaded {
            version: body["version"].as_u64().unwrap_or(0),
            view_url: body["viewUrl"].as_str().map(str::to_owned),
        };
    }
    let error = match body["message"].as_str() {
        Some(message) if !message.is_empty() => format!("{message} (HTTP {status})"),
        _ => format!("HTTP {status}"),
    };
    UploadOutcome::Failed { error }
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "root".to_owned())
}

/// `<name>-<hash of the full path>-uml.json`, so two folders with the same
/// name don't share a map.
fn map_file_name(path: &Path) -> String {
    let safe: String = folder_name(path)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let hash = fnv1a(path.to_string_lossy().as_bytes());
    format!("{safe}-{hash:08x}-uml.json")
}

#[cfg(test)]
#[path = "code_maps_tests.rs"]
mod tests;
