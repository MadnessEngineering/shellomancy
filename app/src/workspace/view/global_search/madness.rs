//! Shellomancy: the search panel's Spells and Scrolls scopes.
//!
//! Spells searches command history: every live session's history (the shell's
//! history file plus what ran in Shellomancy, with its folder and exit code),
//! merged, deduplicated and newest first. A spell goes back into the focused
//! terminal's input with a click, runs, copies, or becomes a lesson.
//!
//! Scrolls searches the signed-in account's Omnispindle lessons on
//! madnessinteractive.cc (`GET /api/lessons?search=`); a blank query lists the
//! newest. Nothing is fetched while signed out.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, Local};
use pathfinder_color::ColorU;
use serde_json::Value;
use warpui::r#async::Timer;
use warpui::clipboard::ClipboardContent;
use warpui::elements::{
    Border, ClippedScrollStateHandle, ClippedScrollable, Container, CornerRadius,
    CrossAxisAlignment, Element, Fill, Flex, Hoverable, MainAxisAlignment, MainAxisSize,
    MouseStateHandle, ParentElement, Radius, ScrollbarWidth, Shrinkable, Wrap,
};
use warpui::fonts::Weight;
use warpui::platform::Cursor;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext};

use crate::appearance::Appearance;
use crate::madness_account::{MADNESS_API_ROOT, error_reason};
use crate::mana_bar::api::{DASHBOARD_URL, Lesson, parse_lessons};
use crate::terminal::history::{History, HistoryEntry};
use crate::view_components::DismissibleToast;
use crate::workspace::{ToastStack, WorkspaceAction};

/// Spells shown at once; the rest are a narrower query away.
const MAX_SPELLS: usize = 200;
const LESSON_LIMIT: u32 = 50;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const COPIED_FLASH: Duration = Duration::from_millis(1500);
const SCROLLBAR_WIDTH: ScrollbarWidth = ScrollbarWidth::Auto;
/// How much of a scroll's body shows before it's unrolled.
const SNIPPET_CHARS: usize = 160;

/// Which kind of thing the search panel searches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchScope {
    #[default]
    Files,
    Spells,
    Scrolls,
}

impl SearchScope {
    pub const ALL: [SearchScope; 3] = [
        SearchScope::Files,
        SearchScope::Spells,
        SearchScope::Scrolls,
    ];

    pub fn voice_key(self) -> &'static str {
        match self {
            SearchScope::Files => "shellomancy.search.scopeFiles",
            SearchScope::Spells => "shellomancy.search.scopeSpells",
            SearchScope::Scrolls => "shellomancy.search.scopeScrolls",
        }
    }

    pub fn placeholder_key(self) -> &'static str {
        match self {
            SearchScope::Files => "shellomancy.search.placeholderFiles",
            SearchScope::Spells => "shellomancy.search.placeholderSpells",
            SearchScope::Scrolls => "shellomancy.search.placeholderScrolls",
        }
    }
}

/// One command from history, as the Spells list shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpellHit {
    pub command: String,
    pub pwd: Option<String>,
    pub exit_code: Option<i32>,
    pub at: Option<DateTime<Local>>,
    pub git_head: Option<String>,
}

