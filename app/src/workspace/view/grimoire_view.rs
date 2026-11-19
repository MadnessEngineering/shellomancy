//! Shellomancy: the Grimoire tab of the tools panel, where Drive's notebooks
//! and workflows went: plain files on disk (see [`crate::grimoire`]).
//!
//! It lists the focused repo's shelf (`<repo>/.mad/grimoire`) and the global
//! one (`~/.mad/grimoire`), grouped by folder, with a filter over titles and
//! text. A notebook opens in the Markdown viewer with runnable blocks, an
//! incantation casts into the focused input (asking for its arguments), and
//! a script puts the command that runs it in the input. The shelves are read
//! again on show, on refresh, and every few seconds while the tab is shown, so
//! edits made anywhere turn up.
//!
//! The page cards are shared with the search panel's Grimoire scope:
//! [`render_page`] draws one and [`act_on_page`] does what a click asks.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Local;
use pathfinder_color::ColorU;
use warp_core::ui::Icon;
use warpui::r#async::Timer;
use warpui::clipboard::ClipboardContent;
use warpui::elements::{
    Border, ChildView, ClippedScrollStateHandle, ClippedScrollable, Container, CornerRadius,
    CrossAxisAlignment, Element, Fill, Flex, Hoverable, MainAxisAlignment, MainAxisSize,
    MouseStateHandle, Padding, ParentElement, Radius, ScrollbarWidth, Shrinkable, Wrap,
};
use warpui::fonts::Weight;
use warpui::platform::Cursor;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle};

use crate::appearance::Appearance;
use crate::editor::{
    EditorView, Event as EditorEvent, PropagateAndNoOpNavigationKeys,
    PropagateHorizontalNavigationKeys, SingleLineEditorOptions, TextOptions,
};
use crate::grimoire::{self, Page, PageKind, Shelf};
use crate::ui_components::buttons::icon_button;
use crate::view_components::DismissibleToast;
use crate::workflows::{WorkflowSelectionSource, WorkflowSource, WorkflowType};
use crate::workspace::{ToastStack, WorkspaceAction};

/// How often a shown tab reads its shelves again.
const POLL_INTERVAL: Duration = Duration::from_secs(10);
const SCROLLBAR_WIDTH: ScrollbarWidth = ScrollbarWidth::Auto;
const COPIED_FLASH: Duration = Duration::from_millis(1500);
/// Lines of a page shown when it's peeked at.
const MAX_PEEK_LINES: usize = 40;
/// A page card's buttons, keyed `<page key>|<slot>`; `row` is the card itself.
pub(crate) const PAGE_SLOTS: [&str; 6] = ["row", "read", "cast", "edit", "copy", "peek"];

/// What a click on a page asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageClick {
    /// The card itself: read a notebook, cast anything castable, else edit.
    Primary,
    /// Open a notebook in the Markdown viewer.
    Read,
    Cast,
    Edit,
    Copy,
    /// Show or hide the page's text in its card.
    Peek,
}

#[derive(Clone, Debug)]
pub enum GrimoireAction {
    Page {
        key: String,
        click: PageClick,
    },
    /// Rolls a folder up or out; keyed `<shelf>|<folder>`.
    ToggleFolder(String),
    NewNotebook(Shelf),
    NewIncantation,
    OpenFolder,
    Refresh,
}

#[derive(Default)]
struct HeaderMouseStates {
    refresh: MouseStateHandle,
    new_notebook: MouseStateHandle,
    new_incantation: MouseStateHandle,
    open_folder: MouseStateHandle,
    start_repo: MouseStateHandle,
    empty_notebook: MouseStateHandle,
    empty_incantation: MouseStateHandle,
}

