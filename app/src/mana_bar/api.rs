//! Inventorium's REST API as the Mana Bar reads it: todos, quests and lessons,
//! authenticated with the Madness account's `omni_` key.
//!
//! The parsers read leniently from `serde_json::Value` (missing fields get
//! defaults) and keep only what the panel shows; lesson `embedding` arrays in
//! particular are dropped.

use std::time::Duration;

use serde_json::{Value, json};
use url::Url;

use crate::madness_account::{MADNESS_API_ROOT, TODO_URL_ROOT, error_reason};

/// The site, for things that have no page of their own (quests, lessons).
pub const DASHBOARD_URL: &str = "https://madnessinteractive.cc/dashboard";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Todos fetched for one project. The server sorts newest first and then
/// limits, so a cap that's too low hides old todos.
const PROJECT_TODO_LIMIT: u32 = 200;
/// Todos fetched for all projects.
pub const ALL_TODO_LIMIT: u32 = 300;
const QUEST_LIMIT: u32 = 50;
const LESSON_LIMIT: u32 = 50;
const ACTIVITY_LIMIT: u32 = 50;

/// A todo's priority, in the order the panel lists them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Critical,
    High,
    #[default]
    Medium,
    Low,
}

impl Priority {
    /// Mirrors Inventorium's `normalizePriority`: known aliases, anything else
    /// (including nothing) is Medium.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_lowercase().as_str() {
            "critical" | "crit" | "urgent" | "blocker" | "p0" => Priority::Critical,
            "high" | "hi" | "h" | "p1" => Priority::High,
            "low" | "lo" | "l" | "p3" | "minor" => Priority::Low,
            _ => Priority::Medium,
        }
    }

    /// The voice key for the group heading.
    pub fn voice_key(self) -> &'static str {
        match self {
            Priority::Critical => "shellomancy.manaBar.priorityCritical",
            Priority::High => "shellomancy.manaBar.priorityHigh",
            Priority::Medium => "shellomancy.manaBar.priorityMedium",
            Priority::Low => "shellomancy.manaBar.priorityLow",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Todo {
    pub id: String,
    pub description: String,
    pub project: String,
    pub priority: Priority,
    pub status: String,
    /// Unix seconds.
    pub created_at: i64,
    pub tags: Vec<String>,
    /// Unix seconds; 0 if never updated.
    pub updated_at: i64,
    pub notes: Option<String>,
    /// Who it's for: "user", "claude", ...
    pub target_agent: Option<String>,
    pub ticket: Option<String>,
    /// The rest of its metadata (quest_id, context, next_step, ...), tags
    /// left out, values as text.
    pub extra: Vec<(String, String)>,
}

/// A quest and how far along it is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quest {
    pub id: String,
    pub name: String,
    pub project: String,
    pub description: Option<String>,
    pub status: String,
    pub total: u64,
    pub completed: u64,
    pub pct: u64,
    /// The first todo that can be worked on next, if any.
    pub next: Option<NextStep>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NextStep {
    pub todo_id: String,
    pub description: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lesson {
    pub id: String,
    pub topic: String,
    pub body: String,
    pub tags: Vec<String>,
    /// bash, rust, python, ... or "general".
    pub language: String,
    /// Unix seconds.
    pub created_at: i64,
}

/// One entry in the activity log: something that happened to a todo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LogEntry {
    /// The todo it happened to.
    pub todo_id: String,
    /// create, update, complete, delete, restore, ...
    pub operation: String,
    pub description: String,
    pub project: String,
    /// Unix seconds.
    pub at: i64,
    /// Which fields an update changed.
    pub fields: Vec<String>,
    /// The note left when it was completed, if any.
    pub comment: Option<String>,
    /// What an update changed, field by field.
    pub changes: Vec<Change>,
    /// Who or what made the change (the request's user agent).
    pub actor: Option<String>,
}

/// One field an update changed, values as text (`None` when unset).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change {
    pub field: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// Whether a todo still needs doing. Inventorium's `exclude_completed` only
/// drops completed/complete/done, so the rest are dropped here.
pub fn is_active(status: &str) -> bool {
    !matches!(
        status.trim().to_lowercase().as_str(),
        "completed" | "complete" | "done" | "archived" | "cancelled" | "canceled" | "deleted"
    )
}

/// The active todos in a `GET /api/todos` reply (`{ "todos": [...] }`).
pub fn parse_todos(reply: &Value) -> Vec<Todo> {
    reply["todos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|todo| {
            let id = text(&todo["id"])?;
            let status = text(&todo["status"]).unwrap_or_else(|| "pending".to_owned());
            is_active(&status).then(|| Todo {
                id,
                description: text(&todo["description"]).unwrap_or_default(),
                project: text(&todo["project"]).unwrap_or_default(),
                priority: Priority::parse(todo["priority"].as_str().unwrap_or_default()),
                status,
                created_at: timestamp(&todo["created_at"]),
                tags: tags_of(&todo["tags"])
                    .into_iter()
                    .chain(tags_of(&todo["metadata"]["tags"]))
                    .fold(Vec::new(), |mut tags, tag| {
                        if !tags.contains(&tag) {
                            tags.push(tag);
                        }
                        tags
                    }),
                updated_at: log_timestamp(&todo["updated_at"]),
                notes: text(&todo["notes"]),
                target_agent: text(&todo["target_agent"]),
                ticket: text(&todo["ticket"]),
                extra: metadata_details(&todo["metadata"]),
            })
        })
        .collect()
}

