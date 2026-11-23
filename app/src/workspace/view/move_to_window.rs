//! Shellomancy: moving a tab to another window without dragging it: the tab menu's "Move to window" sidecar
//! and the quake send-and-back action. The tab's view tree moves whole, so whatever runs in its
//! panes keeps running.

use warpui::elements::DraggableState;
use warpui::geometry::rect::RectF;
use warpui::geometry::vector::vec2f;
use warpui::text_layout::ClipConfig;
use warpui::{AppContext, SingletonEntity, ViewContext, ViewHandle, WindowId};

use super::Workspace;
use crate::menu::{MenuItem, MenuItemFields};
use crate::root_view::{
    create_transferred_window, open_quake_mode_window_with_transferred_tab, quake_mode_window_id,
    show_quake_mode_window,
};
use crate::terminal::keys_settings::KeysSettings;
use crate::workspace::WorkspaceRegistry;
use crate::workspace::action::{TabWindowDestination, WorkspaceAction};
use crate::workspace::cross_window_tab_drag::CrossWindowTabDrag;

/// How far a tab's own new window sits from the window it left.
const NEW_WINDOW_OFFSET: f32 = 32.;

/// Longest window name the sidecar shows before cutting it short.
const MAX_WINDOW_LABEL_CHARS: usize = 28;

/// Where a move lands once its destination is resolved against the open windows.
enum Landing {
    Existing(WindowId, ViewHandle<Workspace>),
    NewQuakeWindow,
    /// A new window with these bounds.
    NewWindow(RectF),
}

impl Workspace {
    /// Where the tab at `tab_index` can go, in the order the sidecar lists them: the quake
    /// window first, then the other windows, then a new window of its own.
    pub(super) fn move_to_window_destinations(
        &self,
        tab_index: usize,
        ctx: &AppContext,
    ) -> Vec<TabWindowDestination> {
        if tab_index >= self.tabs.len() {
            return vec![];
        }
        let source = self.window_id;
        let quake = quake_mode_window_id();
        let source_is_quake = quake == Some(source);
        let mut other_windows = WorkspaceRegistry::as_ref(ctx)
            .all_workspaces(ctx)
            .into_iter()
            .filter(|(window_id, workspace)| {
                *window_id != source && !workspace.as_ref(ctx).is_tab_drag_preview()
            })
            .map(|(window_id, _)| window_id)
            .collect::<Vec<_>>();
        other_windows.sort();

        let quake_is_open = quake.is_some_and(|quake| other_windows.contains(&quake));
        let mut destinations = vec![];
        if !source_is_quake && (quake_is_open || *KeysSettings::as_ref(ctx).quake_mode_enabled) {
            destinations.push(TabWindowDestination::QuakeWindow);
        }
        destinations.extend(
            other_windows
                .into_iter()
                .filter(|window_id| Some(*window_id) != quake)
                .map(TabWindowDestination::Window),
        );
        // A lone tab in a normal window already has a window of its own.
        if self.tabs.len() > 1 || source_is_quake {
            destinations.push(TabWindowDestination::NewWindow);
        }
        destinations
    }

    pub(super) fn build_move_to_window_sidecar_items(
        &self,
        tab_index: usize,
        ctx: &AppContext,
    ) -> Vec<MenuItem<WorkspaceAction>> {
        self.move_to_window_destinations(tab_index, ctx)
            .into_iter()
            .map(|destination| {
                MenuItemFields::new(destination_label(destination, ctx))
                    // A name still too wide for the sidecar keeps its end: the last folders and
                    // the tab count.
                    .with_clip_config(ClipConfig::start())
                    .with_on_select_action(WorkspaceAction::MoveTabToWindow {
                        tab_index,
                        destination,
                    })
                    .into_item()
            })
            .collect()
    }

    /// Sends the active tab to the quake window, or from the quake window back to the most
    /// recently active other window (a new one when there is none).
    pub(super) fn move_active_tab_to_or_from_quake_window(&mut self, ctx: &mut ViewContext<Self>) {
        let source = self.window_id;
        let destination = if quake_mode_window_id() == Some(source) {
            ctx.windows()
                .recently_active_window_ids()
                .find(|window_id| {
                    *window_id != source
                        && WorkspaceRegistry::as_ref(ctx)
                            .get(*window_id, ctx)
                            .is_some_and(|workspace| !workspace.as_ref(ctx).is_tab_drag_preview())
                })
                .map_or(
                    TabWindowDestination::NewWindow,
                    TabWindowDestination::Window,
                )
        } else {
            TabWindowDestination::QuakeWindow
        };
        self.move_tab_to_window(self.active_tab_index, destination, ctx);
    }

