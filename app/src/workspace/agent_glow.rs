//! Shellomancy: tabs glow with what their Claude sessions need. Amber, blinking, while one waits
//! on you; the working color while one runs a turn; green or red once a turn ends, until you look
//! at its tab. The statuses are the ones the bundled Claude plugin reports to
//! `CLIAgentSessionsModel`; workspaces feed them in, since they know which tab is in view.

use std::collections::HashMap;
use std::time::Duration;

use pathfinder_color::ColorU;
use warpui::r#async::Timer;
use warpui::{AppContext, Entity, EntityId, ModelContext, SingletonEntity};

use crate::ai::agent::conversation::{ConversationStatus, StatusColorStyle};
use crate::appearance::Appearance;
use crate::pane_group::PaneGroup;
use crate::terminal::cli_agent_sessions::CLIAgentSessionStatus;
use crate::util::color::coloru_with_opacity;
use crate::workspace::tab_settings::TabSettings;

/// How long the needs-you glow holds each brightness.
const BLINK_PERIOD: Duration = Duration::from_millis(650);

/// What a terminal's Claude session shows on its tab, from least to most urgent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TabGlow {
    /// Running a turn.
    Working,
    /// Finished a turn you haven't looked at.
    Done,
    /// Failed a turn you haven't looked at.
    Failed,
    /// Waiting on you, such as for a permission.
    NeedsYou,
}

impl TabGlow {
    /// How scripts see this glow, e.g. in `tab.list`.
    pub fn name(self) -> &'static str {
        match self {
            TabGlow::Working => "working",
            TabGlow::Done => "done",
            TabGlow::Failed => "failed",
            TabGlow::NeedsYou => "needs_you",
        }
    }

    fn for_status(status: &CLIAgentSessionStatus) -> Option<TabGlow> {
        match status {
            CLIAgentSessionStatus::InProgress => Some(TabGlow::Working),
            CLIAgentSessionStatus::Success => Some(TabGlow::Done),
            CLIAgentSessionStatus::Failed { .. } => Some(TabGlow::Failed),
            CLIAgentSessionStatus::Blocked { .. } => Some(TabGlow::NeedsYou),
            CLIAgentSessionStatus::Cancelled => None,
        }
    }

    /// The status color the rest of the app shows for this state, so the glow matches the
    /// status icons.
    fn color(self, appearance: &Appearance) -> ColorU {
        let status = match self {
            TabGlow::Working => ConversationStatus::InProgress,
            TabGlow::Done => ConversationStatus::Success,
            TabGlow::Failed => ConversationStatus::Error,
            TabGlow::NeedsYou => ConversationStatus::Blocked {
                blocked_action: String::new(),
            },
        };
        status
            .status_icon_and_color(appearance.theme(), StatusColorStyle::Standard)
            .1
    }
}

/// How a glowing tab row or group header is painted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlowPaint {
    pub border: ColorU,
    pub border_width: f32,
    /// A tint behind the row, for the states that should catch the eye from across the screen.
    pub background: Option<ColorU>,
}

pub enum AgentGlowEvent {
    Changed,
}

/// Glows by terminal view, and the needs-you blink.
pub struct AgentGlowModel {
    glows: HashMap<EntityId, TabGlow>,
    blink_on: bool,
    blinking: bool,
}

impl Default for AgentGlowModel {
    fn default() -> Self {
        Self {
            glows: HashMap::new(),
            blink_on: true,
            blinking: false,
        }
    }
}

impl Entity for AgentGlowModel {
    type Event = AgentGlowEvent;
}

impl SingletonEntity for AgentGlowModel {}

