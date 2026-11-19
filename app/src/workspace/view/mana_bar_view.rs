//! Shellomancy: the Mana Bar tab of the tools panel (Drive's old slot).
//!
//! Shows the signed-in Madness account's active todos and quests for the
//! project the focused terminal is in (or every project), and recent lessons
//! with a filter. Clicking a card unrolls it: a terminal-style block of the
//! item's fields (ids with copy buttons), its notes or body, what an update
//! changed, and a link to it on madnessinteractive.cc. Todos can be marked
//! done. The data lives in [`ManaBar`], shared by every window; this view
//! only decides which project to ask about and polls while it's on screen.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ::settings::Setting;
use parking_lot::RwLock;

use pathfinder_color::ColorU;
use warp_core::ui::Icon;
use warpui::r#async::Timer;
use warpui::clipboard::ClipboardContent;
use warpui::elements::{
    Border, ChildView, Clipped, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox,
    Container, CornerRadius, CrossAxisAlignment, Element, Empty, Expanded, Fill, Flex, Hoverable,
    MainAxisAlignment, MainAxisSize, MouseStateHandle, Padding, ParentElement, Radius,
    ScrollbarWidth, SelectableArea, SelectionHandle, Shrinkable, Wrap,
};
use warpui::fonts::Weight;
use warpui::keymap::FixedBinding;
use warpui::platform::Cursor;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle};

use crate::appearance::Appearance;
use crate::editor::{
    EditorView, Event as EditorEvent, PropagateAndNoOpNavigationKeys,
    PropagateHorizontalNavigationKeys, SingleLineEditorOptions, TextOptions,
};
use crate::mana_bar::api::{self, DASHBOARD_URL};
use crate::mana_bar::git;
use crate::mana_bar::{Fetch, List, ManaBar, ManaBarEvent, ProjectKey};
use crate::settings::madness_sync::{MadnessSyncSettings, TodoOpenTarget};
use crate::settings_view::SettingsSection;
use crate::ui_components::buttons::icon_button;
use crate::util::bindings::CustomAction;
use crate::view_components::DismissibleToast;
use crate::workspace::{ToastStack, WorkspaceAction};

/// How often an open panel fetches again.
const POLL_INTERVAL: Duration = Duration::from_secs(60);
const SCROLLBAR_WIDTH: ScrollbarWidth = ScrollbarWidth::Auto;
const SECTION_GAP: f32 = 16.;
/// Cards rendered per tab before "Show more"; rendering hundreds of cards
/// every frame is what made the panel lag.
const PAGE_SIZE: usize = 30;
/// Tag chips shown per card; the rest are on the site.
const MAX_TAGS: usize = 5;
/// How long a copy button says "copied".
const COPIED_FLASH: Duration = Duration::from_millis(1500);
/// The longest field value an unrolled card shows.
const MAX_DETAIL_CHARS: usize = 600;
/// Width of the field-name column in an unrolled card.
const FIELD_COLUMN_WIDTH: f32 = 96.;
/// The buttons an unrolled card can have, keyed `<card>|<slot>`.
/// `head` is the summary a card rolls up from; the rest are buttons.
const BUTTON_SLOTS: [&str; 7] = ["id", "ref", "text", "open", "done", "all", "head"];

/// Which list the panel shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ManaBarTab {
    #[default]
    Todos,
    Quests,
    Scrolls,
    Log,
}

impl ManaBarTab {
    const ALL: [ManaBarTab; 4] = [
        ManaBarTab::Todos,
        ManaBarTab::Quests,
        ManaBarTab::Scrolls,
        ManaBarTab::Log,
    ];

    /// The list this tab shows.
    fn list(self) -> List {
        match self {
            ManaBarTab::Todos => List::Todos,
            ManaBarTab::Quests => List::Quests,
            ManaBarTab::Scrolls => List::Lessons,
            ManaBarTab::Log => List::Activity,
        }
    }

    fn voice_key(self) -> &'static str {
        match self {
            ManaBarTab::Todos => "shellomancy.manaBar.tabTodos",
            ManaBarTab::Quests => "shellomancy.manaBar.tabQuests",
            ManaBarTab::Scrolls => "shellomancy.manaBar.tabScrolls",
            ManaBarTab::Log => "shellomancy.manaBar.tabLog",
        }
    }
}

/// Which log entries the Log tab shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogFilter {
    #[default]
    All,
    Created,
    Updated,
    Completed,
    /// Only the focused repo's git commits.
    Commits,
}

impl LogFilter {
    const ALL: [LogFilter; 5] = [
        LogFilter::All,
        LogFilter::Created,
        LogFilter::Updated,
        LogFilter::Completed,
        LogFilter::Commits,
    ];

    fn voice_key(self) -> &'static str {
        match self {
            LogFilter::All => "shellomancy.manaBar.logAll",
            LogFilter::Created => "shellomancy.manaBar.logCreated",
            LogFilter::Updated => "shellomancy.manaBar.logUpdated",
            LogFilter::Completed => "shellomancy.manaBar.logCompleted",
            LogFilter::Commits => "shellomancy.manaBar.logCommits",
        }
    }

    fn matches(self, operation: &str) -> bool {
        match self {
            LogFilter::All => true,
            LogFilter::Created => operation == "create",
            LogFilter::Updated => operation == "update",
            LogFilter::Completed => operation == "complete",
            LogFilter::Commits => false,
        }
    }

    fn shows_commits(self) -> bool {
        matches!(self, LogFilter::All | LogFilter::Commits)
    }
}

#[derive(Clone, Debug)]
pub enum ManaBarAction {
    SelectTab(ManaBarTab),
    ShowMore,
    SetLogFilter(LogFilter),
    Refresh,
    ToggleAllProjects,
    MarkDone(String),
    OpenUrl(String),
    /// Unrolls or rolls up the card with this key (`todo:<id>`, ...).
    ToggleCard(String),
    /// Copies `text`; `button` (`<card>|<slot>`) says "copied" for a moment.
    Copy {
        button: String,
        text: String,
    },
    /// Unrolls (or rolls up) a quest row's todo in place, fetching it.
    ToggleTodoRow {
        row: String,
        id: String,
    },
    /// Cmd-C: copies the text selected in the panel.
    CopySelectedText,
    /// A selection started; take focus so cmd-C reaches the panel.
    Focus,
}

pub fn init(app: &mut AppContext) {
    use warpui::keymap::macros::*;

    app.register_fixed_bindings([FixedBinding::custom(
        CustomAction::Copy,
        ManaBarAction::CopySelectedText,
        "Copy",
        id!(ManaBarView::ui_name()) & !id!("IMEOpen"),
    )]);
}

#[derive(Default)]
struct HeaderMouseStates {
    refresh: MouseStateHandle,
    scope: MouseStateHandle,
    account: MouseStateHandle,
    tabs: [MouseStateHandle; 4],
    show_more: MouseStateHandle,
    log_filters: [MouseStateHandle; LogFilter::ALL.len()],
}

pub struct ManaBarView {
    /// The focused terminal's project, if it's in one.
    project: Option<String>,
    all_projects: bool,
    tab: ManaBarTab,
    log_filter: LogFilter,
    /// How many cards the current tab shows; grows with "Show more".
    shown: usize,
    lesson_filter: ViewHandle<EditorView>,
    lesson_query: String,
    /// Keys of the unrolled cards.
    expanded: HashSet<String>,
    /// The copy button that just copied, for its "copied" flash.
    copied: Option<String>,
    copy_generation: u64,
    scroll: ClippedScrollStateHandle,
    /// Text selected by dragging across the panel, for cmd-C.
    selection: SelectionHandle,
    selected_text: Arc<RwLock<Option<String>>>,
    header_mouse: HeaderMouseStates,
    /// Per-row mouse states, keyed like `todo:<id>`.
    row_mouse: HashMap<String, MouseStateHandle>,
    completing: HashSet<String>,
    /// Where todos open (Settings > Account), kept in step with the setting.
    open_target: TodoOpenTarget,
    /// The focused terminal's repo, where an agent tab starts.
    repo_root: Option<PathBuf>,
    /// Quest rows unrolled in place, keyed like their buttons (`qt:<q>:<t>`).
    open_rows: HashSet<String>,
    /// Todos fetched for rows unrolled in place: `None` while loading.
    todo_details: HashMap<String, Option<Result<api::Todo, String>>>,
    /// Whether the Log mixes in git commits (Settings > Account).
    log_commits: bool,
    /// The focused repo's recent commits, once read.
    commits: Option<Result<git::RepoCommits, String>>,
    commits_loading: bool,
    /// What each unrolled commit changed: `None` while loading.
    commit_files: HashMap<String, Option<Result<String, String>>>,
    visible: bool,
    /// Bumped to cancel a scheduled poll.
    poll_generation: u64,
}