/// Merges history lists (each oldest first) into one list of distinct
/// commands, newest first. A command's newest run wins; runs with a time sort
/// by it, and the rest (plain history-file lines) by how recent they are in
/// their list.
pub fn merge_history<'a>(lists: impl IntoIterator<Item = Vec<&'a HistoryEntry>>) -> Vec<SpellHit> {
    // command -> (hit, recency rank within its list: 0 is newest)
    let mut best: HashMap<String, (SpellHit, usize)> = HashMap::new();
    for list in lists {
        for (rank, entry) in list.into_iter().rev().enumerate() {
            let command = entry.command.trim();
            if command.is_empty() {
                continue;
            }
            let hit = SpellHit {
                command: command.to_owned(),
                pwd: entry.pwd.clone(),
                exit_code: entry.exit_code.map(|code| code.value()),
                at: entry.start_ts,
                git_head: entry.git_head.clone(),
            };
            match best.get_mut(command) {
                Some((kept, kept_rank)) => {
                    let newer = match (hit.at, kept.at) {
                        (Some(new), Some(old)) => new > old,
                        (Some(_), None) => true,
                        (None, Some(_)) => false,
                        (None, None) => rank < *kept_rank,
                    };
                    if newer {
                        *kept = hit;
                        *kept_rank = rank;
                    }
                }
                None => {
                    best.insert(command.to_owned(), (hit, rank));
                }
            }
        }
    }
    let mut hits: Vec<(SpellHit, usize)> = best.into_values().collect();
    hits.sort_by(|(a, a_rank), (b, b_rank)| match (a.at, b.at) {
        (Some(a_at), Some(b_at)) => b_at.cmp(&a_at),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a_rank.cmp(b_rank),
    });
    hits.into_iter().map(|(hit, _)| hit).collect()
}

/// The indices of the spells whose command contains every word of `query`,
/// ignoring case, at most `limit` of them.
pub fn matching_spells(spells: &[SpellHit], query: &str, limit: usize) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    spells
        .iter()
        .enumerate()
        .filter(|(_, spell)| {
            let command = spell.command.to_lowercase();
            words.iter().all(|word| command.contains(word.as_str()))
        })
        .map(|(index, _)| index)
        .take(limit)
        .collect()
}

/// `GET /api/lessons`, searching for `query` when there is one. The server
/// reads `search` as a regular expression, so the query is escaped first.
pub fn lessons_search_url(query: &str) -> String {
    let mut url = format!("{MADNESS_API_ROOT}/lessons?limit={LESSON_LIMIT}");
    let query = query.trim();
    if !query.is_empty() {
        url.push_str("&search=");
        url.push_str(&urlencoding::encode(&regex::escape(query)));
    }
    url
}

async fn search_lessons(key: String, query: String) -> Result<Vec<Lesson>, String> {
    let response = http_client::Client::new_external()
        .get(lessons_search_url(&query))
        .bearer_auth(&key)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let status = response.status().as_u16();
    let reply: Value = response.json().await.unwrap_or_default();
    if (200..300).contains(&status) {
        Ok(parse_lessons(&reply))
    } else {
        Err(error_reason(status, &reply))
    }
}

/// The start of a scroll's body, on one line.
pub fn snippet(body: &str) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(SNIPPET_CHARS) {
        Some((cut, _)) => format!("{}…", &flat[..cut]),
        None => flat,
    }
}

#[derive(Clone, Debug, Default)]
enum Scrolls {
    #[default]
    Idle,
    Loading,
    Loaded(Vec<Lesson>),
    Failed(String),
}

#[derive(Clone, Debug)]
pub enum MadnessSearchAction {
    /// Puts the spell in the focused terminal's input.
    Insert(String),
    Run(String),
    Copy {
        button: String,
        text: String,
    },
    /// Writes the spell down as a lesson.
    Scroll(usize),
    ToggleScroll(String),
    OpenUrl(String),
}

pub struct MadnessSearchView {
    scope: SearchScope,
    query: String,
    /// Every distinct command in history, newest first; rebuilt when the
    /// Spells scope is entered or searched.
    spells: Vec<SpellHit>,
    /// Indices into `spells` that match the query.
    matching: Vec<usize>,
    scrolls: Scrolls,
    /// Bumped per lesson search so a slow reply to an old query is dropped.
    scroll_generation: u64,
    expanded: HashSet<String>,
    copied: Option<String>,
    copy_generation: u64,
    row_mouse: HashMap<String, MouseStateHandle>,
    scroll_state: ClippedScrollStateHandle,
}

impl Entity for MadnessSearchView {
    type Event = ();
}