pub struct GrimoireView {
    /// The focused terminal's repo, whose shelf lists first.
    repo_root: Option<PathBuf>,
    /// Every page on the shelves, in listing order.
    pages: Vec<Page>,
    loaded: bool,
    /// Bumped per read so a slow one for an old repo is dropped.
    scan_generation: u64,
    filter: ViewHandle<EditorView>,
    query: String,
    /// Pages whose text is showing, by key.
    peeked: HashSet<String>,
    /// Rolled-up folders, keyed `<shelf>|<folder>`.
    collapsed: HashSet<String>,
    copied: Option<String>,
    copy_generation: u64,
    row_mouse: HashMap<String, MouseStateHandle>,
    header_mouse: HeaderMouseStates,
    scroll: ClippedScrollStateHandle,
    visible: bool,
    poll_generation: u64,
}

impl Entity for GrimoireView {
    type Event = ();
}

impl GrimoireView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let filter = ctx.add_typed_action_view(|ctx| {
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
            editor.set_placeholder_text(madness_themes::t("shellomancy.grimoire.filter"), ctx);
            editor
        });
        ctx.subscribe_to_view(&filter, |me, editor, event, ctx| match event {
            EditorEvent::Edited(_) => {
                me.query = editor.as_ref(ctx).buffer_text(ctx);
                ctx.notify();
            }
            // Enter does the first match's main thing: type, Enter, cast.
            EditorEvent::Enter => {
                if let Some(page) = me.matching().first().and_then(|i| me.pages.get(*i)) {
                    act_on_page(page, &PageClick::Primary, ctx);
                }
            }
            _ => {}
        });
        Self {
            repo_root: None,
            pages: Vec::new(),
            loaded: false,
            scan_generation: 0,
            filter,
            query: String::new(),
            peeked: HashSet::new(),
            collapsed: HashSet::new(),
            copied: None,
            copy_generation: 0,
            row_mouse: HashMap::new(),
            header_mouse: Default::default(),
            scroll: Default::default(),
            visible: false,
            poll_generation: 0,
        }
    }

    /// The focused terminal's repo changed: its shelf lists first.
    pub fn set_repo_root(&mut self, root: Option<PathBuf>, ctx: &mut ViewContext<Self>) {
        if self.repo_root == root {
            return;
        }
        self.repo_root = root;
        if self.visible {
            self.rescan(ctx);
        }
        ctx.notify();
    }

    /// The tab is shown or hidden; it only reads the shelves while shown.
    pub fn set_visible(&mut self, visible: bool, ctx: &mut ViewContext<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        self.poll_generation += 1;
        if visible {
            self.rescan(ctx);
            self.schedule_poll(ctx);
        }
    }

    pub fn on_left_panel_focused(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.focus(&self.filter);
    }

    fn rescan(&mut self, ctx: &mut ViewContext<Self>) {
        self.scan_generation += 1;
        let generation = self.scan_generation;
        let shelves = grimoire::shelves(self.repo_root.as_deref());
        let _ = ctx.spawn(grimoire::read_shelves(shelves), move |me, pages, ctx| {
            if me.scan_generation != generation {
                return;
            }
            for page in &pages {
                for slot in PAGE_SLOTS {
                    me.row_mouse
                        .entry(format!("{}|{slot}", page.key()))
                        .or_default();
                }
                me.row_mouse
                    .entry(folder_key(page.shelf, &page.folder))
                    .or_default();
            }
            if me.pages != pages {
                me.pages = pages;
                ctx.notify();
            }
            if !me.loaded {
                me.loaded = true;
                ctx.notify();
            }
        });
    }

    fn schedule_poll(&mut self, ctx: &mut ViewContext<Self>) {
        let generation = self.poll_generation;
        let _ = ctx.spawn(Timer::after(POLL_INTERVAL), move |me, _, ctx| {
            if me.visible && me.poll_generation == generation {
                me.rescan(ctx);
                me.schedule_poll(ctx);
            }
        });
    }

    fn matching(&self) -> Vec<usize> {
        grimoire::search(&self.pages, &self.query)
    }

    fn mouse(&self, key: &str) -> MouseStateHandle {
        self.row_mouse.get(key).cloned().unwrap_or_default()
    }

    /// The repo's shelf folder, when the focused terminal is in a repo that
    /// can have one.
    fn repo_shelf(&self) -> Option<PathBuf> {
        grimoire::shelves(self.repo_root.as_deref())
            .into_iter()
            .find_map(|(shelf, dir)| (shelf == Shelf::Repo).then_some(dir))
    }

    /// Writes a new page with `write`, then opens it to edit.
    fn create(
        &mut self,
        dir: Option<PathBuf>,
        write: fn(&std::path::Path, chrono::DateTime<Local>) -> std::io::Result<PathBuf>,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(dir) = dir else {
            return;
        };
        match write(&dir, Local::now()) {
            Ok(path) => {
                self.rescan(ctx);
                ctx.dispatch_typed_action(&WorkspaceAction::OpenGrimoirePage { path, edit: true });
            }
            Err(error) => show_write_error(&dir, &error, ctx),
        }
    }
}