impl ManaBarView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let lesson_filter = ctx.add_typed_action_view(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let mut editor = EditorView::single_line(
                SingleLineEditorOptions {
                    text: TextOptions::ui_text(Some(13.), appearance),
                    select_all_on_focus: true,
                    clear_selections_on_blur: true,
                    propagate_and_no_op_vertical_navigation_keys:
                        PropagateAndNoOpNavigationKeys::Always,
                    propagate_horizontal_navigation_keys: PropagateHorizontalNavigationKeys::Always,
                    ..Default::default()
                },
                ctx,
            );
            editor
                .set_placeholder_text(madness_themes::t("shellomancy.manaBar.lessonsFilter"), ctx);
            editor
        });
        ctx.subscribe_to_view(&lesson_filter, |me, editor, event, ctx| {
            if let EditorEvent::Edited(_) = event {
                me.lesson_query = editor.as_ref(ctx).buffer_text(ctx);
                ctx.notify();
            }
        });
        if ctx.has_singleton_model::<ManaBar>() {
            ctx.subscribe_to_model(&ManaBar::handle(ctx), |me, _, event, ctx| match event {
                ManaBarEvent::Changed => {
                    me.sync_row_mouse_states(ctx);
                    // The account changed (or first sign-in): fetch again.
                    if me.visible {
                        me.ensure_loaded(ctx);
                    }
                    ctx.notify();
                }
            });
        }
        let (open_target, log_commits) = if ctx.has_singleton_model::<MadnessSyncSettings>() {
            ctx.subscribe_to_model(&MadnessSyncSettings::handle(ctx), |me, settings, _, ctx| {
                let settings = settings.as_ref(ctx);
                let (target, log_commits) = (
                    *settings.todo_open_target.value(),
                    *settings.log_commits.value(),
                );
                if me.open_target != target || me.log_commits != log_commits {
                    me.open_target = target;
                    me.log_commits = log_commits;
                    me.ensure_commits(ctx);
                    ctx.notify();
                }
            });
            let settings = MadnessSyncSettings::as_ref(ctx);
            (
                *settings.todo_open_target.value(),
                *settings.log_commits.value(),
            )
        } else {
            (TodoOpenTarget::default(), true)
        };
        Self {
            project: None,
            all_projects: false,
            tab: ManaBarTab::default(),
            log_filter: LogFilter::default(),
            shown: PAGE_SIZE,
            lesson_filter,
            lesson_query: String::new(),
            expanded: HashSet::new(),
            copied: None,
            copy_generation: 0,
            scroll: Default::default(),
            selection: SelectionHandle::default(),
            selected_text: Default::default(),
            header_mouse: Default::default(),
            row_mouse: HashMap::new(),
            completing: HashSet::new(),
            open_target,
            repo_root: None,
            open_rows: HashSet::new(),
            todo_details: HashMap::new(),
            log_commits,
            commits: None,
            commits_loading: false,
            commit_files: HashMap::new(),
            visible: false,
            poll_generation: 0,
        }
    }

    /// The project the focused terminal is in changed.
    pub fn set_project(&mut self, project: Option<String>, ctx: &mut ViewContext<Self>) {
        if self.project == project {
            return;
        }
        self.project = project;
        self.shown = PAGE_SIZE;
        if self.visible {
            self.ensure_loaded(ctx);
        }
        ctx.notify();
    }

    /// The focused terminal's repo folder changed: an agent tab starts there,
    /// and the Log shows its commits.
    pub fn set_repo_root(&mut self, root: Option<PathBuf>, ctx: &mut ViewContext<Self>) {
        if self.repo_root == root {
            return;
        }
        self.repo_root = root;
        self.ensure_commits(ctx);
        ctx.notify();
    }

    /// Reads the repo's commits if the Log is showing and they're missing
    /// or from another repo.
    fn ensure_commits(&mut self, ctx: &mut ViewContext<Self>) {
        let current = match (&self.commits, &self.repo_root) {
            (Some(Ok(commits)), Some(root)) => commits.root == *root,
            _ => false,
        };
        if !current {
            self.refresh_commits(ctx);
        }
    }

    /// Reads the repo's commits again, when the Log is on screen and wants them.
    fn refresh_commits(&mut self, ctx: &mut ViewContext<Self>) {
        if !self.log_commits || !self.visible || self.tab != ManaBarTab::Log {
            return;
        }
        let Some(root) = self.repo_root.clone() else {
            self.commits = None;
            return;
        };
        if self.commits_loading {
            return;
        }
        self.commits_loading = true;
        let _ = ctx.spawn(git::fetch_commits(root.clone()), move |me, result, ctx| {
            me.commits_loading = false;
            // The repo changed while reading: read the new one.
            if me.repo_root.as_ref() != Some(&root) {
                me.ensure_commits(ctx);
                return;
            }
            if let Ok(repo) = &result {
                for commit in &repo.commits {
                    let card = commit_card_key(commit);
                    me.row_mouse.entry(format!("{card}|head")).or_default();
                    me.row_mouse.entry(card).or_default();
                }
            }
            me.commits = Some(result);
            ctx.notify();
        });
    }

    /// Reads what a commit changed, for its unrolled card.
    fn fetch_commit_files(&mut self, sha: &str, ctx: &mut ViewContext<Self>) {
        if self.commit_files.contains_key(sha) {
            return;
        }
        let Some(Ok(repo)) = &self.commits else {
            return;
        };
        self.commit_files.insert(sha.to_owned(), None);
        let sha = sha.to_owned();
        let _ = ctx.spawn(
            git::fetch_files(repo.root.clone(), sha.clone()),
            move |me, result, ctx| {
                me.commit_files.insert(sha, Some(result));
                ctx.notify();
            },
        );
    }

    /// The panel is shown or hidden; it only fetches and polls while shown.
    /// Shows `tab`, from the first page.
    pub fn select_tab(&mut self, tab: ManaBarTab, ctx: &mut ViewContext<Self>) {
        self.tab = tab;
        self.shown = PAGE_SIZE;
        self.ensure_loaded(ctx);
        ctx.notify();
    }

    pub fn set_visible(&mut self, visible: bool, ctx: &mut ViewContext<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        self.poll_generation += 1;
        if visible {
            self.ensure_loaded(ctx);
            self.schedule_poll(ctx);
        }
    }

    pub fn on_left_panel_focused(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.focus_self();
    }

    fn project_key(&self) -> ProjectKey {
        if self.all_projects {
            None
        } else {
            self.project.clone()
        }
    }

    fn ensure_loaded(&mut self, ctx: &mut ViewContext<Self>) {
        if !ctx.has_singleton_model::<ManaBar>() {
            return;
        }
        let (list, key) = (self.tab.list(), self.project_key());
        ManaBar::handle(ctx).update(ctx, |mana_bar, ctx| mana_bar.ensure_loaded(list, &key, ctx));
        self.ensure_commits(ctx);
    }

    fn refresh(&mut self, ctx: &mut ViewContext<Self>) {
        if !ctx.has_singleton_model::<ManaBar>() {
            return;
        }
        let (list, key) = (self.tab.list(), self.project_key());
        ManaBar::handle(ctx).update(ctx, |mana_bar, ctx| mana_bar.refresh(list, &key, ctx));
        self.refresh_commits(ctx);
    }

    fn schedule_poll(&mut self, ctx: &mut ViewContext<Self>) {
        let generation = self.poll_generation;
        let _ = ctx.spawn(Timer::after(POLL_INTERVAL), move |me, _, ctx| {
            if me.visible && me.poll_generation == generation {
                me.refresh(ctx);
                me.schedule_poll(ctx);
            }
        });
    }

    /// Gives every row a mouse state that lasts across renders, so hovering
    /// works.
    fn sync_row_mouse_states(&mut self, ctx: &mut ViewContext<Self>) {
        let mana_bar = ManaBar::as_ref(ctx);
        let todos = mana_bar
            .todos(&self.project_key())
            .and_then(Fetch::data)
            .into_iter()
            .flatten()
            .flat_map(|todo| [format!("todo:{}", todo.id), format!("done:{}", todo.id)]);
        let quests = mana_bar
            .quests(&self.project_key())
            .and_then(Fetch::data)
            .into_iter()
            .flatten()
            .flat_map(|quest| {
                let card = format!("quest:{}", quest.id);
                // An unrolled quest's todos have buttons of their own.
                let todos: Vec<String> = if self.expanded.contains(&card) {
                    quest
                        .chains
                        .iter()
                        .flat_map(|chain| &chain.todos)
                        .flat_map(|todo| {
                            let row = quest_todo_key(&quest.id, &todo.id);
                            let detail = quest_detail_key(&quest.id, &todo.id);
                            BUTTON_SLOTS
                                .into_iter()
                                .flat_map(move |slot| {
                                    [format!("{row}|{slot}"), format!("{detail}|{slot}")]
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                std::iter::once(card).chain(todos)
            });
        let lessons = mana_bar
            .lessons()
            .data()
            .into_iter()
            .flatten()
            .map(|lesson| format!("lesson:{}", lesson.id));
        let log = mana_bar
            .activity(&self.project_key())
            .and_then(Fetch::data)
            .into_iter()
            .flatten()
            .map(log_card_key);
        let keys: Vec<String> = todos.chain(quests).chain(lessons).chain(log).collect();
        for key in keys {
            // A card's summary has its own mouse state: only it rolls the card.
            self.row_mouse.entry(format!("{key}|head")).or_default();
            self.row_mouse.entry(key).or_default();
        }
    }

    fn row_mouse(&self, key: &str) -> MouseStateHandle {
        self.row_mouse.get(key).cloned().unwrap_or_default()
    }

    /// Fetches a todo for a row unrolled in place.
    fn fetch_todo_detail(&mut self, id: &str, ctx: &mut ViewContext<Self>) {
        if matches!(self.todo_details.get(id), Some(None)) {
            return;
        }
        let Some(key) = crate::mana_bar::api_key(ctx) else {
            return;
        };
        self.todo_details.insert(id.to_owned(), None);
        let id = id.to_owned();
        let _ = ctx.spawn(api::fetch_todo(key, id.clone()), move |me, result, ctx| {
            me.todo_details.insert(id, Some(result));
            ctx.notify();
        });
    }

    fn mark_done(&mut self, id: &str, ctx: &mut ViewContext<Self>) {
        if self.completing.contains(id) {
            return;
        }
        let Some(key) = crate::mana_bar::api_key(ctx) else {
            return;
        };
        let project = self.project_key();
        let mana_bar = ManaBar::as_ref(ctx);
        let description = mana_bar
            .todos(&project)
            .and_then(Fetch::data)
            .and_then(|todos| todos.iter().find(|todo| todo.id == id))
            .map(|todo| todo.description.clone())
            .or_else(|| {
                // Marked done from a quest card.
                mana_bar
                    .quests(&project)
                    .and_then(Fetch::data)
                    .into_iter()
                    .flatten()
                    .flat_map(|quest| &quest.chains)
                    .flat_map(|chain| &chain.todos)
                    .find(|todo| todo.id == id)
                    .map(|todo| todo.description.clone())
            })
            .unwrap_or_default();
        self.completing.insert(id.to_owned());
        ctx.notify();
        let id = id.to_owned();
        let window_id = ctx.window_id();
        let _ = ctx.spawn(
            api::complete_todo(key, id.clone()),
            move |me, result, ctx| {
                me.completing.remove(&id);
                let toast = match result {
                    Ok(()) => {
                        // An unrolled row shows the status as it was; fetch again.
                        if me.todo_details.remove(&id).is_some()
                            && me
                                .open_rows
                                .iter()
                                .any(|row| row.ends_with(&format!(":{id}")))
                        {
                            me.fetch_todo_detail(&id, ctx);
                        }
                        ManaBar::handle(ctx).update(ctx, |mana_bar, ctx| {
                            mana_bar.todo_completed(&id, &project, ctx)
                        });
                        DismissibleToast::success(madness_themes::tf(
                            "shellomancy.manaBar.done",
                            &[("description", &description)],
                        ))
                    }
                    Err(error) => DismissibleToast::error(madness_themes::tf(
                        "shellomancy.manaBar.doneFailed",
                        &[("error", &error)],
                    )),
                };
                ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                    toast_stack.add_ephemeral_toast(toast, window_id, ctx);
                });
                ctx.notify();
            },
        );
    }
}

impl Entity for ManaBarView {
    type Event = ();
}

impl TypedActionView for ManaBarView {
    type Action = ManaBarAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            ManaBarAction::SelectTab(tab) => self.select_tab(*tab, ctx),
            ManaBarAction::SetLogFilter(filter) => {
                self.log_filter = *filter;
                self.shown = PAGE_SIZE;
                ctx.notify();
            }
            ManaBarAction::ShowMore => {
                self.shown += PAGE_SIZE;
                ctx.notify();
            }
            ManaBarAction::Refresh => self.refresh(ctx),
            ManaBarAction::ToggleAllProjects => {
                self.all_projects = !self.all_projects;
                self.shown = PAGE_SIZE;
                self.ensure_loaded(ctx);
                self.sync_row_mouse_states(ctx);
                ctx.notify();
            }
            ManaBarAction::MarkDone(id) => self.mark_done(id, ctx),
            ManaBarAction::OpenUrl(url) => ctx.open_url(url),
            ManaBarAction::ToggleCard(card) => {
                if !self.expanded.remove(card) {
                    for slot in BUTTON_SLOTS {
                        self.row_mouse.entry(format!("{card}|{slot}")).or_default();
                    }
                    self.expanded.insert(card.clone());
                    if card.starts_with("quest:") {
                        self.sync_row_mouse_states(ctx);
                    }
                    if let Some(sha) = card.strip_prefix("commit:") {
                        self.fetch_commit_files(sha, ctx);
                    }
                }
                ctx.notify();
            }
            ManaBarAction::ToggleTodoRow { row, id } => {
                if !self.open_rows.remove(row) {
                    self.open_rows.insert(row.clone());
                    self.fetch_todo_detail(id, ctx);
                }
                ctx.notify();
            }
            ManaBarAction::CopySelectedText => {
                if let Some(text) = self.selected_text.read().clone() {
                    ctx.clipboard().write(ClipboardContent::plain_text(text));
                }
            }
            ManaBarAction::Focus => ctx.focus_self(),
            ManaBarAction::Copy { button, text } => {
                ctx.clipboard()
                    .write(ClipboardContent::plain_text(text.clone()));
                self.copied = Some(button.clone());
                self.copy_generation += 1;
                let generation = self.copy_generation;
                let _ = ctx.spawn(Timer::after(COPIED_FLASH), move |me, _, ctx| {
                    if me.copy_generation == generation {
                        me.copied = None;
                        ctx.notify();
                    }
                });
                ctx.notify();
            }
        }
    }
}

