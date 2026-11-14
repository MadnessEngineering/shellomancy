//! Shellomancy: turn a spell (block) into an Omnispindle todo.
//!
//! The block's command, its exit code, where it ran and the tail of its output
//! (with secrets redacted) go to Inventorium's REST API (`POST /api/todos`) with
//! the signed-in Madness API key. The todo lands in the project named after the
//! repo the spell ran in, ready to be refined on madnessinteractive.cc.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::madness_account::MADNESS_API_ROOT;

/// Where a created todo is opened.
pub const TODO_URL_ROOT: &str = "https://madnessinteractive.cc/todo";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// How much of the output's tail to send.
const OUTPUT_TAIL_CHARS: usize = 4000;
const DESCRIPTION_COMMAND_CHARS: usize = 120;
/// Used when the spell didn't run inside a folder we can name.
const FALLBACK_PROJECT: &str = "shellomancy";

/// What a todo is made from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spell {
    pub command: String,
    pub output: String,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub exit_code: Option<i32>,
}

/// A created todo: its id and what it was called.
#[derive(Clone, Debug, PartialEq)]
pub struct CreatedTodo {
    pub id: String,
    pub description: String,
    pub project: String,
}

/// The request body for `POST /api/todos`.
pub fn todo_body(spell: &Spell) -> Value {
    // Redact before anything is built from the command, the title included.
    let mut command_text = spell.command.trim().to_owned();
    redact(&mut command_text);
    let short_command = truncate_chars(first_line(&command_text), DESCRIPTION_COMMAND_CHARS);
    let failed = spell.exit_code.is_some_and(|code| code != 0);
    let description = match spell.exit_code {
        Some(code) if code != 0 => format!("`{short_command}` failed (exit {code})"),
        _ => format!("Follow up on `{short_command}`"),
    };
    let project = project_for(spell.cwd.as_deref());
    let tags = vec![
        "shellomancy",
        "spell",
        if failed { "failed" } else { "follow-up" },
    ];
    let mut output = tail_chars(&spell.output, OUTPUT_TAIL_CHARS);
    redact(&mut output);
    json!({
        "description": description,
        "project": project,
        "priority": if failed { "High" } else { "Medium" },
        "tags": tags,
        "metadata": {
            "source": "shellomancy",
            "tags": tags,
            "command": command_text,
            "cwd": spell.cwd,
            "git_branch": spell.git_branch,
            "exit_code": spell.exit_code,
            "output_tail": output,
        },
    })
}

/// Creates the todo; returns it, or a message ready to show.
pub async fn create_todo(key: &str, spell: Spell) -> Result<CreatedTodo, String> {
    let body = todo_body(&spell);
    let response = http_client::Client::new_external()
        .post(format!("{MADNESS_API_ROOT}/todos"))
        .bearer_auth(key)
        .timeout(REQUEST_TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|err| {
            madness_themes::tf(
                "shellomancy.spell.todoFailed",
                &[("error", &err.to_string())],
            )
        })?;
    let status = response.status().as_u16();
    let reply: Value = response.json().await.unwrap_or_default();
    created_from_reply(status, &reply, &body)
}

fn created_from_reply(status: u16, reply: &Value, body: &Value) -> Result<CreatedTodo, String> {
    let id = reply["todo_id"]
        .as_str()
        .or_else(|| reply["todo"]["id"].as_str());
    match id {
        Some(id) if (200..300).contains(&status) => Ok(CreatedTodo {
            id: id.to_owned(),
            description: body["description"].as_str().unwrap_or_default().to_owned(),
            project: body["project"].as_str().unwrap_or_default().to_owned(),
        }),
        _ => {
            let reason = reply["message"]
                .as_str()
                .or_else(|| reply["details"].as_str())
                .or_else(|| reply["error"].as_str())
                .filter(|message| !message.is_empty())
                .map(|message| format!("{message} (HTTP {status})"))
                .unwrap_or_else(|| format!("HTTP {status}"));
            Err(madness_themes::tf(
                "shellomancy.spell.todoFailed",
                &[("error", &reason)],
            ))
        }
    }
}

/// The lowercase name of the git repo `cwd` is in, else of `cwd` itself.
fn project_for(cwd: Option<&str>) -> String {
    let Some(cwd) = cwd.map(Path::new) else {
        return FALLBACK_PROJECT.to_owned();
    };
    let root = cwd
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(cwd);
    root.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| FALLBACK_PROJECT.to_owned())
}

/// Blanks out anything that looks like a secret.
fn redact(text: &mut String) {
    crate::server::telemetry::secret_redaction::redact_secrets_in_string(text);
    secret_redaction::redact_secrets(text);
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

fn truncate_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

fn tail_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_owned();
    }
    let skip = count - max;
    let start = text
        .char_indices()
        .nth(skip)
        .map_or(text.len(), |(index, _)| index);
    format!("…{}", &text[start..])
}

#[cfg(test)]
#[path = "spell_todo_tests.rs"]
mod tests;