/// The error toast for a page that couldn't be written.
pub(crate) fn show_write_error<V: View>(
    dir: &std::path::Path,
    error: &std::io::Error,
    ctx: &mut ViewContext<V>,
) {
    let window_id = ctx.window_id();
    let toast = DismissibleToast::error(madness_themes::tf(
        "shellomancy.grimoire.writeFailed",
        &[
            ("path", &grimoire::tilde(&dir.to_string_lossy())),
            ("error", &error.to_string()),
        ],
    ));
    ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
        toast_stack.add_ephemeral_toast(toast, window_id, ctx);
    });
}

/// Does what a click on `page` asks, except `Copy` and `Peek`, which change
/// the clicked view's own state.
pub(crate) fn act_on_page<V: View>(page: &Page, click: &PageClick, ctx: &mut ViewContext<V>) {
    let click = match click {
        PageClick::Primary => match &page.kind {
            PageKind::Notebook => PageClick::Read,
            PageKind::Incantation(_) | PageKind::Script { runner: Some(_) } => PageClick::Cast,
            PageKind::Script { runner: None } => PageClick::Edit,
        },
        click => click.clone(),
    };
    let open = |edit: bool, ctx: &mut ViewContext<V>| {
        ctx.dispatch_typed_action(&WorkspaceAction::OpenGrimoirePage {
            path: page.path.clone(),
            edit,
        });
    };
    match (click, &page.kind) {
        (PageClick::Read, PageKind::Notebook) => open(false, ctx),
        (PageClick::Cast, PageKind::Incantation(workflow)) => {
            ctx.dispatch_typed_action(&WorkspaceAction::RunWorkflow {
                workflow: Arc::new(WorkflowType::Local(workflow.clone())),
                workflow_source: WorkflowSource::Local,
                workflow_selection_source: WorkflowSelectionSource::Undefined,
                argument_override: None,
            });
        }
        (
            PageClick::Cast,
            PageKind::Script {
                runner: Some(runner),
            },
        ) => {
            ctx.dispatch_typed_action(&WorkspaceAction::InsertInInput {
                content: runner.clone(),
                replace_buffer: true,
                ensure_agent_mode: false,
            });
        }
        (PageClick::Copy | PageClick::Peek, _) => {}
        // Reading a non-notebook, casting a notebook: there's only the file.
        _ => open(true, ctx),
    }
}

/// What `[copy]` copies: an incantation's command, else the page's text.
pub(crate) fn copy_text(page: &Page) -> String {
    match &page.kind {
        PageKind::Incantation(workflow) => workflow.content().to_owned(),
        _ => page.text.clone(),
    }
}

