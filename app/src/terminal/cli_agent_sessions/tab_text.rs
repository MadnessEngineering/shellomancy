//! Shellomancy: what a CLI agent tab says about its session.
//!
//! Claude Code titles its terminal `✳ <session uuid> · <name>`, which tells
//! you nothing. An agent tab instead reads `<name> · <the work>`, where the
//! work is the Omnispindle todo the session is on (from the plugin's
//! `mcp__*omnispindle*__*` tool calls) or, without one, the last prompt. Under
//! it, a live activity line says what the session is doing right now.
//!
//! Everything here is pure, so it's tested without a running session.

use super::event::TodoTouch;
use super::{BlockedSource, CLIAgentSessionStatus};

/// How long the work part of a title may get before it's cut.
const WORK_CHARS: usize = 60;
/// How long a command in the activity line may get.
const COMMAND_CHARS: usize = 40;

/// The Omnispindle todo a session is working on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveTodo {
    pub id: String,
    pub description: Option<String>,
    pub done: bool,
}

/// The tool a session last started or finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolActivity {
    pub name: String,
    /// A command, file path or the like, if the plugin sent one.
    pub target: Option<String>,
    /// Started and not yet finished.
    pub running: bool,
}

/// Folds an Omnispindle tool call into the session's active todo.
///
/// - `add_todo` makes the new todo active unless another unfinished one is.
/// - `update_todo` to in_progress makes that todo active; to a finished status
///   (completed, review, done) marks it done; anything else refreshes it if
///   it's the active one, or adopts it when nothing is active.
/// - `complete_todo` marks it done.
/// - `get_todo` adopts it when nothing is active.
/// - Everything else (lists, searches, lessons) changes nothing.
pub fn apply_todo_touch(current: Option<ActiveTodo>, touch: &TodoTouch) -> Option<ActiveTodo> {
    let nothing_active = current.as_ref().is_none_or(|todo| todo.done);
    let adopt = |id: &str, done: bool| {
        let same = current.as_ref().filter(|todo| todo.id == id);
        Some(ActiveTodo {
            id: id.to_owned(),
            description: touch
                .description
                .clone()
                .or_else(|| same.and_then(|todo| todo.description.clone())),
            done,
        })
    };
    let id = touch.todo_id.as_deref();
    match touch.action.as_str() {
        "add_todo" => match touch.result_id.as_deref() {
            Some(new_id) if nothing_active => adopt(new_id, false),
            _ => current,
        },
        "complete_todo" => match id {
            Some(id) if nothing_active || is_current(&current, id) => adopt(id, true),
            _ => current,
        },
        "update_todo" => {
            let Some(id) = id else { return current };
            let status = touch.status.as_deref().map(str::to_lowercase);
            match status.as_deref() {
                Some("completed" | "complete" | "done" | "review") => {
                    if nothing_active || is_current(&current, id) {
                        adopt(id, true)
                    } else {
                        current
                    }
                }
                Some("in_progress") => adopt(id, false),
                _ if is_current(&current, id) || nothing_active => {
                    let done = current
                        .as_ref()
                        .filter(|todo| todo.id == id)
                        .is_some_and(|t| t.done);
                    adopt(id, done)
                }
                _ => current,
            }
        }
        "get_todo" => match id {
            Some(id) if nothing_active => adopt(id, false),
            _ => current,
        },
        _ => current,
    }
}

fn is_current(current: &Option<ActiveTodo>, id: &str) -> bool {
    current.as_ref().is_some_and(|todo| todo.id == id)
}

