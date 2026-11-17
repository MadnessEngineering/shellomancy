//! Shellomancy: prompt chips fed by the Mana Bar's Omnispindle data.
//!
//! The chip generator only sees the session (no app models), so it just names
//! the project (the git repo the prompt is in). This view looks that project
//! up in [`ManaBar`] and re-renders when the data changes: "12 owed · 1 doing"
//! for the Omnispindle chip, "Integrations 6/8" for the Quest chip. It renders
//! nothing while signed out, outside a repo, or before there's data.

use std::time::Duration;

use warp_core::ui::Icon;
use warpui::r#async::Timer;
use warpui::elements::{Element, Empty, Hoverable, MouseStateHandle};
use warpui::platform::Cursor;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext};

use crate::appearance::Appearance;
use crate::mana_bar::{Fetch, List, ManaBar, ManaBarEvent, Quest, Todo};
use crate::workspace::WorkspaceAction;

/// How often a chip asks for fresh numbers. Requests already in flight (from
/// the Mana Bar or another chip) are shared.
const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Which Madness chip this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MadnessChip {
    /// Active todos for the repo.
    Omnispindle,
    /// The repo's least-finished active quest and its progress.
    Quest,
}

impl MadnessChip {
    fn list(self) -> List {
        match self {
            MadnessChip::Omnispindle => List::Todos,
            MadnessChip::Quest => List::Quests,
        }
    }
}

#[derive(Clone, Debug)]
pub enum MadnessChipAction {
    OpenManaBar,
}

pub struct MadnessChipView {
    kind: MadnessChip,
    project: Option<String>,
    mouse: MouseStateHandle,
}

impl MadnessChipView {
    pub fn new(kind: MadnessChip, project: Option<String>, ctx: &mut ViewContext<Self>) -> Self {
        if ctx.has_singleton_model::<ManaBar>() {
            ctx.subscribe_to_model(&ManaBar::handle(ctx), |_, _, event, ctx| match event {
                ManaBarEvent::Changed => ctx.notify(),
            });
        }
        let mut me = Self {
            kind,
            project,
            mouse: Default::default(),
        };
        me.fetch(false, ctx);
        me.schedule_refresh(ctx);
        me
    }

    /// Asks the Mana Bar for this chip's list: once, or again on refresh.
    fn fetch(&mut self, again: bool, ctx: &mut ViewContext<Self>) {
        let Some(project) = self.project.clone() else {
            return;
        };
        if !ctx.has_singleton_model::<ManaBar>() || crate::mana_bar::api_key(ctx).is_none() {
            return;
        }
        let (list, key) = (self.kind.list(), Some(project));
        ManaBar::handle(ctx).update(ctx, |mana_bar, ctx| {
            if again {
                mana_bar.refresh(list, &key, ctx);
            } else {
                mana_bar.ensure_loaded(list, &key, ctx);
            }
        });
    }

    fn schedule_refresh(&mut self, ctx: &mut ViewContext<Self>) {
        let _ = ctx.spawn(Timer::after(REFRESH_INTERVAL), |me, _, ctx| {
            me.fetch(true, ctx);
            me.schedule_refresh(ctx);
        });
    }

    /// The chip's text, or `None` when there's nothing worth showing.
    pub fn label(&self, app: &AppContext) -> Option<String> {
        let project = self.project.clone()?;
        if !app.has_singleton_model::<ManaBar>() || crate::mana_bar::api_key(app).is_none() {
            return None;
        }
        let mana_bar = ManaBar::as_ref(app);
        let key = Some(project);
        match self.kind {
            MadnessChip::Omnispindle => {
                omnispindle_label(mana_bar.todos(&key).and_then(Fetch::data)?)
            }
            MadnessChip::Quest => quest_label(mana_bar.quests(&key).and_then(Fetch::data)?),
        }
    }

    pub fn should_render(&self, app: &AppContext) -> bool {
        self.label(app).is_some()
    }

    pub fn icon(&self) -> Icon {
        match self.kind {
            MadnessChip::Omnispindle => Icon::CheckSkinny,
            MadnessChip::Quest => Icon::Lightning,
        }
    }
}

/// "12 owed · 1 doing"; `None` when nothing is owed.
pub fn omnispindle_label(todos: &[Todo]) -> Option<String> {
    if todos.is_empty() {
        return None;
    }
    let owed = madness_themes::tf(
        "shellomancy.chips.owed",
        &[("count", &todos.len().to_string())],
    );
    let doing = todos
        .iter()
        .filter(|todo| todo.status == "in_progress")
        .count();
    Some(if doing == 0 {
        owed
    } else {
        format!(
            "{owed} · {}",
            madness_themes::tf("shellomancy.chips.doing", &[("count", &doing.to_string())])
        )
    })
}

/// "Integrations 6/8" for the least-finished active quest; `None` without one.
pub fn quest_label(quests: &[Quest]) -> Option<String> {
    let quest = quests
        .iter()
        .filter(|quest| quest.total > 0 && quest.completed < quest.total)
        .min_by_key(|quest| quest.pct)?;
    Some(madness_themes::tf(
        "shellomancy.chips.quest",
        &[
            ("name", &short_name(&quest.name)),
            ("done", &quest.completed.to_string()),
            ("total", &quest.total.to_string()),
        ],
    ))
}

/// A quest name short enough for the prompt.
fn short_name(name: &str) -> String {
    const MAX: usize = 28;
    match name.char_indices().nth(MAX) {
        Some((cut, _)) => format!("{}…", name[..cut].trim_end()),
        None => name.to_owned(),
    }
}

impl Entity for MadnessChipView {
    type Event = ();
}

impl TypedActionView for MadnessChipView {
    type Action = MadnessChipAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            MadnessChipAction::OpenManaBar => {
                ctx.dispatch_typed_action(&WorkspaceAction::ToggleManaBar);
            }
        }
    }
}

impl View for MadnessChipView {
    fn ui_name() -> &'static str {
        "MadnessChipView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let Some(label) = self.label(app) else {
            return Empty::new().finish();
        };
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let text = appearance
            .ui_builder()
            .span(label)
            .with_style(UiComponentStyles {
                font_color: Some(theme.accent().into_solid()),
                font_weight: Some(warpui::fonts::Weight::Semibold),
                ..Default::default()
            })
            .build()
            .finish();
        Hoverable::new(self.mouse.clone(), move |_| text)
            .with_cursor(Cursor::PointingHand)
            .on_click(|ctx, _, _| ctx.dispatch_typed_action(MadnessChipAction::OpenManaBar))
            .finish()
    }
}

#[cfg(test)]
#[path = "madness_chip_tests.rs"]
mod tests;