impl TypedActionView for GrimoireView {
    type Action = GrimoireAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            GrimoireAction::Page { key, click } => {
                let Some(page) = self.pages.iter().find(|page| page.key() == *key).cloned() else {
                    return;
                };
                match click {
                    PageClick::Peek => {
                        if !self.peeked.remove(key) {
                            self.peeked.insert(key.clone());
                        }
                        ctx.notify();
                    }
                    PageClick::Copy => {
                        ctx.clipboard()
                            .write(ClipboardContent::plain_text(copy_text(&page)));
                        self.copied = Some(format!("{key}|copy"));
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
                    click => act_on_page(&page, click, ctx),
                }
            }
            GrimoireAction::ToggleFolder(key) => {
                if !self.collapsed.remove(key) {
                    self.collapsed.insert(key.clone());
                }
                ctx.notify();
            }
            GrimoireAction::NewNotebook(shelf) => {
                let dir = match shelf {
                    Shelf::Repo => self.repo_shelf(),
                    Shelf::Global => grimoire::global_dir(),
                };
                self.create(dir, grimoire::new_notebook, ctx);
            }
            GrimoireAction::NewIncantation => {
                self.create(grimoire::global_dir(), grimoire::new_incantation, ctx);
            }
            GrimoireAction::OpenFolder => {
                if let Some(dir) = grimoire::global_dir() {
                    match grimoire::ensure_shelf(&dir) {
                        Ok(()) => ctx
                            .dispatch_typed_action(&WorkspaceAction::OpenInExplorer { path: dir }),
                        Err(error) => show_write_error(&dir, &error, ctx),
                    }
                }
            }
            GrimoireAction::Refresh => self.rescan(ctx),
        }
    }
}

impl View for GrimoireView {
    fn ui_name() -> &'static str {
        "GrimoireView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let body = if !self.loaded {
            muted(
                madness_themes::t("shellomancy.grimoire.loading").to_owned(),
                appearance,
            )
        } else if self.query.trim().is_empty() {
            self.render_shelves(appearance)
        } else {
            self.render_matches(appearance)
        };
        let scrollable = ClippedScrollable::vertical(
            self.scroll.clone(),
            Container::new(body)
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
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(self.render_header(appearance))
            .with_child(self.render_filter(appearance))
            .with_child(Shrinkable::new(1.0, scrollable).finish())
            .finish()
    }
}