impl View for ManaBarView {
    fn ui_name() -> &'static str {
        "ManaBarView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        if crate::mana_bar::api_key(app).is_none() || !app.has_singleton_model::<ManaBar>() {
            return self.render_signed_out(appearance);
        }
        let mana_bar = ManaBar::as_ref(app);
        let key = self.project_key();

        let mut body = Flex::column().with_main_axis_size(MainAxisSize::Min);
        body.add_child(match self.tab {
            ManaBarTab::Todos => self.render_todos(mana_bar.todos(&key), appearance),
            ManaBarTab::Quests => self.render_quests(mana_bar.quests(&key), appearance),
            ManaBarTab::Scrolls => self.render_lessons(mana_bar.lessons(), appearance),
            ManaBarTab::Log => self.render_log(mana_bar.activity(&key), appearance),
        });

        let theme = appearance.theme();
        let scrollable = ClippedScrollable::vertical(
            self.scroll.clone(),
            Container::new(body.finish())
                .with_horizontal_padding(12.)
                .with_padding_bottom(12.)
                .finish(),
            SCROLLBAR_WIDTH,
            theme.disabled_text_color(theme.background()).into(),
            theme.main_text_color(theme.background()).into(),
            Fill::None,
        )
        .finish();
        // Drag across any card to select its text; cmd-C copies it.
        let selected_text = self.selected_text.clone();
        let scrollable = SelectableArea::new(
            self.selection.clone(),
            move |selection_args, _, _| {
                *selected_text.write() = selection_args.selection.filter(|s| !s.is_empty());
            },
            scrollable,
        )
        .on_selection_updated(|ctx, _| ctx.dispatch_typed_action(ManaBarAction::Focus))
        .finish();

        Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_child(self.render_header(appearance))
            .with_child(self.render_tabs(mana_bar, &key, appearance))
            .with_child(Shrinkable::new(1.0, scrollable).finish())
            .finish()
    }
}