impl MadnessSearchView {
    pub fn new(_ctx: &mut ViewContext<Self>) -> Self {
        Self {
            scope: SearchScope::Spells,
            query: String::new(),
            spells: Vec::new(),
            matching: Vec::new(),
            scrolls: Scrolls::Idle,
            scroll_generation: 0,
            expanded: HashSet::new(),
            copied: None,
            copy_generation: 0,
            row_mouse: HashMap::new(),
            scroll_state: Default::default(),
        }
    }

    /// Searches `scope` for `query` (blank lists the newest).
    pub fn search(&mut self, scope: SearchScope, query: String, ctx: &mut ViewContext<Self>) {
        let scope_changed = self.scope != scope;
        self.scope = scope;
        self.query = query;
        match scope {
            SearchScope::Files => {}
            SearchScope::Spells => {
                if scope_changed || self.spells.is_empty() || self.query.is_empty() {
                    self.reload_spells(ctx);
                }
                self.rematch_spells();
            }
            SearchScope::Scrolls => self.search_scrolls(ctx),
        }
        ctx.notify();
    }

    /// Enter in the query box: the first spell goes into the input.
    pub fn activate_first(&mut self, ctx: &mut ViewContext<Self>) {
        if self.scope == SearchScope::Spells
            && let Some(spell) = self.matching.first().and_then(|i| self.spells.get(*i))
        {
            insert_spell(&spell.command, ctx);
        }
    }

    fn reload_spells(&mut self, ctx: &mut ViewContext<Self>) {
        let history = History::as_ref(ctx);
        let mut session_ids: Vec<_> = history.all_live_session_ids().into_iter().collect();
        session_ids.sort_by_key(|id| format!("{id:?}"));
        self.spells = merge_history(
            session_ids
                .into_iter()
                .filter_map(|id| history.commands(id)),
        );
    }

    fn rematch_spells(&mut self) {
        self.matching = matching_spells(&self.spells, &self.query, MAX_SPELLS);
        for index in self.matching.clone() {
            for slot in ["row", "run", "copy", "scroll"] {
                self.row_mouse
                    .entry(format!("spell:{index}|{slot}"))
                    .or_default();
            }
        }
    }

    fn search_scrolls(&mut self, ctx: &mut ViewContext<Self>) {
        self.scroll_generation += 1;
        let Some(key) = crate::mana_bar::api_key(ctx) else {
            self.scrolls = Scrolls::Idle;
            return;
        };
        self.scrolls = Scrolls::Loading;
        let generation = self.scroll_generation;
        let query = self.query.clone();
        let _ = ctx.spawn(search_lessons(key, query), move |me, result, ctx| {
            if me.scroll_generation != generation {
                return;
            }
            me.scrolls = match result {
                Ok(lessons) => {
                    for lesson in &lessons {
                        for slot in ["row", "copy", "id", "open"] {
                            me.row_mouse
                                .entry(format!("scroll:{}|{slot}", lesson.id))
                                .or_default();
                        }
                    }
                    Scrolls::Loaded(lessons)
                }
                Err(error) => Scrolls::Failed(error),
            };
            ctx.notify();
        });
    }

    fn mouse(&self, key: &str) -> MouseStateHandle {
        self.row_mouse.get(key).cloned().unwrap_or_default()
    }

    fn spell_to_scroll(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        let Some(spell) = self.spells.get(index).cloned() else {
            return;
        };
        let window_id = ctx.window_id();
        let show = move |toast: DismissibleToast<WorkspaceAction>, ctx: &mut ViewContext<Self>| {
            ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                toast_stack.add_ephemeral_toast(toast, window_id, ctx);
            });
        };
        let Some(key) = crate::mana_bar::api_key(ctx) else {
            show(
                DismissibleToast::error(
                    madness_themes::t("shellomancy.spell.signInFirst").to_owned(),
                ),
                ctx,
            );
            return;
        };
        let spell = crate::spell_todo::Spell {
            command: spell.command,
            output: String::new(),
            cwd: spell.pwd,
            git_branch: None,
            exit_code: spell.exit_code,
        };
        let _ = ctx.spawn(
            async move { crate::spell_todo::create_lesson(&key, spell).await },
            move |_me, result, ctx| {
                let toast = match result {
                    Ok(lesson) => DismissibleToast::success(madness_themes::tf(
                        "shellomancy.spell.lessonCreated",
                        &[("topic", &lesson.topic)],
                    )),
                    Err(message) => DismissibleToast::error(message),
                };
                show(toast, ctx);
            },
        );
    }
}

