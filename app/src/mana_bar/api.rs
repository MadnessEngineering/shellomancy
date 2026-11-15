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

/// A todo's priority, in the order the panel lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Critical,
    High,
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

#[derive(Clone, Debug, PartialEq)]
pub struct Todo {
    pub id: String,
    pub description: String,
    pub project: String,
    pub priority: Priority,
    pub status: String,
    /// Unix seconds.
    pub created_at: i64,
}

/// A quest and how far along it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Quest {
    pub id: String,
    pub name: String,
    pub project: String,
    pub total: u64,
    pub completed: u64,
    pub pct: u64,
    /// The first todo that can be worked on next, if any.
    pub next: Option<NextStep>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NextStep {
    pub todo_id: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lesson {
    pub id: String,
    pub topic: String,
    pub body: String,
    pub tags: Vec<String>,
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
            })
        })
        .collect()
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
                tags: lesson["tags"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|tag| tag.as_str().map(str::to_owned))
                    .collect(),
            })
        })
        .collect()
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

/// The Omnispindle project for a terminal: the focused repo's folder name,
/// else the working directory's, lowercased. `None` means "no project here".
pub fn project_name(repo_folder: Option<&str>, cwd_folder: Option<&str>) -> Option<String> {
    repo_folder
        .or(cwd_folder)
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

/// `POST /api/todos/:id/complete`.
pub fn complete_url(id: &str) -> Url {
    let mut url = Url::parse(&format!("{MADNESS_API_ROOT}/todos"))
        .expect("the API root is a valid URL");
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
