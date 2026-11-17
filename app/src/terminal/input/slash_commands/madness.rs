//! Shellomancy: the Madness slash commands.
//!
//! `/todo` and `/lesson` send what was typed to Inventorium's REST API with the
//! signed-in Madness key (the same requests as a spell's "Make it a todo"),
//! `/quest` reads this repo's active quests, `/map` hands the repo to Code
//! Maps, and `/sync` asks settings sync to run now. Each says how it went in a
//! toast; nothing here runs while signed out except `/map`, which maps locally
//! and only uploads when signed in.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use warpui::{SingletonEntity, ViewContext};

use crate::code_maps::CodeMaps;
use crate::madness_sync::{SettingsSync, SyncStatus};
use crate::mana_bar::{self, Quest};
use crate::search::slash_command_menu::static_commands::MadnessSlash;
use crate::spell_todo::{
    CreatedTodo, TODO_URL_ROOT, first_line, post_lesson, post_todo, project_for, redact,
    truncate_chars,
};
use crate::terminal::input::Input;
use crate::view_components::{DismissibleToast, ToastLink};
use crate::workspace::{ToastStack, WorkspaceAction};

type Toast = DismissibleToast<WorkspaceAction>;

/// How much of a lesson's first line becomes its topic.
const LESSON_TOPIC_CHARS: usize = 100;
const TODO_TAGS: [&str; 3] = ["shellomancy", "slash", "quick"];

/// Splits a leading `!critical`, `!high`, `!medium` or `!low` off a todo and
/// returns the API's priority for it (Medium without one).
pub fn split_priority(text: &str) -> (&'static str, &str) {
    let text = text.trim();
    let (first, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    let priority = match first.to_ascii_lowercase().as_str() {
        "!critical" => "Critical",
        "!high" => "High",
        "!medium" => "Medium",
        "!low" => "Low",
        _ => return ("Medium", text),
    };
    (priority, rest.trim())
}

/// The request body for `POST /api/todos` from `/todo <text>`.
pub fn quick_todo_body(text: &str, cwd: Option<&str>) -> Value {
    let mut text = text.trim().to_owned();
    redact(&mut text);
    let (priority, description) = split_priority(&text);
    json!({
        "description": description,
        "project": project_for(cwd),
        "priority": priority,
        "tags": TODO_TAGS,
        "metadata": {
            "source": "shellomancy",
            "tags": TODO_TAGS,
            "cwd": cwd,
        },
    })
}

/// The request body for `POST /api/lessons` from `/lesson <text>`: the first
/// line is the topic, the whole text the lesson.
pub fn quick_lesson_body(text: &str, cwd: Option<&str>) -> Value {
    let mut text = text.trim().to_owned();
    redact(&mut text);
    json!({
        "topic": truncate_chars(first_line(&text), LESSON_TOPIC_CHARS),
        "lesson_learned": text,
        "language": "general",
        "tags": ["shellomancy", "slash", project_for(cwd)],
    })
}

/// The git repo `cwd` is in, if it's in one.
pub fn repo_root(cwd: &Path) -> Option<&Path> {
    cwd.ancestors().find(|dir| dir.join(".git").exists())
}

/// The quest to show: the first with a step ready to work on, else the first.
pub fn pick_quest(quests: &[Quest]) -> Option<&Quest> {
    quests
        .iter()
        .find(|quest| quest.next.is_some())
        .or_else(|| quests.first())
}

/// The toast text for a quest, and where its next step lives.
pub fn quest_message(quest: &Quest, others: usize) -> (String, Option<String>) {
    let progress = format!("{}/{}", quest.completed, quest.total);
    let mut message = match &quest.next {
        Some(next) => madness_themes::tf(
            "shellomancy.slash.quest.next",
            &[
                ("quest", &quest.name),
                ("progress", &progress),
                ("next", &next.description),
            ],
        ),
        None => madness_themes::tf(
            "shellomancy.slash.quest.noNext",
            &[("quest", &quest.name), ("progress", &progress)],
        ),
    };
    if others > 0 {
        message.push(' ');
        message.push_str(&madness_themes::tf(
            "shellomancy.slash.quest.more",
            &[("count", &others.to_string())],
        ));
    }
    let link = quest
        .next
        .as_ref()
        .map(|next| mana_bar::api::todo_page_url(&next.todo_id));
    (message, link)
}

fn show_toast(toast: Toast, ctx: &mut ViewContext<Input>) {
    let window_id = ctx.window_id();
    ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
        toast_stack.add_ephemeral_toast(toast, window_id, ctx);
    });
}

fn open_link(href: String) -> ToastLink<WorkspaceAction> {
    ToastLink::new(madness_themes::t("shellomancy.spell.openTodo").to_owned()).with_href(href)
}

impl Input {
    fn madness_cwd(&self) -> Option<String> {
        self.active_block_metadata
            .as_ref()
            .and_then(|metadata| metadata.current_working_directory())
            .map(str::to_owned)
    }

    /// The signed-in Madness key, or `None` after saying to sign in.
    fn madness_key(&self, ctx: &mut ViewContext<Self>) -> Option<String> {
        let key = mana_bar::api_key(ctx);
        if key.is_none() {
            show_toast(
                Toast::error(madness_themes::t("shellomancy.spell.signInFirst").to_owned()),
                ctx,
            );
        }
        key
    }