impl GrimoireView {
    fn render_header(&self, appearance: &Appearance) -> Box<dyn Element> {
        let ui = appearance.ui_builder();
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Shrinkable::new(
                    1.,
                    ui.span(madness_themes::t("shellomancy.grimoire.title").to_owned())
                        .with_style(UiComponentStyles {
                            font_weight: Some(Weight::Semibold),
                            ..Default::default()
                        })
                        .build()
                        .finish(),
                )
                .finish(),
            );
        let buttons = [
            (
                Icon::Plus,
                "shellomancy.grimoire.newNotebook",
                &self.header_mouse.new_notebook,
                GrimoireAction::NewNotebook(Shelf::Global),
            ),
            (
                Icon::Workflow,
                "shellomancy.grimoire.newIncantation",
                &self.header_mouse.new_incantation,
                GrimoireAction::NewIncantation,
            ),
            (
                Icon::Folder,
                "shellomancy.grimoire.openFolder",
                &self.header_mouse.open_folder,
                GrimoireAction::OpenFolder,
            ),
            (
                Icon::Refresh,
                "shellomancy.grimoire.refresh",
                &self.header_mouse.refresh,
                GrimoireAction::Refresh,
            ),
        ];
        for (icon, tooltip_key, mouse, action) in buttons {
            row.add_child(header_icon_button(
                icon,
                tooltip_key,
                mouse,
                action,
                appearance,
            ));
        }
        Container::new(row.finish())
            .with_horizontal_padding(12.)
            .with_vertical_padding(6.)
            .finish()
    }

    fn render_filter(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        Container::new(
            Container::new(ChildView::new(&self.filter).finish())
                .with_padding(Padding::uniform(6.))
                .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                .finish(),
        )
        .with_horizontal_padding(12.)
        .with_padding_bottom(8.)
        .finish()
    }

    /// No filter: the repo's shelf, then the global one, grouped by folder.
    fn render_shelves(&self, appearance: &Appearance) -> Box<dyn Element> {
        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        let now = Local::now().timestamp();
        if let Some(repo_dir) = self.repo_shelf() {
            let repo_name = self
                .repo_root
                .as_ref()
                .and_then(|root| root.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let repo_pages: Vec<&Page> = self
                .pages
                .iter()
                .filter(|page| page.shelf == Shelf::Repo)
                .collect();
            column.add_child(shelf_heading(
                madness_themes::tf("shellomancy.grimoire.repoShelf", &[("repo", &repo_name)]),
                repo_pages.len(),
                &repo_dir,
                appearance,
            ));
            if repo_pages.is_empty() {
                let mut line = Wrap::row().with_run_spacing(2.);
                line.add_child(muted(
                    madness_themes::tf(
                        "shellomancy.grimoire.repoNone",
                        &[("path", &format!("{repo_name}/{}", grimoire::GRIMOIRE_DIR))],
                    ),
                    appearance,
                ));
                line.add_child(term_button(
                    self.header_mouse.start_repo.clone(),
                    madness_themes::t("shellomancy.grimoire.repoStart").to_owned(),
                    GrimoireAction::NewNotebook(Shelf::Repo),
                    appearance,
                ));
                column.add_child(
                    Container::new(line.finish())
                        .with_margin_bottom(8.)
                        .finish(),
                );
            } else {
                self.render_folders(&repo_pages, now, &mut column, appearance);
            }
        }

        let global_pages: Vec<&Page> = self
            .pages
            .iter()
            .filter(|page| page.shelf == Shelf::Global)
            .collect();
        if let Some(global_dir) = grimoire::global_dir() {
            column.add_child(shelf_heading(
                madness_themes::t("shellomancy.grimoire.globalShelf").to_owned(),
                global_pages.len(),
                &global_dir,
                appearance,
            ));
            if global_pages.is_empty() {
                column.add_child(self.render_empty(&global_dir, appearance));
            } else {
                self.render_folders(&global_pages, now, &mut column, appearance);
            }
        }
        column.finish()
    }

    /// A shelf's pages: loose ones first, then each folder under a heading
    /// that rolls it up.
    fn render_folders(
        &self,
        pages: &[&Page],
        now: i64,
        column: &mut Flex,
        appearance: &Appearance,
    ) {
        let mut current: Option<&str> = None;
        let mut rolled_up = false;
        for page in pages {
            if !page.folder.is_empty() && current != Some(page.folder.as_str()) {
                current = Some(page.folder.as_str());
                let key = folder_key(page.shelf, &page.folder);
                rolled_up = self.collapsed.contains(&key);
                let count = pages.iter().filter(|p| p.folder == page.folder).count();
                column.add_child(folder_heading(
                    &page.folder,
                    count,
                    rolled_up,
                    self.mouse(&key),
                    GrimoireAction::ToggleFolder(key),
                    appearance,
                ));
            }
            if page.folder.is_empty() || !rolled_up {
                column.add_child(self.render_card(page, None, false, now, appearance));
            }
        }
    }

    /// A filter: every matching page, best first, folder on the card.
    fn render_matches(&self, appearance: &Appearance) -> Box<dyn Element> {
        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        let matching = self.matching();
        column.add_child(muted(
            if matching.is_empty() {
                madness_themes::t("shellomancy.grimoire.noMatch").to_owned()
            } else {
                madness_themes::tf(
                    "shellomancy.grimoire.count",
                    &[("count", &matching.len().to_string())],
                )
            },
            appearance,
        ));
        let now = Local::now().timestamp();
        for index in matching {
            if let Some(page) = self.pages.get(index) {
                let hit = grimoire::hit_line(page, &self.query);
                column.add_child(self.render_card(page, hit, true, now, appearance));
            }
        }
        Container::new(column.finish()).finish()
    }

    fn render_card(
        &self,
        page: &Page,
        hit: Option<String>,
        show_folder: bool,
        now: i64,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let key = page.key();
        let mouse = |slot: &str| self.mouse(&format!("{key}|{slot}"));
        render_page(
            page,
            PageLook {
                mouse: &mouse,
                copied: self.copied.as_deref() == Some(format!("{key}|copy").as_str()),
                peeked: self.peeked.contains(&key),
                hit,
                show_folder,
                now,
            },
            |click| GrimoireAction::Page {
                key: key.clone(),
                click,
            },
            appearance,
        )
    }

    /// A blank global shelf: what goes in it, and buttons to start.
    fn render_empty(&self, dir: &std::path::Path, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let mut buttons = Wrap::row().with_run_spacing(2.);
        buttons.add_child(term_button(
            self.header_mouse.empty_notebook.clone(),
            madness_themes::t("shellomancy.grimoire.newNotebook").to_owned(),
            GrimoireAction::NewNotebook(Shelf::Global),
            appearance,
        ));
        buttons.add_child(term_button(
            self.header_mouse.empty_incantation.clone(),
            madness_themes::t("shellomancy.grimoire.newIncantation").to_owned(),
            GrimoireAction::NewIncantation,
            appearance,
        ));
        Container::new(
            Flex::column()
                .with_child(muted(
                    madness_themes::tf(
                        "shellomancy.grimoire.empty",
                        &[("path", &grimoire::tilde(&dir.to_string_lossy()))],
                    ),
                    appearance,
                ))
                .with_child(
                    Container::new(buttons.finish())
                        .with_margin_top(8.)
                        .finish(),
                )
                .finish(),
        )
        .with_uniform_padding(10.)
        .with_background(theme.surface_1())
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
        .finish()
    }
}