impl ManaBarView {
    fn render_header(&self, appearance: &Appearance) -> Box<dyn Element> {
        let ui = appearance.ui_builder();
        let theme = appearance.theme();
        let scope_label = if self.all_projects {
            madness_themes::t("shellomancy.manaBar.thisProject")
        } else {
            madness_themes::t("shellomancy.manaBar.allProjects")
        };
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Shrinkable::new(
                    1.,
                    ui.span(madness_themes::t("shellomancy.manaBar.title").to_owned())
                        .with_style(UiComponentStyles {
                            font_weight: Some(Weight::Semibold),
                            ..Default::default()
                        })
                        .build()
                        .finish(),
                )
                .finish(),
            );
        // Without a project there's nothing to narrow to.
        if self.project.is_some() {
            row.add_child(
                Container::new(
                    ui.button(ButtonVariant::Secondary, self.header_mouse.scope.clone())
                        .with_text_label(scope_label.to_owned())
                        .build()
                        .on_click(|ctx, _, _| {
                            ctx.dispatch_typed_action(ManaBarAction::ToggleAllProjects);
                        })
                        .finish(),
                )
                .with_margin_right(4.)
                .finish(),
            );
        }
        let refresh_tooltip = ui
            .tool_tip(madness_themes::t("shellomancy.manaBar.refresh").to_owned())
            .build()
            .finish();
        let icon_color = theme.sub_text_color(theme.background()).into_solid();
        row.add_child(
            icon_button(
                appearance,
                Icon::Refresh,
                false,
                self.header_mouse.refresh.clone(),
            )
            .with_tooltip(move || refresh_tooltip)
            .with_style(UiComponentStyles {
                font_color: Some(icon_color),
                height: Some(24.),
                width: Some(24.),
                padding: Some(Coords::uniform(4.)),
                ..Default::default()
            })
            .build()
            .on_click(|ctx, _, _| ctx.dispatch_typed_action(ManaBarAction::Refresh))
            .with_cursor(Cursor::PointingHand)
            .finish(),
        );
        Container::new(row.finish())
            .with_horizontal_padding(12.)
            .with_vertical_padding(6.)
            .finish()
    }

    /// Todos · Quests · Scrolls · Log, each with its count once loaded.
    fn render_tabs(
        &self,
        mana_bar: &ManaBar,
        key: &ProjectKey,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let count = |tab: ManaBarTab| match tab {
            ManaBarTab::Todos => mana_bar.todos(key).and_then(Fetch::data).map(Vec::len),
            ManaBarTab::Quests => mana_bar.quests(key).and_then(Fetch::data).map(Vec::len),
            ManaBarTab::Scrolls => mana_bar.lessons().data().map(Vec::len),
            ManaBarTab::Log => None,
        };
        let mut row = Wrap::row().with_run_spacing(6.);
        for (tab, mouse) in ManaBarTab::ALL.iter().zip(&self.header_mouse.tabs) {
            let mut label = madness_themes::t(tab.voice_key()).to_uppercase();
            if let Some(count) = count(*tab) {
                label = format!("{label} {count}");
            }
            row.add_child(toggle_chip(
                label,
                *tab == self.tab,
                mouse.clone(),
                ManaBarAction::SelectTab(*tab),
                appearance,
            ));
        }
        Container::new(row.finish())
            .with_horizontal_padding(12.)
            .with_padding_bottom(6.)
            .finish()
    }

    fn render_log(
        &self,
        fetch: Option<&Fetch<Vec<api::LogEntry>>>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let ui = appearance.ui_builder();
        let heading = match (&self.project, self.all_projects) {
            (Some(project), false) => madness_themes::tf(
                "shellomancy.manaBar.logHeading",
                &[("project", &api::project_label(project))],
            ),
            _ => madness_themes::t("shellomancy.manaBar.logHeadingAll").to_owned(),
        };
        let entries = fetch.and_then(Fetch::data);
        let mut section = Flex::column().with_child(section_heading(heading, None, appearance));
        let mut filters = Wrap::row().with_run_spacing(6.);
        for (filter, mouse) in LogFilter::ALL.iter().zip(&self.header_mouse.log_filters) {
            if *filter == LogFilter::Commits && !self.log_commits {
                continue;
            }
            filters.add_child(toggle_chip(
                madness_themes::t(filter.voice_key()).to_owned(),
                *filter == self.log_filter,
                mouse.clone(),
                ManaBarAction::SetLogFilter(*filter),
                appearance,
            ));
        }
        section.add_child(
            Container::new(filters.finish())
                .with_margin_bottom(8.)
                .finish(),
        );
        if let Some(status) = fetch_status(fetch, appearance) {
            section.add_child(status);
        }
        let entries: &[api::LogEntry] = entries.map(Vec::as_slice).unwrap_or(&[]);
        let commits: &[git::Commit] = match &self.commits {
            Some(Ok(repo)) if self.log_commits && self.log_filter.shows_commits() => &repo.commits,
            _ => &[],
        };
        if self.log_commits
            && self.log_filter.shows_commits()
            && let Some(Err(error)) = &self.commits
        {
            section.add_child(muted(
                madness_themes::tf("shellomancy.manaBar.commitsFailed", &[("error", error)]),
                appearance,
            ));
        }
        // One timeline, newest first: todo activity and commits.
        let mut shown: Vec<LogItem> = entries
            .iter()
            .filter(|entry| self.log_filter.matches(&entry.operation))
            .map(LogItem::Entry)
            .chain(commits.iter().map(LogItem::Commit))
            .collect();
        shown.sort_by_key(|item| std::cmp::Reverse(item.at()));
        if shown.is_empty() && fetch.and_then(Fetch::data).is_some() {
            section.add_child(muted(
                madness_themes::t("shellomancy.manaBar.logEmpty").to_owned(),
                appearance,
            ));
        }
        let now = chrono::Utc::now().timestamp();
        let all_commits = match &self.commits {
            Some(Ok(repo)) if self.log_commits => repo.commits.as_slice(),
            _ => &[],
        };
        let total = shown.len();
        for item in shown.into_iter().take(self.shown) {
            section.add_child(match item {
                LogItem::Entry(entry) => self.render_log_entry(entry, all_commits, now, appearance),
                LogItem::Commit(commit) => self.render_commit(commit, entries, now, appearance),
            });
        }
        if let Some(more) = self.render_show_more(total, appearance) {
            section.add_child(more);
        }
        section.finish()
    }

    /// One todo log entry's card; it names any commit its note mentions.
    fn render_log_entry(
        &self,
        entry: &api::LogEntry,
        commits: &[git::Commit],
        now: i64,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let ui = appearance.ui_builder();
        let card_key = log_card_key(entry);
        let expanded = self.expanded.contains(&card_key);
        let color = operation_color(&entry.operation, appearance);
        let mut pills = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        pills.add_child(pill(entry.operation.to_uppercase(), color, appearance));
        if !entry.project.is_empty() {
            pills.add_child(chip(entry.project.clone(), appearance));
        }
        let top = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Shrinkable::new(1., pills.finish()).finish())
            .with_child(muted(api::age_label(entry.at, now), appearance))
            .finish();
        let mut lines = Flex::column().with_child(top).with_child(
            Container::new(
                ui.wrappable_text(entry.description.clone(), true)
                    .with_style(UiComponentStyles {
                        font_size: Some(13.),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            )
            .with_margin_top(6.)
            .finish(),
        );
        // Unrolled, the diff says the same thing in more detail.
        if !entry.fields.is_empty() && !expanded {
            lines.add_child(
                Container::new(muted(
                    madness_themes::tf(
                        "shellomancy.manaBar.logChanged",
                        &[("fields", &entry.fields.join(", "))],
                    ),
                    appearance,
                ))
                .with_margin_top(4.)
                .finish(),
            );
        }
        if let Some(comment) = &entry.comment {
            lines.add_child(
                Container::new(
                    ui.wrappable_text(comment.clone(), true)
                        .with_style(UiComponentStyles {
                            font_size: Some(12.),
                            font_color: Some(color),
                            ..Default::default()
                        })
                        .build()
                        .finish(),
                )
                .with_margin_top(6.)
                .with_uniform_padding(8.)
                .with_background(appearance.theme().surface_2())
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                .finish(),
            );
        }
        // Commits this entry's note names.
        for commit in commits.iter().filter(|commit| {
            entry
                .comment
                .as_deref()
                .is_some_and(|note| git::mentions(note, &commit.sha))
        }) {
            lines.add_child(
                Container::new(muted(
                    madness_themes::tf(
                        "shellomancy.manaBar.linkedCommit",
                        &[("sha", commit.short()), ("subject", &commit.subject)],
                    ),
                    appearance,
                ))
                .with_margin_top(4.)
                .finish(),
            );
        }
        let mut unrolled = None;
        if expanded {
            let details = vec![
                Detail::copyable("todo_id", entry.todo_id.clone(), "id"),
                Detail::new("operation", entry.operation.clone()),
                Detail::new("project", entry.project.clone()),
                Detail::new("timestamp", when_label(entry.at, now)),
                Detail::new("by", entry.actor.clone().unwrap_or_default()),
            ];
            let blocks = if entry.changes.is_empty() {
                vec![]
            } else {
                vec![changes_block(&entry.changes, appearance)]
            };
            unrolled = Some(self.render_unrolled(
                &card_key,
                details,
                blocks,
                changes_text(&entry.changes),
                vec![self.todo_open_button(&card_key, &entry.todo_id, None, appearance)],
                appearance,
            ));
        }
        card(
            lines.finish(),
            unrolled,
            self.row_mouse(&format!("{card_key}|head")),
            ManaBarAction::ToggleCard(card_key.clone()),
            Some(color),
            appearance,
        )
    }

    /// A commit's card: subject, sha and age, the todos whose notes name it;
    /// unrolled, its message and the files it changed.
    fn render_commit(
        &self,
        commit: &git::Commit,
        entries: &[api::LogEntry],
        now: i64,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let ui = appearance.ui_builder();
        let color: ColorU = appearance.theme().terminal_colors().normal.blue.into();
        let card_key = commit_card_key(commit);
        let mut pills = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        pills.add_child(pill(
            madness_themes::t("shellomancy.manaBar.commitPill").to_uppercase(),
            color,
            appearance,
        ));
        pills.add_child(chip(commit.short().to_owned(), appearance));
        let top = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Shrinkable::new(1., pills.finish()).finish())
            .with_child(muted(api::age_label(commit.at, now), appearance))
            .finish();
        let mut lines = Flex::column().with_child(top).with_child(
            Container::new(
                ui.wrappable_text(commit.subject.clone(), true)
                    .with_style(UiComponentStyles {
                        font_size: Some(13.),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            )
            .with_margin_top(6.)
            .finish(),
        );
        // The todos whose completion notes name this commit, once each.
        let mut linked: Vec<&api::LogEntry> = Vec::new();
        for entry in entries {
            if entry
                .comment
                .as_deref()
                .is_some_and(|note| git::mentions(note, &commit.sha))
                && !linked.iter().any(|seen| seen.todo_id == entry.todo_id)
            {
                linked.push(entry);
            }
        }
        for entry in &linked {
            lines.add_child(
                Container::new(muted(
                    madness_themes::tf(
                        "shellomancy.manaBar.linkedTodo",
                        &[("description", &one_line(&entry.description))],
                    ),
                    appearance,
                ))
                .with_margin_top(4.)
                .finish(),
            );
        }
        let mut unrolled = None;
        if self.expanded.contains(&card_key) {
            let mut details = vec![
                Detail::copyable("sha", commit.sha.clone(), "id"),
                Detail::new("author", commit.author.clone()),
                Detail::new("date", when_label(commit.at, now)),
            ];
            if let Some(entry) = linked.first() {
                details.push(Detail::copyable("todo", entry.todo_id.clone(), "ref"));
            }
            for entry in linked.iter().skip(1) {
                details.push(Detail::new("todo", entry.todo_id.clone()));
            }
            let mut blocks = Vec::new();
            let mut extra = format!("{}\n", commit.subject);
            if !commit.body.is_empty() {
                blocks.push(mono_block(&commit.body, appearance));
                extra.push_str(&format!("\n{}\n", commit.body));
            }
            match self.commit_files.get(&commit.sha) {
                Some(Some(Ok(files))) if !files.is_empty() => {
                    blocks.push(mono(truncate(files), MonoTone::Dim, appearance));
                    extra.push_str(&format!("\n{files}"));
                }
                Some(Some(Err(error))) => blocks.push(mono(
                    madness_themes::tf("shellomancy.manaBar.error", &[("error", error)]),
                    MonoTone::Dim,
                    appearance,
                )),
                Some(None) => blocks.push(mono(
                    madness_themes::t("shellomancy.manaBar.loading").to_owned(),
                    MonoTone::Dim,
                    appearance,
                )),
                _ => {}
            }
            let mut actions = Vec::new();
            if let Some(Ok(repo)) = &self.commits
                && let Some(web) = &repo.web
            {
                actions.push(self.open_button(
                    &card_key,
                    format!("{web}/commit/{}", commit.sha),
                    appearance,
                ));
            }
            unrolled =
                Some(self.render_unrolled(&card_key, details, blocks, extra, actions, appearance));
        }
        card(
            lines.finish(),
            unrolled,
            self.row_mouse(&format!("{card_key}|head")),
            ManaBarAction::ToggleCard(card_key.clone()),
            Some(color),
            appearance,
        )
    }

    /// "Show N more" under a list cut off at `self.shown`.
    fn render_show_more(&self, total: usize, appearance: &Appearance) -> Option<Box<dyn Element>> {
        let hidden = total.checked_sub(self.shown).filter(|hidden| *hidden > 0)?;
        Some(
            Container::new(toggle_chip(
                madness_themes::tf(
                    "shellomancy.manaBar.showMore",
                    &[
                        ("count", &hidden.min(PAGE_SIZE).to_string()),
                        ("hidden", &hidden.to_string()),
                    ],
                ),
                false,
                self.header_mouse.show_more.clone(),
                ManaBarAction::ShowMore,
                appearance,
            ))
            .with_margin_top(4.)
            .finish(),
        )
    }

    fn render_signed_out(&self, appearance: &Appearance) -> Box<dyn Element> {
        let ui = appearance.ui_builder();
        let theme = appearance.theme();
        let content = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                ui.paragraph(madness_themes::t("shellomancy.manaBar.signedOutTitle"))
                    .with_style(UiComponentStyles {
                        font_size: Some(16.),
                        font_weight: Some(Weight::Semibold),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            )
            .with_child(
                Container::new(
                    ui.paragraph(madness_themes::t(
                        "shellomancy.manaBar.signedOutDescription",
                    ))
                    .with_style(UiComponentStyles {
                        font_size: Some(13.),
                        font_color: Some(theme.sub_text_color(theme.background()).into_solid()),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
                )
                .with_margin_top(8.)
                .finish(),
            )
            .with_child(
                Container::new(
                    ui.button(ButtonVariant::Accent, self.header_mouse.account.clone())
                        .with_text_label(
                            madness_themes::t("shellomancy.manaBar.openAccount").to_owned(),
                        )
                        .build()
                        .on_click(|ctx, _, _| {
                            ctx.dispatch_typed_action(WorkspaceAction::ShowSettingsPage(
                                SettingsSection::Account,
                            ));
                        })
                        .finish(),
                )
                .with_margin_top(16.)
                .finish(),
            );
        Container::new(content.finish())
            .with_uniform_padding(24.)
            .finish()
    }

    fn render_todos(
        &self,
        fetch: Option<&Fetch<Vec<api::Todo>>>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let heading = match (&self.project, self.all_projects) {
            (Some(project), false) => madness_themes::tf(
                "shellomancy.manaBar.todosHeading",
                &[("project", &api::project_label(project))],
            ),
            _ => madness_themes::t("shellomancy.manaBar.todosHeadingAll").to_owned(),
        };
        let todos = fetch.and_then(Fetch::data);
        let mut section =
            Flex::column().with_child(section_heading(heading, todos.map(Vec::len), appearance));
        if self.project.is_none() {
            section.add_child(muted(
                madness_themes::t("shellomancy.manaBar.noProject").to_owned(),
                appearance,
            ));
        }
        if let Some(status) = fetch_status(fetch, appearance) {
            section.add_child(status);
        }
        match todos {
            Some(todos) if todos.is_empty() => {
                let empty = match (&self.project, self.all_projects) {
                    (Some(project), false) => madness_themes::tf(
                        "shellomancy.manaBar.todosEmpty",
                        &[("project", &api::project_label(project))],
                    ),
                    _ => madness_themes::t("shellomancy.manaBar.todosEmptyAll").to_owned(),
                };
                section.add_child(muted(empty, appearance));
            }
            Some(todos) => {
                let now = chrono::Utc::now().timestamp();
                let sorted = api::sort_todos(todos);
                for todo in sorted.iter().take(self.shown) {
                    section.add_child(self.render_todo(todo, now, appearance));
                }
                if let Some(more) = self.render_show_more(sorted.len(), appearance) {
                    section.add_child(more);
                }
            }
            None => {}
        }
        section.finish()
    }

    fn render_todo(&self, todo: &api::Todo, now: i64, appearance: &Appearance) -> Box<dyn Element> {
        let ui = appearance.ui_builder();
        let theme = appearance.theme();
        let priority_color = priority_color(todo.priority, appearance);
        let id = todo.id.clone();

        // Pills, then the age and the done button on the right.
        let mut pills = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        pills.add_child(pill(
            status_label(&todo.status),
            status_color(&todo.status, appearance),
            appearance,
        ));
        pills.add_child(pill(
            madness_themes::t(todo.priority.voice_key()).to_uppercase(),
            priority_color,
            appearance,
        ));
        // Name the project whenever the list spans more than one (all
        // projects, or a repo plus its submodules).
        if self.many_projects() {
            pills.add_child(chip(todo.project.clone(), appearance));
        }
        let done_tooltip = ui
            .tool_tip(madness_themes::t("shellomancy.manaBar.markDone").to_owned())
            .build()
            .finish();
        let icon_color = theme.sub_text_color(theme.background()).into_solid();
        let done: Box<dyn Element> = if self.completing.contains(&todo.id) {
            muted("…".to_owned(), appearance)
        } else {
            icon_button(
                appearance,
                Icon::Check,
                false,
                self.row_mouse(&format!("done:{}", todo.id)),
            )
            .with_tooltip(move || done_tooltip)
            .with_style(UiComponentStyles {
                font_color: Some(icon_color),
                height: Some(22.),
                width: Some(22.),
                padding: Some(Coords::uniform(4.)),
                ..Default::default()
            })
            .build()
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(ManaBarAction::MarkDone(id.clone()));
            })
            .with_cursor(Cursor::PointingHand)
            .finish()
        };
        let top = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Shrinkable::new(1., pills.finish()).finish())
            .with_child(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(muted(api::age_label(todo.created_at, now), appearance))
                    .with_child(Container::new(done).with_margin_left(4.).finish())
                    .finish(),
            )
            .finish();

        let mut content = Flex::column().with_child(top).with_child(
            Container::new(
                ui.wrappable_text(todo.description.clone(), true)
                    .with_style(UiComponentStyles {
                        font_size: Some(13.),
                        font_weight: Some(Weight::Semibold),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            )
            .with_margin_top(6.)
            .finish(),
        );
        if !todo.tags.is_empty() {
            content.add_child(
                Container::new(tag_row(&todo.tags, appearance))
                    .with_margin_top(6.)
                    .finish(),
            );
        }
        let card_key = format!("todo:{}", todo.id);
        let mut unrolled = None;
        if self.expanded.contains(&card_key) {
            let details = todo_details(todo, now);
            unrolled = Some(self.render_unrolled(
                &card_key,
                details,
                vec![],
                String::new(),
                vec![self.todo_open_button(&card_key, &todo.id, None, appearance)],
                appearance,
            ));
        }
        card(
            content.finish(),
            unrolled,
            self.row_mouse(&format!("{card_key}|head")),
            ManaBarAction::ToggleCard(card_key.clone()),
            Some(priority_color),
            appearance,
        )
    }

    fn render_quests(
        &self,
        fetch: Option<&Fetch<Vec<api::Quest>>>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let ui = appearance.ui_builder();
        let quests = fetch.and_then(Fetch::data);
        let mut section = Flex::column().with_child(section_heading(
            madness_themes::t("shellomancy.manaBar.questsHeading").to_owned(),
            quests.map(Vec::len),
            appearance,
        ));
        if let Some(status) = fetch_status(fetch, appearance) {
            section.add_child(status);
        }
        match quests {
            Some(quests) if quests.is_empty() => section.add_child(muted(
                madness_themes::t("shellomancy.manaBar.questsEmpty").to_owned(),
                appearance,
            )),
            Some(quests) => {
                let accent = appearance.theme().accent().into_solid();
                for quest in quests.iter().take(self.shown) {
                    let mut lines = Flex::column()
                        .with_child(
                            ui.wrappable_text(quest.name.clone(), true)
                                .with_style(UiComponentStyles {
                                    font_size: Some(13.),
                                    font_weight: Some(Weight::Semibold),
                                    ..Default::default()
                                })
                                .build()
                                .finish(),
                        )
                        .with_child(
                            Container::new(progress_bar(quest.pct, accent, appearance))
                                .with_margin_top(8.)
                                .finish(),
                        )
                        .with_child(
                            Container::new(self.quest_progress_line(quest, appearance))
                                .with_margin_top(4.)
                                .finish(),
                        );
                    if let Some(next) = &quest.next {
                        lines.add_child(
                            Container::new(
                                ui.wrappable_text(
                                    madness_themes::tf(
                                        "shellomancy.manaBar.questNext",
                                        &[("description", &next.description)],
                                    ),
                                    true,
                                )
                                .with_style(UiComponentStyles {
                                    font_size: Some(12.),
                                    ..Default::default()
                                })
                                .build()
                                .finish(),
                            )
                            .with_margin_top(6.)
                            .finish(),
                        );
                    }
                    let card_key = format!("quest:{}", quest.id);
                    let mut unrolled = None;
                    if self.expanded.contains(&card_key) {
                        let url = quest
                            .next
                            .as_ref()
                            .map(|next| api::todo_page_url(&next.todo_id))
                            .unwrap_or_else(|| DASHBOARD_URL.to_owned());
                        let mut details = vec![
                            Detail::copyable("id", quest.id.clone(), "id"),
                            Detail::new("project", quest.project.clone()),
                            Detail::new("status", quest.status.clone()),
                            Detail::new(
                                "progress",
                                format!("{}/{} ({}%)", quest.completed, quest.total, quest.pct),
                            ),
                            Detail::new(
                                "description",
                                quest.description.clone().unwrap_or_default(),
                            ),
                        ];
                        if let Some(next) = &quest.next {
                            details.push(Detail::copyable(
                                "next_todo",
                                next.todo_id.clone(),
                                "ref",
                            ));
                        }
                        unrolled = Some(self.render_unrolled(
                            &card_key,
                            details,
                            self.render_quest_chains(quest, appearance),
                            quest_markdown(quest),
                            vec![self.open_button(&card_key, url, appearance)],
                            appearance,
                        ));
                    }
                    section.add_child(card(
                        lines.finish(),
                        unrolled,
                        self.row_mouse(&format!("{card_key}|head")),
                        ManaBarAction::ToggleCard(card_key.clone()),
                        Some(accent),
                        appearance,
                    ));
                }
                if let Some(more) = self.render_show_more(quests.len(), appearance) {
                    section.add_child(more);
                }
            }
            None => {}
        }
        Container::new(section.finish())
            .with_margin_top(SECTION_GAP)
            .finish()
    }

    /// "10 of 17 done (58%) · 1 blocked", with the project when the list
    /// spans several.
    fn quest_progress_line(&self, quest: &api::Quest, appearance: &Appearance) -> Box<dyn Element> {
        let mut text = madness_themes::tf(
            "shellomancy.manaBar.questProgress",
            &[
                ("done", &quest.completed.to_string()),
                ("total", &quest.total.to_string()),
                ("pct", &quest.pct.to_string()),
            ],
        );
        if quest.blocked > 0 {
            text.push_str(" · ");
            text.push_str(&madness_themes::tf(
                "shellomancy.manaBar.questBlocked",
                &[("count", &quest.blocked.to_string())],
            ));
        }
        let mut row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Shrinkable::new(1., muted(text, appearance)).finish());
        if self.many_projects() && !quest.project.is_empty() {
            row.add_child(
                Container::new(chip(quest.project.clone(), appearance))
                    .with_margin_left(6.)
                    .finish(),
            );
        }
        row.finish()
    }

    /// Whether the lists span more than one project (all projects, or a repo
    /// plus its submodules), so cards name theirs.
    fn many_projects(&self) -> bool {
        self.all_projects
            || self
                .project
                .as_deref()
                .is_none_or(|project| project.contains(','))
    }

    /// An unrolled quest's chains, as the dashboard lists them: each chain's
    /// name and count, then its todos in order with what can be done to them.
    fn render_quest_chains(
        &self,
        quest: &api::Quest,
        appearance: &Appearance,
    ) -> Vec<Box<dyn Element>> {
        let accent = appearance.theme().accent().into_solid();
        let red = appearance.theme().terminal_colors().normal.red.into();
        quest
            .chains
            .iter()
            .map(|chain| {
                let mut heading = Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(mono(
                        chain.label.to_uppercase(),
                        MonoTone::Color(accent),
                        appearance,
                    ))
                    .with_child(
                        Container::new(mono(
                            format!("{}/{}", chain.completed, chain.total),
                            MonoTone::Dim,
                            appearance,
                        ))
                        .with_margin_left(8.)
                        .finish(),
                    );
                if chain.blocked > 0 {
                    heading.add_child(
                        Container::new(mono(
                            madness_themes::tf(
                                "shellomancy.manaBar.questBlocked",
                                &[("count", &chain.blocked.to_string())],
                            ),
                            MonoTone::Color(red),
                            appearance,
                        ))
                        .with_margin_left(8.)
                        .finish(),
                    );
                }
                let mut column = Flex::column().with_child(
                    Container::new(heading.finish())
                        .with_margin_bottom(2.)
                        .finish(),
                );
                for todo in &chain.todos {
                    column.add_child(self.render_quest_todo(quest, todo, appearance));
                }
                column.finish()
            })
            .collect()
    }

    /// One todo in a quest chain: a status glyph, its description (and its
    /// project, when it isn't the quest's), then copy, open and done buttons.
    fn render_quest_todo(
        &self,
        quest: &api::Quest,
        todo: &api::QuestTodo,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let is_next = quest.next_ids.contains(&todo.id);
        let (glyph, glyph_tone, text_tone) = quest_todo_look(todo, is_next, appearance);
        let mut description = if todo.missing || todo.description.is_empty() {
            madness_themes::t("shellomancy.manaBar.questDeleted").to_owned()
        } else {
            one_line(&todo.description)
        };
        if !todo.project.is_empty() && todo.project != quest.project {
            description = format!("{description} · {}", todo.project);
        }
        let card = quest_todo_key(&quest.id, &todo.id);
        let mut row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_child(
                ConstrainedBox::new(mono(glyph.to_owned(), glyph_tone, appearance))
                    .with_width(16.)
                    .finish(),
            )
            .with_child(Shrinkable::new(1., mono(description, text_tone, appearance)).finish());
        if !todo.missing {
            let mut buttons = Flex::row()
                .with_child(self.copy_button(&card, "id", todo.id.clone(), appearance))
                .with_child(
                    Container::new(self.todo_open_button(&card, &todo.id, Some(&card), appearance))
                        .with_margin_left(2.)
                        .finish(),
                );
            if !todo.is_done() && !todo.is_cancelled() {
                let done: Box<dyn Element> = if self.completing.contains(&todo.id) {
                    mono("[…]".to_owned(), MonoTone::Dim, appearance)
                } else {
                    self.term_button(
                        &card,
                        "done",
                        "✓",
                        ManaBarAction::MarkDone(todo.id.clone()),
                        appearance,
                    )
                };
                buttons.add_child(Container::new(done).with_margin_left(2.).finish());
            }
            row.add_child(
                Container::new(buttons.finish())
                    .with_margin_left(6.)
                    .finish(),
            );
        }
        let row = Container::new(row.finish()).with_margin_bottom(2.).finish();
        if !self.open_rows.contains(&card) {
            return row;
        }
        Flex::column()
            .with_child(row)
            .with_child(
                Container::new(self.render_row_detail(quest, todo, appearance))
                    .with_margin_left(16.)
                    .with_margin_bottom(6.)
                    .finish(),
            )
            .finish()
    }

    /// A quest row's todo unrolled in place: every field, fetched by id.
    fn render_row_detail(
        &self,
        quest: &api::Quest,
        todo: &api::QuestTodo,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let key = quest_detail_key(&quest.id, &todo.id);
        match self.todo_details.get(&todo.id) {
            Some(Some(Ok(full))) => self.render_unrolled(
                &key,
                todo_details(full, chrono::Utc::now().timestamp()),
                vec![],
                String::new(),
                vec![self.open_button(&key, api::todo_page_url(&todo.id), appearance)],
                appearance,
            ),
            Some(Some(Err(error))) => mono(
                madness_themes::tf("shellomancy.manaBar.error", &[("error", error)]),
                MonoTone::Color(appearance.theme().terminal_colors().normal.red.into()),
                appearance,
            ),
            _ => mono(
                madness_themes::t("shellomancy.manaBar.loading").to_owned(),
                MonoTone::Dim,
                appearance,
            ),
        }
    }

    fn render_lessons(
        &self,
        fetch: &Fetch<Vec<api::Lesson>>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let ui = appearance.ui_builder();
        let theme = appearance.theme();
        let mut section = Flex::column()
            .with_child(section_heading(
                madness_themes::t("shellomancy.manaBar.lessonsHeading").to_owned(),
                fetch.data().map(Vec::len),
                appearance,
            ))
            .with_child(
                Container::new(
                    Container::new(
                        Clipped::new(ChildView::new(&self.lesson_filter).finish()).finish(),
                    )
                    .with_padding(Padding::uniform(6.).with_left(10.).with_right(10.))
                    .with_background(theme.surface_1())
                    .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                    .finish(),
                )
                .with_margin_bottom(8.)
                .finish(),
            );
        if let Some(status) = fetch_status(Some(fetch), appearance) {
            section.add_child(status);
        }
        if let Some(lessons) = fetch.data() {
            let shown = api::filter_lessons(lessons, &self.lesson_query);
            if lessons.is_empty() {
                section.add_child(muted(
                    madness_themes::t("shellomancy.manaBar.lessonsEmpty").to_owned(),
                    appearance,
                ));
            } else if shown.is_empty() {
                section.add_child(muted(
                    madness_themes::tf(
                        "shellomancy.manaBar.lessonsNoMatch",
                        &[("query", self.lesson_query.trim())],
                    ),
                    appearance,
                ));
            }
            let now = chrono::Utc::now().timestamp();
            let lesson_color = theme.terminal_colors().normal.magenta.into();
            let total = shown.len();
            for lesson in shown.into_iter().take(self.shown) {
                let card_key = format!("lesson:{}", lesson.id);
                let top = Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(pill(
                        lesson.language.to_uppercase(),
                        lesson_color,
                        appearance,
                    ))
                    .with_child(muted(api::age_label(lesson.created_at, now), appearance))
                    .finish();
                let mut lines = Flex::column().with_child(top).with_child(
                    Container::new(
                        ui.wrappable_text(lesson.topic.clone(), true)
                            .with_style(UiComponentStyles {
                                font_size: Some(13.),
                                font_weight: Some(Weight::Semibold),
                                ..Default::default()
                            })
                            .build()
                            .finish(),
                    )
                    .with_margin_top(6.)
                    .finish(),
                );
                if !lesson.tags.is_empty() {
                    lines.add_child(
                        Container::new(tag_row(&lesson.tags, appearance))
                            .with_margin_top(6.)
                            .finish(),
                    );
                }
                let mut unrolled = None;
                if self.expanded.contains(&card_key) {
                    let details = vec![
                        Detail::copyable("id", lesson.id.clone(), "id"),
                        Detail::new("language", lesson.language.clone()),
                        Detail::new("created_at", when_label(lesson.created_at, now)),
                        Detail::new("tags", lesson.tags.join(", ")),
                    ];
                    let copy_body = self.term_button(
                        &card_key,
                        "text",
                        if self.copied.as_deref() == Some(format!("{card_key}|text").as_str()) {
                            madness_themes::t("shellomancy.manaBar.copied")
                        } else {
                            madness_themes::t("shellomancy.manaBar.copyText")
                        },
                        ManaBarAction::Copy {
                            button: format!("{card_key}|text"),
                            text: lesson.body.clone(),
                        },
                        appearance,
                    );
                    unrolled = Some(self.render_unrolled(
                        &card_key,
                        details,
                        vec![mono_block(&lesson.body, appearance)],
                        lesson.body.clone(),
                        vec![
                            copy_body,
                            self.open_button(&card_key, DASHBOARD_URL.to_owned(), appearance),
                        ],
                        appearance,
                    ));
                }
                section.add_child(card(
                    lines.finish(),
                    unrolled,
                    self.row_mouse(&format!("{card_key}|head")),
                    ManaBarAction::ToggleCard(card_key.clone()),
                    Some(lesson_color),
                    appearance,
                ));
            }
            if let Some(more) = self.render_show_more(total, appearance) {
                section.add_child(more);
            }
        }
        Container::new(section.finish())
            .with_margin_top(SECTION_GAP)
            .finish()
    }
}

/// One line of an unrolled card: the API's field name and its value, with a
/// copy button in `copy` slot when it's an id worth copying.
struct Detail {
    field: String,
    value: String,
    copy: Option<&'static str>,
}

impl Detail {
    fn new(field: &str, value: impl Into<String>) -> Self {
        Self {
            field: field.to_owned(),
            value: value.into(),
            copy: None,
        }
    }

    fn copyable(field: &str, value: impl Into<String>, slot: &'static str) -> Self {
        Self {
            copy: Some(slot),
            ..Self::new(field, value)
        }
    }
}

impl ManaBarView {
    /// The unrolled part of a card: the fields, then any `blocks` (notes,
    /// diffs), then the action row, led by `[copy all]`: every field as
    /// `field: value`, uncut, then `extra` (what the blocks show, as text).
    fn render_unrolled(
        &self,
        card: &str,
        details: Vec<Detail>,
        blocks: Vec<Box<dyn Element>>,
        extra: String,
        mut actions: Vec<Box<dyn Element>>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let button = format!("{card}|all");
        let label = if self.copied.as_deref() == Some(button.as_str()) {
            madness_themes::t("shellomancy.manaBar.copied")
        } else {
            madness_themes::t("shellomancy.manaBar.copyAll")
        };
        let copy_all = self.term_button(
            card,
            "all",
            label,
            ManaBarAction::Copy {
                button,
                text: copy_all_text(&details, &extra),
            },
            appearance,
        );
        actions.insert(0, copy_all);
        let theme = appearance.theme();
        let mut rows = Flex::column();
        for detail in details.into_iter().filter(|d| !d.value.trim().is_empty()) {
            let mut row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_child(
                    ConstrainedBox::new(mono(detail.field, MonoTone::Dim, appearance))
                        .with_width(FIELD_COLUMN_WIDTH)
                        .finish(),
                )
                .with_child(
                    Shrinkable::new(
                        1.,
                        mono(truncate(&detail.value), MonoTone::Plain, appearance),
                    )
                    .finish(),
                );
            if let Some(slot) = detail.copy {
                row.add_child(
                    Container::new(self.copy_button(card, slot, detail.value, appearance))
                        .with_margin_left(6.)
                        .finish(),
                );
            }
            rows.add_child(Container::new(row.finish()).with_margin_bottom(2.).finish());
        }
        for block in blocks {
            rows.add_child(Container::new(block).with_margin_top(6.).finish());
        }
        let inset = Container::new(rows.finish())
            .with_uniform_padding(8.)
            .with_background(theme.background())
            .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
            .finish();
        let mut column = Flex::column().with_child(inset);
        if !actions.is_empty() {
            let mut row = Wrap::row().with_run_spacing(4.);
            for action in actions {
                row.add_child(Container::new(action).with_margin_right(8.).finish());
            }
            column.add_child(Container::new(row.finish()).with_margin_top(6.).finish());
        }
        Container::new(column.finish()).with_margin_top(8.).finish()
    }

    /// `[copy]`, or `[copied ✓]` for a moment after it's clicked.
    fn copy_button(
        &self,
        card: &str,
        slot: &str,
        text: String,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let button = format!("{card}|{slot}");
        let label = if self.copied.as_deref() == Some(button.as_str()) {
            madness_themes::t("shellomancy.manaBar.copied")
        } else {
            madness_themes::t("shellomancy.manaBar.copy")
        };
        self.term_button(
            card,
            slot,
            label,
            ManaBarAction::Copy {
                button: button.clone(),
                text,
            },
            appearance,
        )
    }

    /// A bracketed monospace button, like `[open ↗]`. Its action can be the
    /// panel's or the workspace's (an agent tab).
    fn term_button<A: warpui::Action + Clone>(
        &self,
        card: &str,
        slot: &str,
        label: &str,
        action: A,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let accent = theme.accent().into_solid();
        let hover = theme.surface_3();
        let text = appearance
            .ui_builder()
            .span(format!("[{label}]"))
            .with_style(UiComponentStyles {
                font_family_id: Some(appearance.monospace_font_family()),
                font_size: Some(11.),
                font_color: Some(accent),
                ..Default::default()
            })
            .build()
            .finish();
        Hoverable::new(self.row_mouse(&format!("{card}|{slot}")), move |state| {
            let mut container = Container::new(text)
                .with_horizontal_padding(2.)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.)));
            if state.is_hovered() {
                container = container.with_background(hover);
            }
            container.finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .finish()
    }

    /// Opens a todo where Settings > Account says: its page, an agent tab,
    /// or (for a quest's `row`) unrolled in place. Rows get short labels.
    fn todo_open_button(
        &self,
        card: &str,
        id: &str,
        row: Option<&str>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let short = row.is_some();
        match (self.open_target, row) {
            (TodoOpenTarget::AgentTab, _) => {
                let short_id: String = id.chars().take(8).collect();
                let label = if short {
                    madness_themes::t("shellomancy.manaBar.agentShort")
                } else {
                    madness_themes::t("shellomancy.manaBar.openAgent")
                };
                self.term_button(
                    card,
                    "open",
                    label,
                    WorkspaceAction::OpenTabWithCommand {
                        command: api::agent_command(id),
                        cwd: self.repo_root.clone(),
                        title: Some(madness_themes::tf(
                            "shellomancy.agentTab.todoShort",
                            &[("id", &short_id)],
                        )),
                    },
                    appearance,
                )
            }
            (TodoOpenTarget::InPlace, Some(row)) => self.term_button(
                card,
                "open",
                if self.open_rows.contains(row) {
                    "▴"
                } else {
                    "▾"
                },
                ManaBarAction::ToggleTodoRow {
                    row: row.to_owned(),
                    id: id.to_owned(),
                },
                appearance,
            ),
            _ if short => self.term_button(
                card,
                "open",
                "↗",
                ManaBarAction::OpenUrl(api::todo_page_url(id)),
                appearance,
            ),
            _ => self.open_button(card, api::todo_page_url(id), appearance),
        }
    }

    fn open_button(&self, card: &str, url: String, appearance: &Appearance) -> Box<dyn Element> {
        self.term_button(
            card,
            "open",
            madness_themes::t("shellomancy.manaBar.open"),
            ManaBarAction::OpenUrl(url),
            appearance,
        )
    }
}

/// How bright a line of monospace text is.
#[derive(Clone, Copy)]
enum MonoTone {
    Plain,
    Dim,
    Color(ColorU),
}

/// Monospace text that wraps, for the unrolled cards.
fn mono(text: String, tone: MonoTone, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    let color = match tone {
        MonoTone::Plain => theme.main_text_color(theme.background()).into_solid(),
        MonoTone::Dim => theme.sub_text_color(theme.background()).into_solid(),
        MonoTone::Color(color) => color,
    };
    appearance
        .ui_builder()
        .wrappable_text(text, true)
        .with_style(UiComponentStyles {
            font_family_id: Some(appearance.monospace_font_family()),
            font_size: Some(11.),
            font_color: Some(color),
            ..Default::default()
        })
        .build()
        .finish()
}

/// Free text (notes, a quest's description, a scroll's body) in the inset.
fn mono_block(text: &str, appearance: &Appearance) -> Box<dyn Element> {
    mono(truncate(text.trim()), MonoTone::Plain, appearance)
}

/// What an update changed, as a diff: `- field: old` / `+ field: new`.
fn changes_block(changes: &[api::Change], appearance: &Appearance) -> Box<dyn Element> {
    let colors = &appearance.theme().terminal_colors().normal;
    let (red, green) = (colors.red.into(), colors.green.into());
    let mut lines = Flex::column();
    for change in changes {
        if let Some(old) = &change.old {
            lines.add_child(mono(
                format!("- {}: {}", change.field, one_line(old)),
                MonoTone::Color(red),
                appearance,
            ));
        }
        if let Some(new) = &change.new {
            lines.add_child(mono(
                format!("+ {}: {}", change.field, one_line(new)),
                MonoTone::Color(green),
                appearance,
            ));
        }
        if change.old.is_none() && change.new.is_none() {
            lines.add_child(mono(
                format!("~ {}", change.field),
                MonoTone::Dim,
                appearance,
            ));
        }
    }
    lines.finish()
}

/// A todo's fields as an unrolled card lists them.
fn todo_details(todo: &api::Todo, now: i64) -> Vec<Detail> {
    let mut details = vec![
        Detail::copyable("id", todo.id.clone(), "id"),
        Detail::new("project", todo.project.clone()),
        Detail::new("status", todo.status.clone()),
        Detail::new("priority", format!("{:?}", todo.priority)),
        Detail::new(
            "target_agent",
            todo.target_agent.clone().unwrap_or_default(),
        ),
        Detail::new("created_at", when_label(todo.created_at, now)),
        Detail::new("updated_at", when_label(todo.updated_at, now)),
        Detail::new("ticket", todo.ticket.clone().unwrap_or_default()),
        Detail::new("tags", todo.tags.join(", ")),
        Detail::new("notes", todo.notes.clone().unwrap_or_default()),
    ];
    for (field, value) in &todo.extra {
        details.push(if field == "quest_id" {
            Detail::copyable(field, value.clone(), "ref")
        } else {
            Detail::new(field, value.clone())
        });
    }
    details
}

/// What `[copy all]` copies: each non-blank field as `field: value`, then
/// `extra` after a blank line.
fn copy_all_text(details: &[Detail], extra: &str) -> String {
    let mut text = details
        .iter()
        .filter(|detail| !detail.value.trim().is_empty())
        .map(|detail| format!("{}: {}", detail.field, detail.value))
        .collect::<Vec<_>>()
        .join("\n");
    let extra = extra.trim();
    if !extra.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(extra);
    }
    text
}