    /// Runs one of the Madness commands. Always handles it.
    pub(super) fn execute_madness_slash(
        &mut self,
        which: MadnessSlash,
        argument: Option<&String>,
        ctx: &mut ViewContext<Self>,
    ) {
        let text = argument
            .map(|text| text.trim())
            .filter(|text| !text.is_empty());
        match which {
            MadnessSlash::Todo => self.madness_todo(text, ctx),
            MadnessSlash::Lesson => self.madness_lesson(text, ctx),
            MadnessSlash::Map => self.madness_map(ctx),
            MadnessSlash::Quest => self.madness_quest(ctx),
            MadnessSlash::Sync => self.madness_sync(ctx),
        }
    }

    fn madness_todo(&mut self, text: Option<&str>, ctx: &mut ViewContext<Self>) {
        let Some(text) = text.filter(|text| !split_priority(text).1.is_empty()) else {
            show_toast(
                Toast::error(madness_themes::t("shellomancy.slash.todo.empty").to_owned()),
                ctx,
            );
            return;
        };
        let Some(key) = self.madness_key(ctx) else {
            return;
        };
        let body = quick_todo_body(text, self.madness_cwd().as_deref());
        let _ = ctx.spawn(
            async move { post_todo(&key, body).await },
            |_me, result: Result<CreatedTodo, String>, ctx| {
                let toast = match result {
                    Ok(todo) => Toast::success(madness_themes::tf(
                        "shellomancy.spell.todoCreated",
                        &[
                            ("project", &todo.project),
                            ("description", &todo.description),
                        ],
                    ))
                    .with_link(open_link(format!("{TODO_URL_ROOT}/{}", todo.id))),
                    Err(message) => Toast::error(message),
                };
                show_toast(toast, ctx);
            },
        );
    }

    fn madness_lesson(&mut self, text: Option<&str>, ctx: &mut ViewContext<Self>) {
        let Some(text) = text else {
            show_toast(
                Toast::error(madness_themes::t("shellomancy.slash.lesson.empty").to_owned()),
                ctx,
            );
            return;
        };
        let Some(key) = self.madness_key(ctx) else {
            return;
        };
        let body = quick_lesson_body(text, self.madness_cwd().as_deref());
        let _ = ctx.spawn(
            async move { post_lesson(&key, body).await },
            |_me, result, ctx| {
                let toast = match result {
                    Ok(lesson) => Toast::success(madness_themes::tf(
                        "shellomancy.spell.lessonCreated",
                        &[("topic", &lesson.topic)],
                    )),
                    Err(message) => Toast::error(message),
                };
                show_toast(toast, ctx);
            },
        );
    }

    fn madness_map(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(cwd) = self.madness_cwd() else {
            show_toast(
                Toast::error(madness_themes::t("shellomancy.slash.map.noFolder").to_owned()),
                ctx,
            );
            return;
        };
        if !ctx.has_singleton_model::<CodeMaps>() {
            return;
        }
        let cwd = PathBuf::from(cwd);
        let folder = repo_root(&cwd).unwrap_or(&cwd);
        let folder = folder
            .canonicalize()
            .unwrap_or_else(|_| folder.to_path_buf());
        let name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| folder.display().to_string());
        let key = if CodeMaps::as_ref(ctx).is_mapping(&folder) {
            "shellomancy.slash.map.alreadyMapping"
        } else {
            CodeMaps::handle(ctx).update(ctx, |maps, ctx| maps.map(folder, ctx));
            "shellomancy.slash.map.started"
        };
        show_toast(
            Toast::default(madness_themes::tf(key, &[("folder", &name)])),
            ctx,
        );
    }

    fn madness_quest(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(key) = self.madness_key(ctx) else {
            return;
        };
        // Outside a repo there's no project, so every project's quests count.
        let project = self
            .madness_cwd()
            .filter(|cwd| repo_root(Path::new(cwd)).is_some())
            .map(|cwd| project_for(Some(&cwd)));
        let request_project = project.clone();
        let _ = ctx.spawn(
            async move { mana_bar::api::fetch_quests(key, request_project).await },
            move |_me, result, ctx| {
                let toast = match result {
                    Ok(quests) => match pick_quest(&quests) {
                        Some(quest) => {
                            let (message, link) = quest_message(quest, quests.len() - 1);
                            let toast = Toast::default(message);
                            match link {
                                Some(href) => toast.with_link(open_link(href)),
                                None => toast,
                            }
                        }
                        None => Toast::default(match &project {
                            Some(project) => madness_themes::tf(
                                "shellomancy.slash.quest.none",
                                &[("project", project)],
                            ),
                            None => {
                                madness_themes::t("shellomancy.slash.quest.noneAnywhere").to_owned()
                            }
                        }),
                    },
                    Err(message) => Toast::error(message),
                };
                show_toast(toast, ctx);
            },
        );
    }

    fn madness_sync(&mut self, ctx: &mut ViewContext<Self>) {
        if !ctx.has_singleton_model::<SettingsSync>() || self.madness_key(ctx).is_none() {
            return;
        }
        SettingsSync::handle(ctx).update(ctx, |sync, ctx| sync.sync_now(ctx));
        // `sync_now` settles on Idle right away when sync and following are off.
        let toast = match SettingsSync::as_ref(ctx).status() {
            SyncStatus::Idle => {
                Toast::error(madness_themes::t("shellomancy.slash.sync.off").to_owned())
            }
            _ => Toast::default(madness_themes::t("shellomancy.slash.sync.started").to_owned()),
        };
        show_toast(toast, ctx);
    }
}

#[cfg(test)]
#[path = "madness_tests.rs"]
mod tests;
