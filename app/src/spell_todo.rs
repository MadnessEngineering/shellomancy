//! Shellomancy: turn a spell (block) into an Omnispindle todo or lesson.
//!
//! The block's command, its exit code, where it ran and the tail of its output
//! (with secrets redacted) go to Inventorium's REST API (`POST /api/todos` or
//! `POST /api/lessons`) with the signed-in Madness API key. A todo lands in the
//! project named after the repo the spell ran in; a lesson is tagged with it.
//! Either is ready to be refined on madnessinteractive.cc.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::madness_account::{MADNESS_API_ROOT, error_reason};

pub use crate::madness_account::TODO_URL_ROOT;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// How much of the output's tail to send.
const OUTPUT_TAIL_CHARS: usize = 4000;
/// Lessons carry less output: they're read, not debugged.
const LESSON_OUTPUT_TAIL_CHARS: usize = 2000;
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

/// A recorded lesson: what it's called.
#[derive(Clone, Debug, PartialEq)]
pub struct CreatedLesson {
    pub topic: String,
}

/// The request body for `POST /api/lessons`. The lesson starts as a record of
/// what ran and what came back; the actual lesson gets written on the site.
pub fn lesson_body(spell: &Spell) -> Value {
    let mut command_text = spell.command.trim().to_owned();
    redact(&mut command_text);
    let short_command = truncate_chars(first_line(&command_text), DESCRIPTION_COMMAND_CHARS);
    let topic = match spell.exit_code {
        Some(code) if code != 0 => format!("`{short_command}` failed (exit {code})"),
        _ => format!("`{short_command}`"),
    };
    let project = project_for(spell.cwd.as_deref());
    let mut output = tail_chars(&spell.output, LESSON_OUTPUT_TAIL_CHARS);
    redact(&mut output);
    let mut lesson = String::new();
    if let Some(cwd) = &spell.cwd {
        lesson.push_str(&format!("Ran in {cwd}"));
        if let Some(branch) = &spell.git_branch {
            lesson.push_str(&format!(" on {branch}"));
        }
        if let Some(code) = spell.exit_code {
            lesson.push_str(&format!(", exit {code}"));
        }
        lesson.push_str(".\n\n");
    }
    lesson.push_str(&format!("```\n$ {command_text}\n"));
    if !output.trim().is_empty() {
        lesson.push_str(&format!("{}\n", output.trim_end()));
    }
    lesson.push_str("```");
    json!({
        "topic": topic,
        "lesson_learned": lesson,
        "language": language_for(&command_text),
        "tags": ["shellomancy", "spell", project],
    })
}

/// Records the lesson; returns it, or a message ready to show.
pub async fn create_lesson(key: &str, spell: Spell) -> Result<CreatedLesson, String> {
    let body = lesson_body(&spell);
    let failed =
        |error: String| madness_themes::tf("shellomancy.spell.lessonFailed", &[("error", &error)]);
    let response = http_client::Client::new_external()
        .post(format!("{MADNESS_API_ROOT}/lessons"))
        .bearer_auth(key)
        .timeout(REQUEST_TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|err| failed(err.to_string()))?;
    let status = response.status().as_u16();
    let reply: Value = response.json().await.unwrap_or_default();
    if (200..300).contains(&status) && reply["success"] == true {
        return Ok(CreatedLesson {
            topic: body["topic"].as_str().unwrap_or_default().to_owned(),
        });
    }
    Err(failed(error_reason(status, &reply)))
}

/// The lessons API's language for a command, from the tool it runs.
fn language_for(command: &str) -> &'static str {
    let tool = command
        .split_whitespace()
        .find(|word| !word.contains('='))
        .map(|word| word.rsplit('/').next().unwrap_or(word))
        .unwrap_or_default();
    match tool {
        "cargo" | "rustc" | "rustup" => "rust",
        "python" | "python3" | "pip" | "pip3" | "pytest" | "uv" | "poetry" => "python",
        "node" | "npm" | "npx" | "yarn" | "pnpm" | "bun" | "deno" => "javascript",
        "tsc" => "typescript",
        "go" => "go",
        "java" | "mvn" | "gradle" => "java",
        "psql" | "mysql" | "sqlite3" | "mongosh" => "sql",
        _ => "bash",
    }
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
        _ => Err(madness_themes::tf(
            "shellomancy.spell.todoFailed",
            &[("error", &error_reason(status, reply))],
        )),
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