/// An update's changes as the diff the card shows, in plain text.
fn changes_text(changes: &[api::Change]) -> String {
    let mut lines = Vec::new();
    for change in changes {
        if let Some(old) = &change.old {
            lines.push(format!("- {}: {old}", change.field));
        }
        if let Some(new) = &change.new {
            lines.push(format!("+ {}: {new}", change.field));
        }
        if change.old.is_none() && change.new.is_none() {
            lines.push(format!("~ {}", change.field));
        }
    }
    lines.join("\n")
}

/// A quest's chains as a markdown checklist, ids kept for agents:
/// `## Foundation (1/2)` then `- [x] Sign in (`t1`)`.
fn quest_markdown(quest: &api::Quest) -> String {
    let mut sections = Vec::new();
    for chain in &quest.chains {
        let mut lines = vec![format!(
            "## {} ({}/{})",
            chain.label, chain.completed, chain.total
        )];
        for todo in &chain.todos {
            let mark = if todo.is_done() {
                "x"
            } else if todo.is_cancelled() {
                "-"
            } else {
                " "
            };
            let description = if todo.missing || todo.description.is_empty() {
                "(deleted)".to_owned()
            } else {
                // Folded to one line, but not cut short: it's for pasting.
                todo.description
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let mut status = String::new();
            if !todo.is_done()
                && !todo.is_cancelled()
                && !matches!(todo.status.as_str(), "" | "pending")
            {
                status = format!(" [{}]", todo.status);
            }
            if quest.next_ids.contains(&todo.id) {
                status.push_str(" <- next");
            }
            lines.push(format!("- [{mark}] {description}{status} (`{}`)", todo.id));
        }
        sections.push(lines.join("\n"));
    }
    sections.join("\n\n")
}

/// `2026-11-16 22:24 (3h)`, in local time; blank for 0.
pub fn when_label(at: i64, now: i64) -> String {
    if at <= 0 {
        return String::new();
    }
    let Some(local) = chrono::DateTime::from_timestamp(at, 0) else {
        return String::new();
    };
    let local = local.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M");
    format!("{local} ({})", api::age_label(at, now))
}

/// The button key prefix for a todo inside a quest card.
fn quest_todo_key(quest_id: &str, todo_id: &str) -> String {
    format!("qt:{quest_id}:{todo_id}")
}

/// The button key prefix for a quest row's todo unrolled in place.
fn quest_detail_key(quest_id: &str, todo_id: &str) -> String {
    format!("qd:{quest_id}:{todo_id}")
}

/// How a quest's todo looks: its glyph, the glyph's tone and the text's.
/// Done and cancelled todos fade; the ones to do next get the accent.
fn quest_todo_look(
    todo: &api::QuestTodo,
    is_next: bool,
    appearance: &Appearance,
) -> (&'static str, MonoTone, MonoTone) {
    let colors = &appearance.theme().terminal_colors().normal;
    if todo.missing {
        ("?", MonoTone::Dim, MonoTone::Dim)
    } else if todo.is_done() {
        ("✓", MonoTone::Color(colors.green.into()), MonoTone::Dim)
    } else if todo.is_cancelled() {
        ("✕", MonoTone::Dim, MonoTone::Dim)
    } else if todo.status == "blocked" {
        ("⊘", MonoTone::Color(colors.red.into()), MonoTone::Plain)
    } else if todo.status == "in_progress" {
        ("◐", MonoTone::Color(colors.cyan.into()), MonoTone::Plain)
    } else if is_next {
        (
            "▶",
            MonoTone::Color(appearance.theme().accent().into_solid()),
            MonoTone::Plain,
        )
    } else {
        ("○", MonoTone::Dim, MonoTone::Plain)
    }
}

/// One line of the Log's timeline.
enum LogItem<'a> {
    Entry(&'a api::LogEntry),
    Commit(&'a git::Commit),
}

impl LogItem<'_> {
    fn at(&self) -> i64 {
        match self {
            LogItem::Entry(entry) => entry.at,
            LogItem::Commit(commit) => commit.at,
        }
    }
}

/// A commit card's key.
fn commit_card_key(commit: &git::Commit) -> String {
    format!("commit:{}", commit.sha)
}

/// A card's key for a log entry: stable while newer entries arrive above it.
fn log_card_key(entry: &api::LogEntry) -> String {
    format!("log:{}:{}:{}", entry.todo_id, entry.at, entry.operation)
}

fn truncate(text: &str) -> String {
    match text.char_indices().nth(MAX_DETAIL_CHARS) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

/// Newlines folded so a diff line stays one line.
fn one_line(text: &str) -> String {
    truncate(&text.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// A section's heading in the accent colour, with a count badge once loaded.
fn section_heading(
    text: String,
    count: Option<usize>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let accent = theme.accent().into_solid();
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Shrinkable::new(
                1.,
                appearance
                    .ui_builder()
                    .wrappable_text(text.to_uppercase(), true)
                    .with_style(UiComponentStyles {
                        font_size: Some(12.),
                        font_weight: Some(Weight::Bold),
                        font_color: Some(accent),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            )
            .finish(),
        );
    if let Some(count) = count {
        row.add_child(
            Container::new(
                appearance
                    .ui_builder()
                    .span(count.to_string())
                    .with_style(UiComponentStyles {
                        font_size: Some(11.),
                        font_color: Some(theme.main_text_color(theme.background()).into_solid()),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            )
            .with_horizontal_padding(7.)
            .with_vertical_padding(1.)
            .with_margin_left(8.)
            .with_background(theme.surface_3())
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(9.)))
            .finish(),
        );
    }
    Container::new(row.finish())
        .with_margin_top(8.)
        .with_margin_bottom(8.)
        .finish()
}

fn muted(text: String, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    appearance
        .ui_builder()
        .wrappable_text(text, true)
        .with_style(UiComponentStyles {
            font_size: Some(12.),
            font_color: Some(theme.sub_text_color(theme.background()).into_solid()),
            ..Default::default()
        })
        .build()
        .finish()
}

/// A small rounded label tinted with `color`, like Inventorium's status pills.
fn pill(text: String, color: ColorU, appearance: &Appearance) -> Box<dyn Element> {
    Container::new(
        appearance
            .ui_builder()
            .span(text)
            .with_style(UiComponentStyles {
                font_size: Some(10.),
                font_weight: Some(Weight::Bold),
                font_color: Some(color),
                ..Default::default()
            })
            .build()
            .finish(),
    )
    .with_horizontal_padding(7.)
    .with_vertical_padding(2.)
    .with_margin_right(6.)
    .with_background(Fill::Solid(ColorU::new(color.r, color.g, color.b, 40)))
    .with_border(Border::all(1.).with_border_color(color))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
    .finish()
}

/// A neutral chip for projects and tags.
fn chip(text: String, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    Container::new(
        appearance
            .ui_builder()
            .span(text)
            .with_style(UiComponentStyles {
                font_size: Some(11.),
                font_color: Some(theme.main_text_color(theme.background()).into_solid()),
                ..Default::default()
            })
            .build()
            .finish(),
    )
    .with_horizontal_padding(8.)
    .with_vertical_padding(2.)
    .with_margin_right(6.)
    .with_background(theme.surface_2())
    .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.)))
    .finish()
}

/// Tag chips in a row that wraps.
fn tag_row(tags: &[String], appearance: &Appearance) -> Box<dyn Element> {
    let mut row = Wrap::row().with_run_spacing(4.);
    for tag in tags.iter().take(MAX_TAGS) {
        row.add_child(chip(tag.clone(), appearance));
    }
    row.finish()
}

/// A thin bar filled to `pct` percent.
fn progress_bar(pct: u64, color: ColorU, appearance: &Appearance) -> Box<dyn Element> {
    let pct = pct.min(100) as f32;
    let track = appearance.theme().surface_3();
    let mut bar = Flex::row().with_main_axis_size(MainAxisSize::Max);
    if pct > 0. {
        bar.add_child(
            Expanded::new(
                pct,
                ConstrainedBox::new(
                    Container::new(Empty::new().finish())
                        .with_background(Fill::Solid(color))
                        .finish(),
                )
                .with_height(5.)
                .finish(),
            )
            .finish(),
        );
    }
    if pct < 100. {
        bar.add_child(
            Expanded::new(
                100. - pct,
                ConstrainedBox::new(
                    Container::new(Empty::new().finish())
                        .with_background(track)
                        .finish(),
                )
                .with_height(5.)
                .finish(),
            )
            .finish(),
        );
    }
    Container::new(bar.finish())
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.)))
        .finish()
}