    /// Moves the tab at `tab_index` into `destination`, closing this window when it was the last
    /// tab, and brings the destination forward with the tab active. Returns the window the tab
    /// landed in, or `None` when it can't go there (its own window, a new window for a window's
    /// only tab, or while a tab drag is in flight).
    pub(crate) fn move_tab_to_window(
        &mut self,
        tab_index: usize,
        destination: TabWindowDestination,
        ctx: &mut ViewContext<Self>,
    ) -> Option<WindowId> {
        // A tab drag in flight owns the windows it's moving between.
        if CrossWindowTabDrag::as_ref(ctx).source_window_id().is_some() {
            return None;
        }
        let source_window_id = self.window_id;
        let landing = self.resolve_landing(destination, ctx)?;
        let mut transferred_tab = self.get_tab_transfer_info_for_attach(tab_index, ctx)?;
        transferred_tab.draggable_state = DraggableState::default();
        let was_only_tab = self.tabs.len() == 1;

        ctx.unsubscribe_to_view(&transferred_tab.pane_group);
        let target_window_id = match landing {
            Landing::Existing(target_window_id, target_workspace) => {
                ctx.transfer_view_tree_to_window(
                    transferred_tab.pane_group.id(),
                    source_window_id,
                    target_window_id,
                );
                target_workspace.update(ctx, |workspace, ctx| {
                    let end = workspace.tabs.len();
                    workspace.insert_transferred_tab_at_index(transferred_tab, end, ctx);
                });
                target_window_id
            }
            Landing::NewQuakeWindow => {
                open_quake_mode_window_with_transferred_tab(transferred_tab, source_window_id, ctx)
            }
            Landing::NewWindow(bounds) => create_transferred_window(
                transferred_tab,
                source_window_id,
                bounds.size(),
                bounds.origin(),
                false,
                ctx,
            ),
        };

        if was_only_tab {
            // This window lists the moved tab until it finishes closing; holding saves off until
            // then keeps its panes from being persisted twice, as a tab drag does.
            CrossWindowTabDrag::handle(ctx).update(ctx, |drag, _| {
                drag.register_pending_source_close(source_window_id);
            });
            self.close_window_for_content_transfer(ctx);
        } else {
            self.remove_tab_without_undo(tab_index, ctx);
        }

        if quake_mode_window_id() == Some(target_window_id) {
            show_quake_mode_window(ctx);
        } else {
            ctx.windows().show_window_and_focus_app(target_window_id);
        }
        if let Some(target_workspace) = WorkspaceRegistry::as_ref(ctx).get(target_window_id, ctx) {
            target_workspace.update(ctx, |_, ctx| {
                // The window takes focus after `show_window_and_focus_app` returns, so the moved
                // tab is focused on the next tick, as a finished tab drag does.
                ctx.spawn(async {}, |workspace, _, ctx| {
                    workspace.focus_active_tab(ctx)
                });
            });
        }
        Some(target_window_id)
    }

    fn resolve_landing(
        &self,
        destination: TabWindowDestination,
        ctx: &AppContext,
    ) -> Option<Landing> {
        let source_window_id = self.window_id;
        let existing = |window_id: WindowId| {
            if window_id == source_window_id {
                return None;
            }
            let workspace = WorkspaceRegistry::as_ref(ctx).get(window_id, ctx)?;
            (!workspace.as_ref(ctx).is_tab_drag_preview())
                .then_some(Landing::Existing(window_id, workspace))
        };
        match destination {
            TabWindowDestination::Window(window_id) => existing(window_id),
            TabWindowDestination::QuakeWindow => match quake_mode_window_id() {
                Some(quake) if WorkspaceRegistry::as_ref(ctx).get(quake, ctx).is_some() => {
                    existing(quake)
                }
                _ => Some(Landing::NewQuakeWindow),
            },
            TabWindowDestination::NewWindow => {
                let source_is_quake = quake_mode_window_id() == Some(source_window_id);
                if self.tabs.len() == 1 && !source_is_quake {
                    return None;
                }
                let source_bounds = ctx.window_bounds(&source_window_id)?;
                Some(Landing::NewWindow(RectF::new(
                    source_bounds.origin() + vec2f(NEW_WINDOW_OFFSET, NEW_WINDOW_OFFSET),
                    source_bounds.size(),
                )))
            }
        }
    }
}

fn destination_label(destination: TabWindowDestination, ctx: &AppContext) -> String {
    match destination {
        TabWindowDestination::QuakeWindow => {
            madness_themes::t("shellomancy.tab.quakeWindow").into()
        }
        TabWindowDestination::NewWindow => madness_themes::t("shellomancy.tab.newWindow").into(),
        TabWindowDestination::Window(window_id) => {
            let Some(workspace) = WorkspaceRegistry::as_ref(ctx).get(window_id, ctx) else {
                return madness_themes::t("shellomancy.tab.untitledWindow").into();
            };
            let workspace = workspace.as_ref(ctx);
            let title = workspace
                .get_pane_group_view(workspace.active_tab_index())
                .map(|pane_group| pane_group.as_ref(ctx).display_title(ctx))
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| madness_themes::t("shellomancy.tab.untitledWindow").into());
            let title = shorten(&title, MAX_WINDOW_LABEL_CHARS);
            match workspace.tab_count().saturating_sub(1) {
                0 => title,
                more => madness_themes::tf(
                    "shellomancy.tab.windowWithMoreTabs",
                    &[("title", &title), ("more", &more.to_string())],
                ),
            }
        }
    }
}

/// `text` cut to at most `max_chars` characters, keeping its end (a path's last folders say the
/// most) behind a leading ellipsis.
fn shorten(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_owned();
    }
    let kept = text.chars().skip(count - (max_chars - 1));
    std::iter::once('…').chain(kept).collect()
}

#[cfg(test)]
#[path = "move_to_window_tests.rs"]
mod tests;