/// How a page card looks right now.
pub(crate) struct PageLook<'a> {
    /// The mouse state for one of the card's [`PAGE_SLOTS`].
    pub mouse: &'a dyn Fn(&str) -> MouseStateHandle,
    /// `[copy]` was just clicked.
    pub copied: bool,
    pub peeked: bool,
    /// The line a full-text match was found on.
    pub hit: Option<String>,
    /// Say which folder (and shelf) the page is in: the list isn't grouped.
    pub show_folder: bool,
    /// Unix seconds, for the age.
    pub now: i64,
}

/// One page's card. `wrap` turns a click into the host view's action.
pub(crate) fn render_page<A: warpui::Action + Clone>(
    page: &Page,
    look: PageLook,
    wrap: impl Fn(PageClick) -> A,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let colors = &theme.terminal_colors().normal;
    let (glyph, edge): (&str, ColorU) = match &page.kind {
        PageKind::Notebook => ("¶", colors.blue.into()),
        PageKind::Incantation(_) => ("$_", colors.magenta.into()),
        PageKind::Script { .. } => ("#!", colors.yellow.into()),
    };
    let dim = theme.sub_text_color(theme.background()).into_solid();
    let plain = theme.main_text_color(theme.background()).into_solid();

    let mut top = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Container::new(mono(glyph.to_owned(), edge, 11., appearance))
                .with_margin_right(6.)
                .finish(),
        )
        .with_child(
            Shrinkable::new(
                1.,
                appearance
                    .ui_builder()
                    .wrappable_text(page.title.clone(), true)
                    .with_style(UiComponentStyles {
                        font_size: Some(13.),
                        font_weight: Some(Weight::Semibold),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            )
            .finish(),
        );
    if let Some(modified) = page.modified {
        top.add_child(
            Container::new(mono(
                crate::mana_bar::api::age_label(modified, look.now),
                dim,
                11.,
                appearance,
            ))
            .with_margin_left(6.)
            .finish(),
        );
    }
    let mut content = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(top.finish());

    if look.show_folder {
        let mut place = Vec::new();
        if page.shelf == Shelf::Repo {
            place.push(madness_themes::t("shellomancy.grimoire.repoBadge").to_owned());
        }
        if !page.folder.is_empty() {
            place.push(format!("{}/", page.folder));
        }
        if !place.is_empty() {
            content.add_child(
                Container::new(mono(place.join(" · "), dim, 11., appearance))
                    .with_margin_top(2.)
                    .finish(),
            );
        }
    }
    if let PageKind::Incantation(workflow) = &page.kind {
        content.add_child(
            Container::new(mono(
                format!("$ {}", workflow.content()),
                plain,
                11.,
                appearance,
            ))
            .with_margin_top(4.)
            .finish(),
        );
        let arguments: Vec<String> = workflow
            .arguments()
            .iter()
            .map(|argument| format!("{{{{{}}}}}", argument.name()))
            .collect();
        if !arguments.is_empty() {
            content.add_child(
                Container::new(mono(
                    madness_themes::tf(
                        "shellomancy.grimoire.asksFor",
                        &[("args", &arguments.join(" "))],
                    ),
                    dim,
                    11.,
                    appearance,
                ))
                .with_margin_top(2.)
                .finish(),
            );
        }
    }
    if let Some(hit) = &look.hit
        && !page.title.to_lowercase().contains(&hit.to_lowercase())
    {
        content.add_child(
            Container::new(mono(format!("… {hit}"), dim, 11., appearance))
                .with_margin_top(4.)
                .finish(),
        );
    }

    let mut buttons = Wrap::row().with_run_spacing(2.);
    let mut button = |slot: &str, label_key: &'static str, click: PageClick| {
        buttons.add_child(term_button(
            (look.mouse)(slot),
            madness_themes::t(label_key).to_owned(),
            wrap(click),
            appearance,
        ));
    };
    match &page.kind {
        PageKind::Notebook => button("read", "shellomancy.grimoire.read", PageClick::Read),
        PageKind::Incantation(_) | PageKind::Script { runner: Some(_) } => {
            button("cast", "shellomancy.grimoire.cast", PageClick::Cast)
        }
        PageKind::Script { runner: None } => {}
    }
    button("edit", "shellomancy.grimoire.edit", PageClick::Edit);
    button(
        "copy",
        if look.copied {
            "shellomancy.search.copied"
        } else {
            "shellomancy.search.copy"
        },
        PageClick::Copy,
    );
    if !page.is_incantation() {
        button(
            "peek",
            if look.peeked {
                "shellomancy.grimoire.unpeek"
            } else {
                "shellomancy.grimoire.peek"
            },
            PageClick::Peek,
        );
    }
    content.add_child(
        Container::new(buttons.finish())
            .with_margin_top(6.)
            .finish(),
    );

    let body = look.peeked.then(|| {
        let mut lines: Vec<&str> = page.text.lines().take(MAX_PEEK_LINES + 1).collect();
        if lines.len() > MAX_PEEK_LINES {
            lines.truncate(MAX_PEEK_LINES);
            lines.push("…");
        }
        Container::new(mono(lines.join("\n"), plain, 11., appearance))
            .with_uniform_padding(8.)
            .with_background(theme.background())
            .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
            .finish()
    });
    card(
        content.finish(),
        body,
        (look.mouse)("row"),
        wrap(PageClick::Primary),
        edge,
        appearance,
    )
}