/// A chip that's filled with the accent colour when selected.
fn toggle_chip(
    label: String,
    selected: bool,
    mouse: MouseStateHandle,
    action: ManaBarAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let accent_fill = theme.accent();
    let text_color = if selected {
        theme.background().into_solid()
    } else {
        theme.main_text_color(theme.background()).into_solid()
    };
    let idle = theme.surface_2();
    let hover = theme.surface_3();
    let border = theme.surface_3();
    let label_element = appearance
        .ui_builder()
        .span(label)
        .with_style(UiComponentStyles {
            font_size: Some(11.),
            font_weight: Some(Weight::Bold),
            font_color: Some(text_color),
            ..Default::default()
        })
        .build()
        .finish();
    Container::new(
        Hoverable::new(mouse, move |state| {
            let background = if selected {
                accent_fill
            } else if state.is_hovered() {
                hover
            } else {
                idle
            };
            Container::new(label_element)
                .with_horizontal_padding(10.)
                .with_vertical_padding(4.)
                .with_background(background)
                .with_border(Border::all(1.).with_border_fill(border))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(12.)))
                .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .finish(),
    )
    .with_margin_right(6.)
    .finish()
}

fn operation_color(operation: &str, appearance: &Appearance) -> ColorU {
    let colors = &appearance.theme().terminal_colors().normal;
    match operation {
        "create" => colors.green.into(),
        "complete" => colors.cyan.into(),
        "delete" => colors.red.into(),
        "restore" => colors.magenta.into(),
        _ => colors.yellow.into(),
    }
}