/// Todos in the order the panel lists them (priority, then status, then
/// newest first), without copying them.
pub fn sort_todos(todos: &[Todo]) -> Vec<&Todo> {
    let mut sorted: Vec<&Todo> = todos.iter().collect();
    sorted.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| status_rank(&a.status).cmp(&status_rank(&b.status)))
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    sorted
}

/// Todos grouped by priority (Critical first), each group ordered by status
/// (in progress, pending, blocked, review, anything else) and then newest
/// first. Empty groups are left out.
pub fn group_by_priority(mut todos: Vec<Todo>) -> Vec<(Priority, Vec<Todo>)> {
    todos.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| status_rank(&a.status).cmp(&status_rank(&b.status)))
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    let mut groups: Vec<(Priority, Vec<Todo>)> = Vec::new();
    for todo in todos {
        match groups.last_mut() {
            Some((priority, group)) if *priority == todo.priority => group.push(todo),
            _ => groups.push((todo.priority, vec![todo])),
        }
    }
    groups
}

fn status_rank(status: &str) -> u8 {
    match status {
        "in_progress" => 0,
        "pending" => 1,
        "blocked" => 2,
        "review" => 3,
        _ => 4,
    }
}

/// The quests in a `GET /api/quests/progress` reply
/// (`{ "data": [{ "quest": {...}, "overall": {...}, "next_actions": [...] }] }`).
pub fn parse_quests(reply: &Value) -> Vec<Quest> {
    reply["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let quest = &entry["quest"];
            let overall = &entry["overall"];
            let next = entry["next_actions"]
                .as_array()
                .and_then(|actions| actions.first())
                .and_then(|action| {
                    Some(NextStep {
                        todo_id: text(&action["todo_id"])?,
                        description: text(&action["description"]).unwrap_or_default(),
                    })
                });
            Some(Quest {
                id: text(&quest["id"])?,
                name: text(&quest["name"]).unwrap_or_default(),
                project: text(&quest["project"]).unwrap_or_default(),
                description: text(&quest["description"]),
                status: text(&quest["status"]).unwrap_or_default(),
                total: overall["total"].as_u64().unwrap_or(0),
                completed: overall["completed"].as_u64().unwrap_or(0),
                pct: overall["pct"].as_u64().unwrap_or(0),
                next,
            })
        })
        .collect()
}

/// The lessons in a `GET /api/lessons` reply (`{ "data": [...] }`).
pub fn parse_lessons(reply: &Value) -> Vec<Lesson> {
    reply["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|lesson| {
            Some(Lesson {
                id: text(&lesson["id"])?,
                topic: text(&lesson["topic"]).unwrap_or_default(),
                body: text(&lesson["lesson_learned"]).unwrap_or_default(),
                tags: tags_of(&lesson["tags"]),
                language: text(&lesson["language"]).unwrap_or_else(|| "general".to_owned()),
                created_at: timestamp(&lesson["created_at"]),
            })
        })
        .collect()
}

