//! Shellomancy: the Mana Bar tab of the tools panel (Drive's old slot).
//!
//! Shows the signed-in Madness account's active todos and quests for the
//! project the focused terminal is in (or every project), and recent lessons
//! with a filter. Items open on madnessinteractive.cc; todos can be marked
//! done. The data lives in [`ManaBar`], shared by every window; this view
//! only decides which project to ask about and polls while it's on screen.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use pathfinder_color::ColorU;
use warp_core::ui::Icon;
use warpui::r#async::Timer;
use warpui::elements::{
    Border, ChildView, Clipped, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox,
    Container, CornerRadius, CrossAxisAlignment, Element, Empty, Expanded, Fill, Flex, Hoverable,
    MainAxisAlignment, MainAxisSize, MouseStateHandle, Padding, ParentElement, Radius,
    ScrollbarWidth, Shrinkable, Wrap,
};
use warpui::fonts::Weight;
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
use crate::mana_bar::{Fetch, List, ManaBar, ManaBarEvent, ProjectKey};
use crate::settings_view::SettingsSection;
use crate::ui_components::buttons::icon_button;
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
}

impl LogFilter {
    const ALL: [LogFilter; 4] = [
        LogFilter::All,
        LogFilter::Created,
        LogFilter::Updated,
        LogFilter::Completed,
    ];

    fn voice_key(self) -> &'static str {
        match self {
            LogFilter::All => "shellomancy.manaBar.logAll",
            LogFilter::Created => "shellomancy.manaBar.logCreated",
            LogFilter::Updated => "shellomancy.manaBar.logUpdated",
            LogFilter::Completed => "shellomancy.manaBar.logCompleted",
        }
    }

    fn matches(self, operation: &str) -> bool {
        match self {
            LogFilter::All => true,
            LogFilter::Created => operation == "create",
            LogFilter::Updated => operation == "update",
            LogFilter::Completed => operation == "complete",
        }
    }
}

#[derive(Clone, Debug)]
pub enum ManaBarAction {
    SelectTab(ManaBarTab),
    ShowMore,
    SetLogFilter(LogFilter),
    Refresh,
    ToggleAllProjects,
    OpenTodo(String),
    MarkDone(String),
    OpenUrl(String),
    ToggleLesson(String),
}

#[derive(Default)]
struct HeaderMouseStates {
    refresh: MouseStateHandle,
    scope: MouseStateHandle,
    account: MouseStateHandle,
    site: MouseStateHandle,
    tabs: [MouseStateHandle; 4],
    show_more: MouseStateHandle,
    log_filters: [MouseStateHandle; 4],
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
    expanded_lesson: Option<String>,
    scroll: ClippedScrollStateHandle,
    header_mouse: HeaderMouseStates,
    /// Per-row mouse states, keyed like `todo:<id>`.
    row_mouse: HashMap<String, MouseStateHandle>,
    completing: HashSet<String>,
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
        Self {
            project: None,
            all_projects: false,
            tab: ManaBarTab::default(),
            log_filter: LogFilter::default(),
            shown: PAGE_SIZE,
            lesson_filter,
            lesson_query: String::new(),
            expanded_lesson: None,
            scroll: Default::default(),
            header_mouse: Default::default(),
            row_mouse: HashMap::new(),
            completing: HashSet::new(),
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
    }

    fn refresh(&mut self, ctx: &mut ViewContext<Self>) {
        if !ctx.has_singleton_model::<ManaBar>() {
            return;
        }
        let (list, key) = (self.tab.list(), self.project_key());
        ManaBar::handle(ctx).update(ctx, |mana_bar, ctx| mana_bar.refresh(list, &key, ctx));
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
            .map(|quest| format!("quest:{}", quest.id));
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
            .enumerate()
            .map(|(index, _)| format!("log:{index}"));
        let keys: Vec<String> = todos.chain(quests).chain(lessons).chain(log).collect();
        for key in keys {
            self.row_mouse.entry(key).or_default();
        }
    }

    fn row_mouse(&self, key: &str) -> MouseStateHandle {
        self.row_mouse.get(key).cloned().unwrap_or_default()
    }