fn priority_color(priority: api::Priority, appearance: &Appearance) -> ColorU {
    let colors = &appearance.theme().terminal_colors().normal;
    match priority {
        api::Priority::Critical => colors.red.into(),
        api::Priority::High => colors.yellow.into(),
        api::Priority::Medium => colors.blue.into(),
        api::Priority::Low => colors.green.into(),
    }
}

fn status_color(status: &str, appearance: &Appearance) -> ColorU {
    let theme = appearance.theme();
    let colors = &theme.terminal_colors().normal;
    match status {
        "in_progress" => colors.cyan.into(),
        "blocked" => colors.red.into(),
        "review" => colors.magenta.into(),
        _ => theme.sub_text_color(theme.background()).into_solid(),
    }
}

/// "IN PROGRESS", "PENDING", ... as the pill shows it.
fn status_label(status: &str) -> String {
    status.replace('_', " ").to_uppercase()
}

/// "Loading…" before the first data arrives, or the error from the last try.
fn fetch_status<T>(fetch: Option<&Fetch<T>>, appearance: &Appearance) -> Option<Box<dyn Element>> {
    let fetch = fetch?;
    if let Some(error) = fetch.error() {
        return Some(
            appearance
                .ui_builder()
                .wrappable_text(
                    madness_themes::tf("shellomancy.manaBar.error", &[("error", error)]),
                    true,
                )
                .with_style(UiComponentStyles {
                    font_size: Some(12.),
                    font_color: Some(appearance.theme().ui_error_color()),
                    ..Default::default()
                })
                .build()
                .finish(),
        );
    }
    (fetch.is_loading() && fetch.data().is_none()).then(|| {
        muted(
            madness_themes::t("shellomancy.manaBar.loading").to_owned(),
            appearance,
        )
    })
}