fn insert_spell(command: &str, ctx: &mut ViewContext<MadnessSearchView>) {
    ctx.dispatch_typed_action(&WorkspaceAction::InsertInInput {
        content: command.to_owned(),
        replace_buffer: true,
        ensure_agent_mode: false,
    });
}

impl TypedActionView for MadnessSearchView {
    type Action = MadnessSearchAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            MadnessSearchAction::Insert(command) => insert_spell(command, ctx),
            MadnessSearchAction::Run(command) => {
                ctx.dispatch_typed_action(&WorkspaceAction::RunCommand(command.clone()));
            }
            MadnessSearchAction::Copy { button, text } => {
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
            MadnessSearchAction::Scroll(index) => self.spell_to_scroll(*index, ctx),
            MadnessSearchAction::ToggleScroll(id) => {
                if !self.expanded.remove(id) {
                    self.expanded.insert(id.clone());
                }
                ctx.notify();
            }
            MadnessSearchAction::OpenUrl(url) => ctx.open_url(url),
        }
    }
}

impl View for MadnessSearchView {
    fn ui_name() -> &'static str {
        "MadnessSearchView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let body = match self.scope {
            SearchScope::Spells => self.render_spells(appearance),
            SearchScope::Scrolls => self.render_scrolls(app, appearance),
            SearchScope::Files => Flex::column().finish(),
        };
        let theme = appearance.theme();
        ClippedScrollable::vertical(
            self.scroll_state.clone(),
            Container::new(body)
                .with_horizontal_padding(12.)
                .with_padding_bottom(12.)
                .finish(),
            SCROLLBAR_WIDTH,
            theme.disabled_text_color(theme.background()).into(),
            theme.main_text_color(theme.background()).into(),
            Fill::None,
        )
        .finish()
    }
}

impl MadnessSearchView {
    fn render_spells(&self, appearance: &Appearance) -> Box<dyn Element> {
        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        let status = if self.spells.is_empty() {
            madness_themes::t("shellomancy.search.spellsNoHistory").to_owned()
        } else if self.matching.is_empty() {
            madness_themes::t("shellomancy.search.spellsEmpty").to_owned()
        } else if self.query.trim().is_empty() {
            madness_themes::t("shellomancy.search.spellsRecent").to_owned()
        } else {
            madness_themes::tf(
                "shellomancy.search.spellsCount",
                &[("count", &self.matching.len().to_string())],
            )
        };
        column.add_child(status_line(status, appearance));
        let now = Local::now();
        for index in &self.matching {
            if let Some(spell) = self.spells.get(*index) {
                column.add_child(self.render_spell(*index, spell, now, appearance));
            }
        }
        column.finish()
    }

