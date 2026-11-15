//! Shellomancy: the Mana Bar, Omnispindle in the tools panel.
//!
//! One model shared by every window's panel: it fetches the signed-in Madness
//! account's active todos and quests (per project) and recent lessons from
//! Inventorium, keeps the last result on screen while a refresh runs, and
//! drops everything when the account changes. Nothing is fetched while signed
//! out or until a panel asks.

pub mod api;

use std::collections::HashMap;

use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

pub use self::api::{Lesson, NextStep, Priority, Quest, Todo};
use crate::madness_account::{MadnessAccount, MadnessAccountState};

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

    /// Fetches `project`'s lists if they've never been fetched.
    pub fn ensure_loaded(&mut self, project: &ProjectKey, ctx: &mut ModelContext<Self>) {
        let never_fetched = !self.todos.contains_key(project)
            || !self.quests.contains_key(project)
            || matches!(self.lessons, Fetch::Idle);
        if never_fetched {
            self.refresh(project, ctx);
        }
    }

    /// Fetches `project`'s todos and quests, and the lessons, again. Lists
    /// already being fetched are left alone.
    pub fn refresh(&mut self, project: &ProjectKey, ctx: &mut ModelContext<Self>) {
        let Some(key) = api_key(ctx) else {
            return;
        };
        let generation = self.generation;

        let todos = self.todos.entry(project.clone()).or_default();
        if !todos.is_loading() {
            todos.start();
            let (key, request_project, project) = (key.clone(), project.clone(), project.clone());
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

        let quests = self.quests.entry(project.clone()).or_default();
        if !quests.is_loading() {
            quests.start();
            let (key, request_project, project) = (key.clone(), project.clone(), project.clone());
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

        if !self.lessons.is_loading() {
            self.lessons.start();
            let _ = ctx.spawn(api::fetch_lessons(key), move |me, result, ctx| {
                if me.generation == generation {
                    me.lessons.finish(result);
                    me.changed(ctx);
                }
            });
        }
        self.changed(ctx);
    }

    /// Takes a todo off every list after it was marked done, and fetches the
    /// quests again since their progress moved.
    pub fn todo_completed(&mut self, id: &str, project: &ProjectKey, ctx: &mut ModelContext<Self>) {
        for fetch in self.todos.values_mut() {
            if let Fetch::Loaded { data } = fetch {
                data.retain(|todo| todo.id != id);
            }
        }
        self.refresh(project, ctx);
    }

    fn reset(&mut self, ctx: &mut ModelContext<Self>) {
        self.generation += 1;
        self.todos.clear();
        self.quests.clear();
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