/// A bordered card with an optional coloured left edge. Its summary
/// (`content`) is what you click to unroll it, tinted on hover; the unrolled
/// `body` isn't clickable, so a drag there selects text instead of rolling the
/// card up.
fn card(
    content: Box<dyn Element>,
    body: Option<Box<dyn Element>>,
    mouse: MouseStateHandle,
    action: ManaBarAction,
    edge: Option<ColorU>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let background = theme.surface_1();
    let hover = theme.surface_2();
    let border = theme.surface_3();
    let radius = Radius::Pixels(8.);
    let (head_corners, head_bottom) = if body.is_some() {
        (CornerRadius::with_top(radius), 2.)
    } else {
        (CornerRadius::with_all(radius), 10.)
    };
    let head = Hoverable::new(mouse, move |state| {
        let mut head = Container::new(content)
            .with_horizontal_padding(10.)
            .with_padding_top(10.)
            .with_padding_bottom(head_bottom)
            .with_corner_radius(head_corners);
        if state.is_hovered() {
            head = head.with_background(hover);
        }
        head.finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
    .finish();
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(head);
    if let Some(body) = body {
        column.add_child(
            Container::new(body)
                .with_horizontal_padding(10.)
                .with_padding_bottom(10.)
                .finish(),
        );
    }
    let inner = Container::new(column.finish())
        .with_background(background)
        .with_border(Border::all(1.).with_border_fill(border))
        .with_corner_radius(CornerRadius::with_all(radius))
        .finish();
    let mut card = Container::new(inner).with_margin_bottom(8.);
    if let Some(edge) = edge {
        card = card
            .with_border(Border::left(3.).with_border_color(edge))
            .with_corner_radius(CornerRadius::with_all(radius));
    }
    card.finish()
}

#[cfg(test)]
#[path = "mana_bar_view_tests.rs"]
mod tests;