/// Reads `GET /api/todos/activity` (`{ logs: [...] }`). Entries without a
/// todo id are dropped; everything else is read leniently, since the log has
/// been written by several generations of Inventorium and Omnispindle.
pub fn parse_activity(reply: &Value) -> Vec<LogEntry> {
    reply["logs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let todo_id = text(&entry["todoId"]).or_else(|| text(&entry["todo_id"]))?;
            let changes: Vec<Change> = entry["changes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|change| {
                    Some(Change {
                        field: text(&change["field"])?,
                        old: value_text(&change["old_value"]),
                        new: value_text(&change["new_value"]),
                    })
                })
                .collect();
            let mut fields: Vec<String> = changes.iter().map(|c| c.field.clone()).collect();
            fields.dedup();
            Some(LogEntry {
                todo_id,
                operation: text(&entry["operation"]).unwrap_or_else(|| "update".to_owned()),
                description: text(&entry["todoTitle"])
                    .or_else(|| text(&entry["description"]))
                    .or_else(|| text(&entry["todo_description"]))
                    .unwrap_or_default(),
                project: text(&entry["project"]).unwrap_or_default(),
                at: log_timestamp(&entry["timestamp"]),
                fields,
                comment: [
                    "completion_comment",
                    "comment",
                    "completionComment",
                    "completion_notes",
                ]
                .iter()
                .find_map(|key| text(&entry[*key]).or_else(|| text(&entry["metadata"][*key]))),
                changes,
                actor: text(&entry["userAgent"]).or_else(|| text(&entry["user_agent"])),
            })
        })
        .collect()
}

/// A log timestamp in Unix seconds: an ISO date string (how Mongo dates come
/// back as JSON), or a number in seconds or milliseconds.
fn log_timestamp(value: &Value) -> i64 {
    if let Some(text) = value.as_str() {
        if let Ok(at) = chrono::DateTime::parse_from_rfc3339(text.trim()) {
            return at.timestamp();
        }
    }
    let seconds = timestamp(value);
    if seconds > 10_000_000_000 {
        seconds / 1000
    } else {
        seconds
    }
}

/// The lessons whose topic, tags or body mention `query` (ignoring case);
/// all of them for a blank query.
pub fn filter_lessons<'a>(lessons: &'a [Lesson], query: &str) -> Vec<&'a Lesson> {
    let query = query.trim().to_lowercase();
    lessons
        .iter()
        .filter(|lesson| {
            query.is_empty()
                || lesson.topic.to_lowercase().contains(&query)
                || lesson.body.to_lowercase().contains(&query)
                || lesson
                    .tags
                    .iter()
                    .any(|tag| tag.to_lowercase().contains(&query))
        })
        .collect()
}

/// The Omnispindle project for a terminal: the focused git repo's folder
/// name, lowercased. Outside a repo (say, the home folder) there's no project,
/// so the panel shows every project.
pub fn project_name(repo_folder: Option<&str>) -> Option<String> {
    repo_folder
        .map(|name| name.trim().to_lowercase())
        .filter(|name| !name.is_empty())
}

/// `GET /api/todos` for one project (or all of them).
pub fn todos_url(project: Option<&str>) -> Url {
    let limit = match project {
        Some(_) => PROJECT_TODO_LIMIT,
        None => ALL_TODO_LIMIT,
    }
    .to_string();
    let mut params = vec![("exclude_completed", "true"), ("limit", limit.as_str())];
    if let Some(project) = project {
        params.push(("project", project));
    }
    api_url("/todos", &params)
}

/// `GET /api/quests/progress` for active quests in one project (or all).
pub fn quests_url(project: Option<&str>) -> Url {
    let limit = QUEST_LIMIT.to_string();
    let mut params = vec![("status", "active"), ("limit", limit.as_str())];
    if let Some(project) = project {
        params.push(("project", project));
    }
    api_url("/quests/progress", &params)
}

/// `GET /api/lessons`, newest first.
pub fn lessons_url() -> Url {
    let limit = LESSON_LIMIT.to_string();
    api_url(
        "/lessons",
        &[
            ("limit", limit.as_str()),
            ("sortBy", "created_at"),
            ("sortOrder", "desc"),
        ],
    )
}

/// `GET /api/todos/activity`, newest first, for one project (or all).
pub fn activity_url(project: Option<&str>) -> Url {
    let limit = ACTIVITY_LIMIT.to_string();
    let mut params = vec![("limit", limit.as_str())];
    if let Some(project) = project {
        params.push(("project", project));
    }
    api_url("/todos/activity", &params)
}