/// `Repo|regression/balena`.
fn folder_key(shelf: Shelf, folder: &str) -> String {
    format!("{shelf:?}|{folder}")
}

fn header_icon_button(
    icon: Icon,
    tooltip_key: &'static str,
    mouse: &MouseStateHandle,
    action: GrimoireAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let tooltip = appearance
        .ui_builder()
        .tool_tip(madness_themes::t(tooltip_key).to_owned())
        .build()
        .finish();
    let icon_color = theme.sub_text_color(theme.background()).into_solid();
    Container::new(
        icon_button(appearance, icon, false, mouse.clone())
            .with_tooltip(move || tooltip)
            .with_style(UiComponentStyles {
                font_color: Some(icon_color),
                height: Some(24.),
                width: Some(24.),
                padding: Some(Coords::uniform(4.)),
                ..Default::default()
            })
            .build()
            .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
            .with_cursor(Cursor::PointingHand)
            .finish(),
    )
    .with_margin_left(2.)
    .finish()
}

/// `THIS REPO · SHELLOMANCY  3        ~/lab/x/.mad/grimoire`
fn shelf_heading(
    label: String,
    count: usize,
    dir: &std::path::Path,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let accent = theme.accent().into_solid();
    let dim = theme.sub_text_color(theme.background()).into_solid();
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            appearance
                .ui_builder()
                .span(label.to_uppercase())
                .with_style(UiComponentStyles {
                    font_size: Some(12.),
                    font_weight: Some(Weight::Bold),
                    font_color: Some(accent),
                    ..Default::default()
                })
                .build()
                .finish(),
        );
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
    row.add_child(
        Shrinkable::new(
            1.,
            Container::new(mono(
                grimoire::tilde(&dir.to_string_lossy()),
                dim,
                10.,
                appearance,
            ))
            .with_margin_left(8.)
            .finish(),
        )
        .finish(),
    );
    Container::new(row.finish())
        .with_margin_top(8.)
        .with_margin_bottom(8.)
        .finish()
}