impl AgentGlowModel {
    /// Records a terminal's new session status. A turn that ends in a tab already in view
    /// counts as seen, so it doesn't glow.
    pub fn note_status(
        &mut self,
        terminal_view_id: EntityId,
        status: &CLIAgentSessionStatus,
        in_view: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        let glow = TabGlow::for_status(status)
            .filter(|glow| !(in_view && matches!(glow, TabGlow::Done | TabGlow::Failed)));
        let changed = match glow {
            Some(glow) => self.glows.insert(terminal_view_id, glow) != Some(glow),
            None => self.glows.remove(&terminal_view_id).is_some(),
        };
        if changed {
            self.start_blinking_if_needed(ctx);
            ctx.emit(AgentGlowEvent::Changed);
        }
    }

    /// Drops a terminal whose session ended.
    pub fn forget(&mut self, terminal_view_id: EntityId, ctx: &mut ModelContext<Self>) {
        if self.glows.remove(&terminal_view_id).is_some() {
            ctx.emit(AgentGlowEvent::Changed);
        }
    }

    /// Clears the finished-turn glows of terminals now in view; a session still working or
    /// waiting on you keeps glowing.
    pub fn mark_seen(
        &mut self,
        terminal_view_ids: impl IntoIterator<Item = EntityId>,
        ctx: &mut ModelContext<Self>,
    ) {
        let mut changed = false;
        for terminal_view_id in terminal_view_ids {
            if matches!(
                self.glows.get(&terminal_view_id),
                Some(TabGlow::Done | TabGlow::Failed)
            ) {
                self.glows.remove(&terminal_view_id);
                changed = true;
            }
        }
        if changed {
            ctx.emit(AgentGlowEvent::Changed);
        }
    }

    pub fn glow(&self, terminal_view_id: EntityId) -> Option<TabGlow> {
        self.glows.get(&terminal_view_id).copied()
    }

    /// The most urgent glow among a pane group's terminals.
    pub fn glow_for_pane_group(&self, pane_group: &PaneGroup, app: &AppContext) -> Option<TabGlow> {
        pane_group
            .terminal_views(app)
            .iter()
            .filter_map(|terminal_view| self.glow(terminal_view.id()))
            .max()
    }

    /// How to paint `glow` right now, or `None` while glowing is turned off.
    pub fn paint(&self, glow: TabGlow, app: &AppContext) -> Option<GlowPaint> {
        if !*TabSettings::as_ref(app).agent_status_glow {
            return None;
        }
        let color = glow.color(Appearance::as_ref(app));
        Some(match glow {
            TabGlow::Working => GlowPaint {
                border: coloru_with_opacity(color, 45),
                border_width: 1.,
                background: None,
            },
            TabGlow::Done => GlowPaint {
                border: coloru_with_opacity(color, 85),
                border_width: 1.5,
                background: None,
            },
            TabGlow::Failed => GlowPaint {
                border: coloru_with_opacity(color, 90),
                border_width: 1.5,
                background: Some(coloru_with_opacity(color, 8)),
            },
            TabGlow::NeedsYou if self.blink_on => GlowPaint {
                border: color,
                border_width: 2.,
                background: Some(coloru_with_opacity(color, 18)),
            },
            TabGlow::NeedsYou => GlowPaint {
                border: coloru_with_opacity(color, 35),
                border_width: 2.,
                background: Some(coloru_with_opacity(color, 6)),
            },
        })
    }

    fn start_blinking_if_needed(&mut self, ctx: &mut ModelContext<Self>) {
        if !self.blinking && self.anyone_needs_you() {
            self.blinking = true;
            self.schedule_blink(ctx);
        }
    }

    fn schedule_blink(&mut self, ctx: &mut ModelContext<Self>) {
        let _ = ctx.spawn(Timer::after(BLINK_PERIOD), |me, _, ctx| {
            if me.anyone_needs_you() {
                me.blink_on = !me.blink_on;
                ctx.emit(AgentGlowEvent::Changed);
                me.schedule_blink(ctx);
            } else {
                me.blinking = false;
                me.blink_on = true;
            }
        });
    }

    fn anyone_needs_you(&self) -> bool {
        self.glows.values().any(|glow| *glow == TabGlow::NeedsYou)
    }
}

#[cfg(test)]
#[path = "agent_glow_tests.rs"]
mod tests;