    fn render_spell(
        &self,
        index: usize,
        spell: &SpellHit,
        now: DateTime<Local>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let colors = &theme.terminal_colors().normal;
        let (green, red): (ColorU, ColorU) = (colors.green.into(), colors.red.into());
        let key = format!("spell:{index}");
        let command = mono(
            format!("$ {}", spell.command),
            theme.main_text_color(theme.background()).into_solid(),
            12.,
            appearance,
        );
        let mut meta = Vec::new();
        if let Some(code) = spell.exit_code {
            meta.push(if code == 0 {
                "✓".to_owned()
            } else {
                format!("✗ {code}")
            });
        }
        if let Some(at) = spell.at {
            meta.push(crate::mana_bar::api::age_label(
                at.timestamp(),
                now.timestamp(),
            ));
        }
        if let Some(pwd) = &spell.pwd {
            meta.push(tilde(pwd));
        }
        let meta_color = match spell.exit_code {
            Some(0) => green,
            Some(_) => red,
            None => theme.sub_text_color(theme.background()).into_solid(),
        };
        let mut buttons = Wrap::row().with_run_spacing(2.);
        buttons.add_child(self.term_button(
            &format!("{key}|run"),
            madness_themes::t("shellomancy.search.run"),
            MadnessSearchAction::Run(spell.command.clone()),
            appearance,
        ));
        buttons.add_child(self.copy_button(
            &format!("{key}|copy"),
            spell.command.clone(),
            appearance,
        ));
        buttons.add_child(self.term_button(
            &format!("{key}|scroll"),
            madness_themes::t("shellomancy.search.toScroll"),
            MadnessSearchAction::Scroll(index),
            appearance,
        ));
        let bottom = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Shrinkable::new(1., mono(meta.join("  "), meta_color, 11., appearance)).finish(),
            )
            .with_child(buttons.finish())
            .finish();
        let content = Flex::column()
            .with_child(command)
            .with_child(Container::new(bottom).with_margin_top(4.).finish())
            .finish();
        row(
            content,
            self.mouse(&format!("{key}|row")),
            MadnessSearchAction::Insert(spell.command.clone()),
            appearance,
        )
    }

    fn render_scrolls(&self, app: &AppContext, appearance: &Appearance) -> Box<dyn Element> {
        let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        if crate::mana_bar::api_key(app).is_none() {
            column.add_child(status_line(
                madness_themes::t("shellomancy.search.scrollsSignedOut").to_owned(),
                appearance,
            ));
            return column.finish();
        }
        match &self.scrolls {
            Scrolls::Idle => {}
            Scrolls::Loading => column.add_child(status_line(
                madness_themes::t("shellomancy.search.scrollsLoading").to_owned(),
                appearance,
            )),
            Scrolls::Failed(error) => column.add_child(status_line(
                madness_themes::tf("shellomancy.search.scrollsError", &[("error", error)]),
                appearance,
            )),
            Scrolls::Loaded(lessons) => {
                column.add_child(status_line(
                    if lessons.is_empty() {
                        madness_themes::t("shellomancy.search.scrollsEmpty").to_owned()
                    } else {
                        madness_themes::tf(
                            "shellomancy.search.scrollsCount",
                            &[("count", &lessons.len().to_string())],
                        )
                    },
                    appearance,
                ));
                let now = Local::now().timestamp();
                for lesson in lessons {
                    column.add_child(self.render_scroll(lesson, now, appearance));
                }
            }
        }
        column.finish()
    }

    fn render_scroll(
        &self,
        lesson: &Lesson,
        now: i64,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let magenta: ColorU = theme.terminal_colors().normal.magenta.into();
        let dim = theme.sub_text_color(theme.background()).into_solid();
        let key = format!("scroll:{}", lesson.id);
        let expanded = self.expanded.contains(&lesson.id);
        let mut top = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(mono(
                lesson.language.to_uppercase(),
                magenta,
                10.,
                appearance,
            ));
        top.add_child(mono(
            crate::mana_bar::api::age_label(lesson.created_at, now),
            dim,
            11.,
            appearance,
        ));
        let topic = appearance
            .ui_builder()
            .wrappable_text(lesson.topic.clone(), true)
            .with_style(UiComponentStyles {
                font_size: Some(13.),
                font_weight: Some(Weight::Semibold),
                ..Default::default()
            })
            .build()
            .finish();
        let mut content = Flex::column()
            .with_child(top.finish())
            .with_child(Container::new(topic).with_margin_top(4.).finish());
        if !lesson.tags.is_empty() {
            content.add_child(
                Container::new(mono(
                    lesson
                        .tags
                        .iter()
                        .map(|tag| format!("#{tag}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    dim,
                    11.,
                    appearance,
                ))
                .with_margin_top(4.)
                .finish(),
            );
        }
        if expanded {
            let body = Container::new(mono(
                lesson.body.trim().to_owned(),
                theme.main_text_color(theme.background()).into_solid(),
                11.,
                appearance,
            ))
            .with_uniform_padding(8.)
            .with_background(theme.background())
            .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
            .finish();
            content.add_child(Container::new(body).with_margin_top(6.).finish());
            let mut buttons = Wrap::row().with_run_spacing(2.);
            buttons.add_child(self.copy_button(
                &format!("{key}|copy"),
                lesson.body.clone(),
                appearance,
            ));
            buttons.add_child(self.term_button(
                &format!("{key}|id"),
                if self.copied.as_deref() == Some(format!("{key}|id").as_str()) {
                    madness_themes::t("shellomancy.search.copied")
                } else {
                    madness_themes::t("shellomancy.search.copyId")
                },
                MadnessSearchAction::Copy {
                    button: format!("{key}|id"),
                    text: lesson.id.clone(),
                },
                appearance,
            ));
            buttons.add_child(self.term_button(
                &format!("{key}|open"),
                madness_themes::t("shellomancy.search.open"),
                MadnessSearchAction::OpenUrl(DASHBOARD_URL.to_owned()),
                appearance,
            ));
            content.add_child(
                Container::new(buttons.finish())
                    .with_margin_top(6.)
                    .finish(),
            );
        } else if !lesson.body.trim().is_empty() {
            content.add_child(
                Container::new(mono(snippet(&lesson.body), dim, 11., appearance))
                    .with_margin_top(4.)
                    .finish(),
            );
        }
        row(
            content.finish(),
            self.mouse(&format!("{key}|row")),
            MadnessSearchAction::ToggleScroll(lesson.id.clone()),
            appearance,
        )
    }

    /// `[copy]`, or `[copied ✓]` for a moment after a click.
    fn copy_button(&self, button: &str, text: String, appearance: &Appearance) -> Box<dyn Element> {
        let label = if self.copied.as_deref() == Some(button) {
            madness_themes::t("shellomancy.search.copied")
        } else {
            madness_themes::t("shellomancy.search.copy")
        };
        self.term_button(
            button,
            label,
            MadnessSearchAction::Copy {
                button: button.to_owned(),
                text,
            },
            appearance,
        )
    }

    /// A bracketed monospace button: `[run]`.
    fn term_button(
        &self,
        mouse_key: &str,
        label: &str,
        action: MadnessSearchAction,
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
            Hoverable::new(self.mouse(mouse_key), move |state| {
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
        .with_margin_left(4.)
        .finish()
    }
}

/// A result row: hover tint, click does `action`.
fn row(
    content: Box<dyn Element>,
    mouse: MouseStateHandle,
    action: MadnessSearchAction,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let (idle, hover, border) = (theme.surface_1(), theme.surface_2(), theme.surface_3());
    Container::new(
        Hoverable::new(mouse, move |state| {
            Container::new(content)
                .with_uniform_padding(8.)
                .with_background(if state.is_hovered() { hover } else { idle })
                .with_border(Border::all(1.).with_border_fill(border))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action.clone()))
        .finish(),
    )
    .with_margin_bottom(6.)
    .finish()
}

fn status_line(text: String, appearance: &Appearance) -> Box<dyn Element> {
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

/// `~/lab/x` for a path under the home folder.
fn tilde(path: &str) -> String {
    match dirs::home_dir().and_then(|home| home.to_str().map(str::to_owned)) {
        Some(home) if path == home => "~".to_owned(),
        Some(home) if path.starts_with(&format!("{home}/")) => format!("~{}", &path[home.len()..]),
        _ => path.to_owned(),
    }
}

#[cfg(test)]
#[path = "madness_tests.rs"]
mod tests;