    fn mark_done(&mut self, id: &str, ctx: &mut ViewContext<Self>) {
        if self.completing.contains(id) {
            return;
        }
        let Some(key) = crate::mana_bar::api_key(ctx) else {
            return;
        };
        let project = self.project_key();
        let description = ManaBar::as_ref(ctx)
            .todos(&project)
            .and_then(Fetch::data)
            .and_then(|todos| todos.iter().find(|todo| todo.id == id))
            .map(|todo| todo.description.clone())
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
            ManaBarAction::OpenTodo(id) => ctx.open_url(&api::todo_page_url(id)),
            ManaBarAction::MarkDone(id) => self.mark_done(id, ctx),
            ManaBarAction::OpenUrl(url) => ctx.open_url(url),
            ManaBarAction::ToggleLesson(id) => {
                self.expanded_lesson = if self.expanded_lesson.as_deref() == Some(id.as_str()) {
                    None
                } else {
                    Some(id.clone())
                };
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
            (Some(project), false) => {
                madness_themes::tf("shellomancy.manaBar.logHeading", &[("project", project)])
            }
            _ => madness_themes::t("shellomancy.manaBar.logHeadingAll").to_owned(),
        };
        let entries = fetch.and_then(Fetch::data);
        let mut section = Flex::column().with_child(section_heading(heading, None, appearance));
        let mut filters = Wrap::row().with_run_spacing(6.);
        for (filter, mouse) in LogFilter::ALL.iter().zip(&self.header_mouse.log_filters) {
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
        let Some(entries) = entries else {
            return section.finish();
        };
        let shown: Vec<(usize, &api::LogEntry)> = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| self.log_filter.matches(&entry.operation))
            .collect();
        if shown.is_empty() {
            section.add_child(muted(
                madness_themes::t("shellomancy.manaBar.logEmpty").to_owned(),
                appearance,
            ));
        }
        let now = chrono::Utc::now().timestamp();
        let total = shown.len();
        for (index, entry) in shown.into_iter().take(self.shown) {
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
            if !entry.fields.is_empty() {
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
            section.add_child(card(
                lines.finish(),
                self.row_mouse(&format!("log:{index}")),
                ManaBarAction::OpenTodo(entry.todo_id.clone()),
                Some(color),
                appearance,
            ));
        }
        if let Some(more) = self.render_show_more(total, appearance) {
            section.add_child(more);
        }
        section.finish()
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
            (Some(project), false) => {
                madness_themes::tf("shellomancy.manaBar.todosHeading", &[("project", project)])
            }
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
                        &[("project", project)],
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
        if self.all_projects || self.project.is_none() {
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
        card(
            content.finish(),
            self.row_mouse(&format!("todo:{}", todo.id)),
            ManaBarAction::OpenTodo(todo.id.clone()),
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
                            Container::new(muted(
                                madness_themes::tf(
                                    "shellomancy.manaBar.questProgress",
                                    &[
                                        ("done", &quest.completed.to_string()),
                                        ("total", &quest.total.to_string()),
                                        ("pct", &quest.pct.to_string()),
                                    ],
                                ),
                                appearance,
                            ))
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
                    let url = quest
                        .next
                        .as_ref()
                        .map(|next| api::todo_page_url(&next.todo_id))
                        .unwrap_or_else(|| DASHBOARD_URL.to_owned());
                    section.add_child(card(
                        lines.finish(),
                        self.row_mouse(&format!("quest:{}", quest.id)),
                        ManaBarAction::OpenUrl(url),
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
                let expanded = self.expanded_lesson.as_deref() == Some(lesson.id.as_str());
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
                if expanded {
                    lines.add_child(
                        Container::new(
                            ui.wrappable_text(lesson.body.clone(), true)
                                .with_style(UiComponentStyles {
                                    font_size: Some(12.),
                                    ..Default::default()
                                })
                                .build()
                                .finish(),
                        )
                        .with_margin_top(8.)
                        .with_uniform_padding(8.)
                        .with_background(theme.surface_2())
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                        .finish(),
                    );
                    lines.add_child(
                        Container::new(
                            ui.link(
                                madness_themes::t("shellomancy.manaBar.openOnSite").to_owned(),
                                None,
                                Some(Box::new(|ctx| {
                                    ctx.dispatch_typed_action(ManaBarAction::OpenUrl(
                                        DASHBOARD_URL.to_owned(),
                                    ));
                                })),
                                self.header_mouse.site.clone(),
                            )
                            .build()
                            .finish(),
                        )
                        .with_margin_top(6.)
                        .finish(),
                    );
                }
                section.add_child(card(
                    lines.finish(),
                    self.row_mouse(&format!("lesson:{}", lesson.id)),
                    ManaBarAction::ToggleLesson(lesson.id.clone()),
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

/// A bordered card, tinted on hover, with an optional coloured left edge.
fn card(
    content: Box<dyn Element>,
    mouse: MouseStateHandle,
    action: ManaBarAction,
    edge: Option<ColorU>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let background = theme.surface_1();
    let hover = theme.surface_2();
    let border = theme.surface_3();
    let inner = Hoverable::new(mouse, move |state| {
        Container::new(content)
            .with_uniform_padding(10.)
            .with_background(if state.is_hovered() {
                hover
            } else {
                background
            })
            .with_border(Border::all(1.).with_border_fill(border))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
            .finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
    .finish();
    let mut card = Container::new(inner).with_margin_bottom(8.);
    if let Some(edge) = edge {
        card = card
            .with_border(Border::left(3.).with_border_color(edge))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)));
    }
    card.finish()
}