/// `POST /api/todos/:id/complete`.
pub fn complete_url(id: &str) -> Url {
    let mut url =
        Url::parse(&format!("{MADNESS_API_ROOT}/todos")).expect("the API root is a valid URL");
    url.path_segments_mut()
        .expect("the API root is a base URL")
        .push(id)
        .push("complete");
    url
}

/// Where a todo opens on the site.
pub fn todo_page_url(id: &str) -> String {
    let mut url = Url::parse(TODO_URL_ROOT).expect("TODO_URL_ROOT is a valid URL");
    url.path_segments_mut()
        .expect("TODO_URL_ROOT is a base URL")
        .push(id);
    url.to_string()
}

fn api_url(path: &str, params: &[(&str, &str)]) -> Url {
    Url::parse_with_params(&format!("{MADNESS_API_ROOT}{path}"), params)
        .expect("the API root and paths are valid URLs")
}

pub async fn fetch_todos(key: String, project: Option<String>) -> Result<Vec<Todo>, String> {
    get_json(&key, todos_url(project.as_deref()))
        .await
        .map(|reply| parse_todos(&reply))
}

pub async fn fetch_quests(key: String, project: Option<String>) -> Result<Vec<Quest>, String> {
    get_json(&key, quests_url(project.as_deref()))
        .await
        .map(|reply| parse_quests(&reply))
}

pub async fn fetch_activity(key: String, project: Option<String>) -> Result<Vec<LogEntry>, String> {
    get_json(&key, activity_url(project.as_deref()))
        .await
        .map(|reply| parse_activity(&reply))
}

pub async fn fetch_lessons(key: String) -> Result<Vec<Lesson>, String> {
    get_json(&key, lessons_url())
        .await
        .map(|reply| parse_lessons(&reply))
}

/// Marks a todo done; the error is a message ready to show.
pub async fn complete_todo(key: String, id: String) -> Result<(), String> {
    let response = http_client::Client::new_external()
        .post(complete_url(&id))
        .bearer_auth(&key)
        .timeout(REQUEST_TIMEOUT)
        .json(&json!({ "comment": "Marked done from Shellomancy's Mana Bar." }))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(());
    }
    let reply: Value = response.json().await.unwrap_or_default();
    Err(error_reason(status, &reply))
}

async fn get_json(key: &str, url: Url) -> Result<Value, String> {
    let response = http_client::Client::new_external()
        .get(url)
        .bearer_auth(key)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let status = response.status().as_u16();
    let reply: Value = response.json().await.unwrap_or_default();
    if (200..300).contains(&status) {
        Ok(reply)
    } else {
        Err(error_reason(status, &reply))
    }
}

/// A JSON array of tag strings, trimmed, blanks dropped.
fn tags_of(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(text)
        .collect()
}

/// How long ago `created_at` was, short: "now", "5m", "3h", "27d", "8mo", "2y".
pub fn age_label(created_at: i64, now: i64) -> String {
    if created_at <= 0 {
        return String::new();
    }
    let seconds = (now - created_at).max(0);
    let (minutes, hours, days) = (seconds / 60, seconds / 3600, seconds / 86_400);
    match () {
        _ if minutes < 1 => "now".to_owned(),
        _ if hours < 1 => format!("{minutes}m"),
        _ if days < 1 => format!("{hours}h"),
        _ if days < 60 => format!("{days}d"),
        _ if days < 730 => format!("{}mo", days / 30),
        _ => format!("{}y", days / 365),
    }
}

/// A todo's metadata other than its tags, as (key, text) sorted by key.
fn metadata_details(metadata: &Value) -> Vec<(String, String)> {
    let mut details: Vec<(String, String)> = metadata
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(key, _)| key.as_str() != "tags")
        .filter_map(|(key, value)| Some((key.clone(), value_text(value)?)))
        .collect();
    details.sort();
    details
}

/// Any JSON value as one line of text for the detail view: strings as they
/// are, arrays joined with ", ", objects as compact JSON. Null and blank give
/// `None`.
pub fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(_) => text(value),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(value_text).collect();
            (!parts.is_empty()).then(|| parts.join(", "))
        }
        Value::Object(map) if map.is_empty() => None,
        other => Some(other.to_string()),
    }
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// Unix seconds from a number or a numeric string; 0 otherwise.
fn timestamp(value: &Value) -> i64 {
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|seconds| seconds as i64))
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
