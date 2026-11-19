//! Shellomancy: the Mana Bar, Omnispindle in the tools panel.
//!
//! One model shared by every window's panel: it fetches the signed-in Madness
//! account's active todos, quests and activity log (per project) and recent
//! lessons from
//! Inventorium, keeps the last result on screen while a refresh runs, and
//! drops everything when the account changes. Nothing is fetched while signed
//! out or until a panel asks.

pub mod api;
pub mod git;

use std::collections::HashMap;

use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

pub use self::api::{Lesson, LogEntry, NextStep, Priority, Quest, QuestChain, QuestTodo, Todo};
use crate::madness_account::{MadnessAccount, MadnessAccountState};

/// One of the panel's lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    Todos,
    Quests,
    Lessons,
    Activity,
}

/// A project name, or `None` for all projects.
pub type ProjectKey = Option<String>;

/// One fetched list and where it's at. A refresh keeps the previous data
/// (`stale`) so the panel doesn't blank out while it waits.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Fetch<T> {
    #[default]
    Idle,
    Loading {
        stale: Option<T>,
    },
    Loaded {
        data: T,
    },
    Failed {
        message: String,
        stale: Option<T>,
    },
}

impl<T> Fetch<T> {
    /// The newest data there is, fresh or stale.
    pub fn data(&self) -> Option<&T> {
        match self {
            Fetch::Idle => None,
            Fetch::Loading { stale } | Fetch::Failed { stale, .. } => stale.as_ref(),
            Fetch::Loaded { data } => Some(data),
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Fetch::Loading { .. })
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Fetch::Failed { message, .. } => Some(message),
            _ => None,
        }
    }

    /// Starts a fetch, keeping whatever data there was.
    fn start(&mut self) {
        let stale = match std::mem::take(self) {
            Fetch::Idle => None,
            Fetch::Loading { stale } | Fetch::Failed { stale, .. } => stale,
            Fetch::Loaded { data } => Some(data),
        };
        *self = Fetch::Loading { stale };
    }

    /// Finishes a fetch; a failure keeps the data there was.
    fn finish(&mut self, result: Result<T, String>) {
        *self = match result {
            Ok(data) => Fetch::Loaded { data },
            Err(message) => {
                let stale = match std::mem::take(self) {
                    Fetch::Idle => None,
                    Fetch::Loading { stale } | Fetch::Failed { stale, .. } => stale,
                    Fetch::Loaded { data } => Some(data),
                };
                Fetch::Failed { message, stale }
            }
        };
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ManaBarEvent {
    Changed,
}

#[derive(Default)]
pub struct ManaBar {
    todos: HashMap<ProjectKey, Fetch<Vec<Todo>>>,
    quests: HashMap<ProjectKey, Fetch<Vec<Quest>>>,
    lessons: Fetch<Vec<Lesson>>,
    activity: HashMap<ProjectKey, Fetch<Vec<LogEntry>>>,
    /// Bumped when the account changes, so replies to requests made with the
    /// old key are dropped.
    generation: u64,
}

impl Entity for ManaBar {
    type Event = ManaBarEvent;
}

impl SingletonEntity for ManaBar {}

impl ManaBar {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        if ctx.has_singleton_model::<MadnessAccount>() {
            ctx.subscribe_to_model(&MadnessAccount::handle(ctx), |me, _, _, ctx| {
                me.reset(ctx);
            });
        }
        Self::default()
    }

    pub fn todos(&self, project: &ProjectKey) -> Option<&Fetch<Vec<Todo>>> {
        self.todos.get(project)
    }

    pub fn quests(&self, project: &ProjectKey) -> Option<&Fetch<Vec<Quest>>> {
        self.quests.get(project)
    }

    pub fn lessons(&self) -> &Fetch<Vec<Lesson>> {
        &self.lessons
    }

    pub fn activity(&self, project: &ProjectKey) -> Option<&Fetch<Vec<LogEntry>>> {
        self.activity.get(project)
    }

    /// Fetches one list for `project` if it's never been fetched.
    pub fn ensure_loaded(
        &mut self,
        list: List,
        project: &ProjectKey,
        ctx: &mut ModelContext<Self>,
    ) {
        let never_fetched = match list {
            List::Todos => !self.todos.contains_key(project),
            List::Quests => !self.quests.contains_key(project),
            List::Lessons => matches!(self.lessons, Fetch::Idle),
            List::Activity => !self.activity.contains_key(project),
        };
        if never_fetched {
            self.refresh(list, project, ctx);
        }
    }

    /// Fetches one list for `project` again, unless it's already being
    /// fetched. Only the list on screen is fetched, so opening the panel or a
    /// poll costs one request, not four.
    pub fn refresh(&mut self, list: List, project: &ProjectKey, ctx: &mut ModelContext<Self>) {
        let Some(key) = api_key(ctx) else {
            return;
        };
        let generation = self.generation;
        let (request_project, project) = (project.clone(), project.clone());
        match list {
            List::Todos => {
                let todos = self.todos.entry(project.clone()).or_default();
                if todos.is_loading() {
                    return;
                }
                todos.start();
                let _ = ctx.spawn(
                    api::fetch_todos(key, request_project),
                    move |me, result, ctx| {
                        if me.generation == generation {
                            me.todos.entry(project).or_default().finish(result);
                            me.changed(ctx);
                        }
                    },
                );
            }
            List::Quests => {
                let quests = self.quests.entry(project.clone()).or_default();
                if quests.is_loading() {
                    return;
                }
                quests.start();
                let _ = ctx.spawn(
                    api::fetch_quests(key, request_project),
                    move |me, result, ctx| {
                        if me.generation == generation {
                            me.quests.entry(project).or_default().finish(result);
                            me.changed(ctx);
                        }
                    },
                );
            }
            List::Lessons => {
                if self.lessons.is_loading() {
                    return;
                }
                self.lessons.start();
                let _ = ctx.spawn(api::fetch_lessons(key), move |me, result, ctx| {
                    if me.generation == generation {
                        me.lessons.finish(result);
                        me.changed(ctx);
                    }
                });
            }
            List::Activity => {
                let activity = self.activity.entry(project.clone()).or_default();
                if activity.is_loading() {
                    return;
                }
                activity.start();
                let _ = ctx.spawn(
                    api::fetch_activity(key, request_project),
                    move |me, result, ctx| {
                        if me.generation == generation {
                            me.activity.entry(project).or_default().finish(result);
                            me.changed(ctx);
                        }
                    },
                );
            }
        }
        self.changed(ctx);
    }

    /// Takes a todo off every list after it was marked done.
    pub fn todo_completed(&mut self, id: &str, project: &ProjectKey, ctx: &mut ModelContext<Self>) {
        for fetch in self.todos.values_mut() {
            if let Fetch::Loaded { data } = fetch {
                data.retain(|todo| todo.id != id);
            }
        }
        // Quest progress moved: show it at once (an open quest card stays
        // open), then fetch the real numbers. The log has a new entry; fetch
        // it when it's next looked at.
        for fetch in self.quests.values_mut() {
            if let Fetch::Loaded { data } = fetch {
                api::quest_todo_done(data, id);
            }
        }
        self.activity.remove(project);
        self.refresh(List::Quests, project, ctx);
    }

    fn reset(&mut self, ctx: &mut ModelContext<Self>) {
        self.generation += 1;
        self.todos.clear();
        self.quests.clear();
        self.activity.clear();
        self.lessons = Fetch::Idle;
        self.changed(ctx);
    }

    fn changed(&mut self, ctx: &mut ModelContext<Self>) {
        ctx.emit(ManaBarEvent::Changed);
        ctx.notify();
    }
}

/// The signed-in Madness account's key, if there is one.
pub fn api_key(app: &AppContext) -> Option<String> {
    if !app.has_singleton_model::<MadnessAccount>() {
        return None;
    }
    let account = MadnessAccount::as_ref(app);
    matches!(account.state(), MadnessAccountState::SignedIn { .. })
        .then(|| account.api_key().map(str::to_owned))
        .flatten()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