/// `▾ regression/  4`, rolling the folder up or out.
fn folder_heading(
    folder: &str,
    count: usize,
    rolled_up: bool,
    mouse: MouseStateHandle,
    action: GrimoireAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let (dim, hover) = (
        theme.sub_text_color(theme.background()).into_solid(),
        theme.surface_2(),
    );
    let label = mono(
        format!("{} {folder}/  {count}", if rolled_up { "▸" } else { "▾" }),
        dim,
        11.,
        appearance,
    );
    Container::new(
        Hoverable::new(mouse, move |state| {
            let mut container = Container::new(label)
                .with_horizontal_padding(4.)
                .with_vertical_padding(2.)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));
            if state.is_hovered() {
                container = container.with_background(hover);
            }
            container.finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .finish(),
    )
    .with_margin_bottom(6.)
    .finish()
}

/// A bracketed monospace button: `[cast]`.
fn term_button<A: warpui::Action + Clone>(
    mouse: MouseStateHandle,
    label: String,
    action: A,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let hover = theme.surface_3();
    let text = mono(
        format!("[{label}]"),
        theme.accent().into_solid(),
        11.,
        appearance,
    );
    Container::new(
        Hoverable::new(mouse, move |state| {
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
        .finish(),
    )
    .with_margin_right(4.)
    .finish()
}

/// A bordered card with a coloured left edge. The summary (`content`) is
/// clickable and tints on hover; the peeked `body` isn't, so a drag there
/// selects text instead of casting.
fn card<A: warpui::Action + Clone>(
    content: Box<dyn Element>,
    body: Option<Box<dyn Element>>,
    mouse: MouseStateHandle,
    action: A,
    edge: ColorU,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let (background, hover, border) = (theme.surface_1(), theme.surface_2(), theme.surface_3());
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
    Container::new(inner)
        .with_margin_bottom(8.)
        .with_border(Border::left(3.).with_border_color(edge))
        .with_corner_radius(CornerRadius::with_all(radius))
        .finish()
}

fn muted(text: String, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    Container::new(
        appearance
            .ui_builder()
            .wrappable_text(text, true)
            .with_style(UiComponentStyles {
                font_size: Some(12.),
                font_color: Some(theme.sub_text_color(theme.background()).into_solid()),
                ..Default::default()
            })
            .build()
            .finish(),
    )
    .with_margin_bottom(8.)
    .finish()
}

fn mono(text: String, color: ColorU, size: f32, appearance: &Appearance) -> Box<dyn Element> {
    appearance
        .ui_builder()
        .wrappable_text(text, true)
        .with_style(UiComponentStyles {
            font_family_id: Some(appearance.monospace_font_family()),
            font_size: Some(size),
            font_color: Some(color),
            ..Default::default()
        })
        .build()
        .finish()
}
