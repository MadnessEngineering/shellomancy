//! Shellomancy's own tabs in the tools panel, and how to add one.
//!
//! A tab is a [`MadnessTab`] variant plus:
//! 1. its row in [`MadnessTab::spec`]: icon, title, keybinding;
//! 2. its view: a field of [`MadnessTabViews`], made in `new` and matched in
//!    the methods below.
//!
//! Everything else follows from those two: the toolbelt button, the toggle
//! keybinding ([`init`]), saved panel state (`LeftPanelDisplayedTab`),
//! being shown and hidden, and following the focused terminal's repo.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use warp_core::ui::Icon;
use warpui::elements::{ChildView, Element};
use warpui::keymap::{BindingDescription, EditableBinding};
use warpui::{AppContext, ViewContext, ViewHandle};

use super::grimoire_view::GrimoireView;
use super::left_panel::LeftPanelView;
use super::mana_bar_view::ManaBarView;
use super::{LEFT_PANEL_GRIMOIRE_BINDING_NAME, LEFT_PANEL_MANA_BAR_BINDING_NAME};
use crate::settings_view::flags;
use crate::util::bindings::BindingGroup;
use crate::workspace::WorkspaceAction;

/// A keybinding per tab that toggles the tools panel onto it.
pub fn init(app: &mut AppContext) {
    use warpui::keymap::macros::*;

    app.register_editable_bindings(MadnessTab::ALL.map(|tab| {
        let spec = tab.spec();
        EditableBinding::new(
            spec.binding_name,
            BindingDescription::new(madness_themes::t(spec.binding_key)),
            WorkspaceAction::ToggleMadnessTab(tab),
        )
        .with_group(BindingGroup::Navigation.as_str())
        .with_context_predicate(id!("Workspace") & id!(flags::SHOW_MADNESS_TABS))
        .with_mac_key_binding(spec.mac_key)
        .with_linux_or_windows_key_binding(spec.linux_or_windows_key)
    }));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MadnessTab {
    /// Omnispindle: todos, quests, lessons, the Log.
    ManaBar,
    /// Scratch notebooks, workflows and scripts on disk.
    Grimoire,
}

/// What the toolbelt and keymap need to know about a tab.
pub struct MadnessTabSpec {
    pub icon: Icon,
    /// Voice key of the tab's name (tooltip, single-tab panel button).
    pub title_key: &'static str,
    pub binding_name: &'static str,
    /// Voice key of the keybinding's description.
    pub binding_key: &'static str,
    pub mac_key: &'static str,
    pub linux_or_windows_key: &'static str,
}

impl MadnessTab {
    /// In toolbelt order.
    pub const ALL: [MadnessTab; 2] = [MadnessTab::ManaBar, MadnessTab::Grimoire];

    pub fn spec(self) -> MadnessTabSpec {
        match self {
            MadnessTab::ManaBar => MadnessTabSpec {
                icon: Icon::Lightning,
                title_key: "shellomancy.manaBar.title",
                binding_name: LEFT_PANEL_MANA_BAR_BINDING_NAME,
                binding_key: "shellomancy.manaBar.binding",
                // Drive's old binding: the Mana Bar took its slot.
                mac_key: "ctrl-4",
                linux_or_windows_key: "alt-4",
            },
            MadnessTab::Grimoire => MadnessTabSpec {
                icon: Icon::BookOpen,
                title_key: "shellomancy.grimoire.title",
                binding_name: LEFT_PANEL_GRIMOIRE_BINDING_NAME,
                binding_key: "shellomancy.grimoire.binding",
                mac_key: "ctrl-5",
                linux_or_windows_key: "alt-5",
            },
        }
    }

    pub fn title(self) -> &'static str {
        madness_themes::t(self.spec().title_key)
    }
}

/// Where the focused terminal is, for the tabs that follow it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MadnessFocus {
    /// The focused terminal's repo, when it's local.
    pub repo_root: Option<PathBuf>,
    /// Its Omnispindle project scope (`mana_bar::api::project_scope`).
    pub project: Option<String>,
}

/// One view per tab, made once per window.
pub struct MadnessTabViews {
    mana_bar: ViewHandle<ManaBarView>,
    grimoire: ViewHandle<GrimoireView>,
}

impl MadnessTabViews {
    pub fn new(ctx: &mut ViewContext<LeftPanelView>) -> Self {
        Self {
            mana_bar: ctx.add_typed_action_view(ManaBarView::new),
            grimoire: ctx.add_typed_action_view(GrimoireView::new),
        }
    }

    pub fn mana_bar(&self) -> &ViewHandle<ManaBarView> {
        &self.mana_bar
    }

    /// The tab's content.
    pub fn child(&self, tab: MadnessTab) -> Box<dyn Element> {
        match tab {
            MadnessTab::ManaBar => ChildView::new(&self.mana_bar).finish(),
            MadnessTab::Grimoire => ChildView::new(&self.grimoire).finish(),
        }
    }

    /// The panel took focus while showing `tab`.
    pub fn focus(&self, tab: MadnessTab, ctx: &mut ViewContext<LeftPanelView>) {
        match tab {
            MadnessTab::ManaBar => ctx.focus(&self.mana_bar),
            MadnessTab::Grimoire => ctx.focus(&self.grimoire),
        }
    }

    /// The panel was opened onto `tab` (a click or keybinding).
    pub fn on_left_panel_focused(&self, tab: MadnessTab, ctx: &mut ViewContext<LeftPanelView>) {
        match tab {
            MadnessTab::ManaBar => self
                .mana_bar
                .update(ctx, |view, ctx| view.on_left_panel_focused(ctx)),
            MadnessTab::Grimoire => self
                .grimoire
                .update(ctx, |view, ctx| view.on_left_panel_focused(ctx)),
        }
    }

    /// Which tab is on screen, if any: tabs only fetch and poll while shown.
    pub fn set_shown(&self, shown: Option<MadnessTab>, ctx: &mut ViewContext<LeftPanelView>) {
        let visible = |tab| shown == Some(tab);
        self.mana_bar.update(ctx, |view, ctx| {
            view.set_visible(visible(MadnessTab::ManaBar), ctx)
        });
        self.grimoire.update(ctx, |view, ctx| {
            view.set_visible(visible(MadnessTab::Grimoire), ctx)
        });
    }

    /// The focused terminal moved to another repo or project.
    pub fn set_focus(&self, focus: &MadnessFocus, ctx: &mut ViewContext<LeftPanelView>) {
        self.mana_bar.update(ctx, |view, ctx| {
            view.set_repo_root(focus.repo_root.clone(), ctx);
            view.set_project(focus.project.clone(), ctx);
        });
        self.grimoire.update(ctx, |view, ctx| {
            view.set_repo_root(focus.repo_root.clone(), ctx)
        });
    }
}