/// The session's name from Claude Code's terminal title: spinner glyphs and
/// session UUIDs dropped, and Claude Code's generic default title ignored.
/// `None` when nothing meaningful is left.
pub fn clean_session_name(terminal_title: &str) -> Option<String> {
    let parts: Vec<String> = terminal_title
        .split(['·', '|'])
        .map(|part| {
            part.split_whitespace()
                .filter(|word| {
                    !is_uuid(word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-'))
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .map(|part| {
            part.trim_start_matches(|c: char| !c.is_alphanumeric())
                .trim()
                .to_owned()
        })
        .filter(|part| !part.is_empty() && !is_generic_title(part))
        .collect();
    let name = parts.join(" · ");
    (!name.is_empty()).then_some(name)
}

/// Whether `word` is a UUID (8-4-4-4-12 hex).
fn is_uuid(word: &str) -> bool {
    let groups: Vec<&str> = word.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_generic_title(text: &str) -> bool {
    matches!(text.to_lowercase().as_str(), "claude code" | "claude")
}

/// The work part of a title: the active todo (with a check once it's done),
/// else the last prompt, cut short.
pub fn work_text(active_todo: Option<&ActiveTodo>, last_prompt: Option<&str>) -> Option<String> {
    if let Some(todo) = active_todo {
        let text = match &todo.description {
            Some(description) => shorten(description, WORK_CHARS),
            None => madness_themes::tf(
                "shellomancy.agentTab.todoShort",
                &[("id", todo.id.get(..8).unwrap_or(&todo.id))],
            ),
        };
        return Some(if todo.done {
            format!("✓ {text}")
        } else {
            text
        });
    }
    last_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(|prompt| shorten(prompt, WORK_CHARS))
}

/// `<name> · <work>`, or just the name.
pub fn agent_tab_title(name: &str, work: Option<&str>) -> String {
    match work {
        Some(work) => format!("{name} · {work}"),
        None => name.to_owned(),
    }
}

/// What a session is doing, for the activity line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentActivity {
    Running(String),
    Editing(String),
    Reading(String),
    Searching,
    Browsing,
    Delegating,
    Omnispindle,
    Calling(String),
    Using(String),
    Thinking,
    NeedsPermission,
    WaitingForYou,
    Done,
    Failed,
    Cancelled,
}

/// The activity for a session's status and its last tool.
pub fn agent_activity(
    status: &CLIAgentSessionStatus,
    tool: Option<&ToolActivity>,
) -> AgentActivity {
    match status {
        CLIAgentSessionStatus::Blocked {
            source: BlockedSource::PermissionRequest,
            ..
        } => AgentActivity::NeedsPermission,
        CLIAgentSessionStatus::Blocked { .. } => AgentActivity::WaitingForYou,
        CLIAgentSessionStatus::Success => AgentActivity::Done,
        CLIAgentSessionStatus::Failed { .. } => AgentActivity::Failed,
        CLIAgentSessionStatus::Cancelled => AgentActivity::Cancelled,
        CLIAgentSessionStatus::InProgress => match tool.filter(|tool| tool.running) {
            Some(tool) => tool_activity(tool),
            None => AgentActivity::Thinking,
        },
    }
}

fn tool_activity(tool: &ToolActivity) -> AgentActivity {
    let target = tool
        .target
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let file = || {
        target
            .map(|path| path.rsplit('/').next().unwrap_or(path).to_owned())
            .unwrap_or_default()
    };
    match tool.name.as_str() {
        "Bash" | "BashOutput" => match target {
            Some(command) => AgentActivity::Running(shorten(command, COMMAND_CHARS)),
            None => AgentActivity::Using(tool.name.clone()),
        },
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => AgentActivity::Editing(file()),
        "Read" => AgentActivity::Reading(file()),
        "Grep" | "Glob" | "LS" => AgentActivity::Searching,
        "WebFetch" | "WebSearch" => AgentActivity::Browsing,
        "Task" | "Agent" => AgentActivity::Delegating,
        name if name.starts_with("mcp__") && name.contains("omnispindle") => {
            AgentActivity::Omnispindle
        }
        name if name.starts_with("mcp__") => {
            let server = name
                .trim_start_matches("mcp__")
                .split("__")
                .next()
                .unwrap_or(name);
            AgentActivity::Calling(server.to_owned())
        }
        name => AgentActivity::Using(name.to_owned()),
    }
}

/// The activity line, in the active voice.
pub fn activity_text(activity: &AgentActivity) -> String {
    use madness_themes::{t, tf};
    match activity {
        AgentActivity::Running(command) => {
            tf("shellomancy.agentTab.running", &[("command", command)])
        }
        AgentActivity::Editing(file) => tf("shellomancy.agentTab.editing", &[("file", file)]),
        AgentActivity::Reading(file) => tf("shellomancy.agentTab.reading", &[("file", file)]),
        AgentActivity::Searching => t("shellomancy.agentTab.searching").to_owned(),
        AgentActivity::Browsing => t("shellomancy.agentTab.browsing").to_owned(),
        AgentActivity::Delegating => t("shellomancy.agentTab.delegating").to_owned(),
        AgentActivity::Omnispindle => t("shellomancy.agentTab.omnispindle").to_owned(),
        AgentActivity::Calling(tool) => tf("shellomancy.agentTab.calling", &[("tool", tool)]),
        AgentActivity::Using(tool) => tf("shellomancy.agentTab.using", &[("tool", tool)]),
        AgentActivity::Thinking => t("shellomancy.agentTab.thinking").to_owned(),
        AgentActivity::NeedsPermission => t("shellomancy.agentTab.permission").to_owned(),
        AgentActivity::WaitingForYou => t("shellomancy.agentTab.waiting").to_owned(),
        AgentActivity::Done => t("shellomancy.agentTab.done").to_owned(),
        AgentActivity::Failed => t("shellomancy.agentTab.failed").to_owned(),
        AgentActivity::Cancelled => t("shellomancy.agentTab.cancelled").to_owned(),
    }
}

/// The first line of `text`, cut to `max` characters with an ellipsis.
fn shorten(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    match line.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", line[..cut].trim_end()),
        None => line.to_owned(),
    }
}

#[cfg(test)]
#[path = "tab_text_tests.rs"]
mod tests;
