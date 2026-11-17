//! Shellomancy: settings sync through Inventorium (madnessinteractive.cc).
//!
//! Off by default. With "Sync settings" on and a Madness account signed in,
//! the theme and the voice are kept in Inventorium's `shellomancy` settings
//! category, so every machine signed in to the account ends up with the same
//! ones (newest change wins, per value). Separately, "Follow Inventorium's
//! colour scheme / voice" makes Shellomancy take whatever the web app is set
//! to. Shellomancy never writes the web app's own settings.
//!
//! Remote values are applied with `set_value_from_cloud_sync`, and only
//! `LocalChange` events are stamped and sent, so applying a value never sends
//! it straight back. Warp's own cloud preference sync stays hidden.

pub mod api;
pub mod model;

use std::sync::Arc;
use std::time::Duration;

use settings::{ChangeEventReason, Setting as _};
use warpui::r#async::Timer;
use warpui::{AppContext, Entity, ModelContext, SingletonEntity};

use self::api::{HttpTransport, Pulled, Pushed, SyncError, Transport};
use self::model::{Key, LocalState, Stamped, SyncDoc, clamp_minutes, merge};
use crate::madness_account::{MadnessAccount, MadnessAccountState};
use crate::settings::madness_sync::{MadnessSyncSettings, MadnessSyncSettingsChangedEvent};
use crate::settings::madness_voice::{MadnessVoiceSettings, MadnessVoiceSettingsChangedEvent};
use crate::settings::{ThemeSettings, ThemeSettingsChangedEvent};
use crate::themes::theme::ThemeKind;

/// How long after a local change the change is sent.
const PUSH_DELAY: Duration = Duration::from_secs(2);
/// How often to look for changes made elsewhere while sync is on.
/// Retry delays after a network error; the last repeats.
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(30),
    Duration::from_secs(2 * 60),
    Duration::from_secs(10 * 60),
];

/// Where sync is at, for the Account page.
#[derive(Clone, Debug, PartialEq)]
pub enum SyncStatus {
    /// Sync and following are off, or no account is signed in.
    Idle,
    Syncing,
    /// Last synced at this Unix-seconds time.
    Synced(i64),
    /// A change is waiting to be sent.
    Pending,
    /// Couldn't reach the server; will retry.
    Offline,
    /// The server refused; message ready to show. Not retried.
    Refused(String),
    Failed(String),
}

#[derive(Clone, Copy, Debug)]
pub enum SettingsSyncEvent {
    StatusChanged,
}

pub struct SettingsSync {
    state: LocalState,
    status: SyncStatus,
    transport: Arc<dyn Transport>,
    /// Bumped when the account changes so replies to the old key are dropped.
    generation: u64,
    /// Bumped to cancel a scheduled push.
    push_generation: u64,
    /// Bumped to cancel the scheduled check, e.g. when its interval changes.
    poll_generation: u64,
    retry_level: usize,
    /// A refused push merges and sends again once; never in a loop.
    resent_after_refusal: bool,
}

impl Entity for SettingsSync {
    type Event = SettingsSyncEvent;
}

impl SingletonEntity for SettingsSync {}

impl SettingsSync {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        Self::with_transport(Arc::new(HttpTransport), ctx)
    }

    pub fn with_transport(transport: Arc<dyn Transport>, ctx: &mut ModelContext<Self>) -> Self {
        let state = if ctx.has_singleton_model::<MadnessSyncSettings>() {
            LocalState::from_json(MadnessSyncSettings::as_ref(ctx).state.value())
        } else {
            LocalState::default()
        };
        if ctx.has_singleton_model::<MadnessAccount>() {
            ctx.subscribe_to_model(&MadnessAccount::handle(ctx), |me, _, _, ctx| {
                me.account_changed(ctx);
            });
        }
        if ctx.has_singleton_model::<ThemeSettings>() {
            ctx.subscribe_to_model(&ThemeSettings::handle(ctx), |me, _, event, ctx| {
                if let ThemeSettingsChangedEvent::Theme {
                    change_event_reason: ChangeEventReason::LocalChange,
                } = event
                {
                    me.theme_changed_locally(ctx);
                }
            });
        }
        if ctx.has_singleton_model::<MadnessVoiceSettings>() {
            ctx.subscribe_to_model(&MadnessVoiceSettings::handle(ctx), |me, _, event, ctx| {
                if let MadnessVoiceSettingsChangedEvent::MadnessVoice {
                    change_event_reason: ChangeEventReason::LocalChange,
                } = event
                {
                    me.voice_changed_locally(ctx);
                }
            });
        }
        if ctx.has_singleton_model::<MadnessSyncSettings>() {
            ctx.subscribe_to_model(&MadnessSyncSettings::handle(ctx), |me, _, event, ctx| {
                if matches!(
                    event,
                    MadnessSyncSettingsChangedEvent::MadnessSettingsSyncEnabled { .. }
                        | MadnessSyncSettingsChangedEvent::FollowInventoriumPalette { .. }
                        | MadnessSyncSettingsChangedEvent::FollowInventoriumVoice { .. }
                ) {
                    me.sync_now(ctx);
                }
                if matches!(
                    event,
                    MadnessSyncSettingsChangedEvent::MadnessSyncIntervalMinutes { .. }
                ) {
                    me.restart_polling(ctx);
                }
            });
        }
        let me = Self {
            state,
            status: SyncStatus::Idle,
            transport,
            generation: 0,
            push_generation: 0,
            poll_generation: 0,
            retry_level: 0,
            resent_after_refusal: false,
        };
        // Settle in first; a pull at startup catches changes made elsewhere.
        let _ = ctx.spawn(Timer::after(PUSH_DELAY), |me, _, ctx| {
            me.sync_now(ctx);
            me.schedule_poll(me.generation, ctx);
        });
        me
    }

    pub fn status(&self) -> &SyncStatus {
        &self.status
    }

    /// Pulls (and pushes, if needed) right away.
    pub fn sync_now(&mut self, ctx: &mut ModelContext<Self>) {
        if !self.wants_pull(ctx) {
            self.set_status(SyncStatus::Idle, ctx);
            return;
        }
        let Some(key) = api_key(ctx) else {
            self.set_status(SyncStatus::Idle, ctx);
            return;
        };
        self.set_status(SyncStatus::Syncing, ctx);
        let generation = self.generation;
        let _ = ctx.spawn(self.transport.pull(key), move |me, result, ctx| {
            if me.generation != generation {
                return;
            }
            match result {
                Ok(pulled) => me.pulled(pulled, ctx),
                Err(error) => me.failed(error, ctx),
            }
        });
    }

    fn pulled(&mut self, pulled: Pulled, ctx: &mut ModelContext<Self>) {
        self.retry_level = 0;
        if sync_enabled(ctx) {
            self.seed_missing(ctx);
            let merged = merge(&self.state.doc, pulled.doc.as_ref());
            self.apply(&merged.doc, &merged.apply, ctx);
            self.state.doc = merged.doc;
            if let Some(remote) = &pulled.doc {
                self.state.last_acked = self.state.last_acked.max(remote.stamp());
            }
            self.save(ctx);
            if merged.push || self.state.has_unsent() {
                self.push(ctx);
                return;
            }
        }
        self.follow(&pulled.appearance, ctx);
        self.set_status(SyncStatus::Synced(chrono::Utc::now().timestamp()), ctx);
    }

    /// Applies the remote values that won, where this build can use them.
    fn apply(&mut self, doc: &SyncDoc, keys: &[Key], ctx: &mut ModelContext<Self>) {
        for key in keys {
            match key {
                Key::Theme => {
                    let Some(theme) = &doc.theme else { continue };
                    if follow_palette(ctx) {
                        // Following the web app's palette wins over sync.
                        continue;
                    }
                    if let Some(kind) = model::theme_from_value(&theme.value, &palette_ids()) {
                        set_theme(kind, ctx);
                    }
                }
                Key::Voice => {
                    let Some(voice) = &doc.voice else { continue };
                    if follow_voice(ctx) {
                        continue;
                    }
                    if let Some(id) = model::voice_from_value(&voice.value, &voice_ids()) {
                        set_voice(id, ctx);
                    }
                }
            }
        }
    }

    /// "Follow Inventorium": take the web app's palette and voice, compared by
    /// value (the web also writes them without updating stamps).
    fn follow(&mut self, appearance: &serde_json::Value, ctx: &mut ModelContext<Self>) {
        let (palette, voice) = model::follow_targets(appearance, &palette_ids(), &voice_ids());
        if follow_palette(ctx)
            && let Some(kind) = palette
            && ThemeSettings::as_ref(ctx).theme_kind.value() != &kind
        {
            set_theme(kind, ctx);
        }
        if follow_voice(ctx)
            && let Some(id) = voice
            && MadnessVoiceSettings::as_ref(ctx).voice.value() != &id
        {
            set_voice(id, ctx);
        }
    }

    fn push(&mut self, ctx: &mut ModelContext<Self>) {
        let Some(key) = api_key(ctx) else {
            return;
        };
        if !sync_enabled(ctx) || self.state.doc.is_empty() {
            return;
        }
        self.set_status(SyncStatus::Syncing, ctx);
        let generation = self.generation;
        let doc = self.state.doc.clone();
        let _ = ctx.spawn(self.transport.push(key, doc), move |me, result, ctx| {
            if me.generation != generation {
                return;
            }
            match result {
                Ok(Pushed::Applied(stamp)) => {
                    me.retry_level = 0;
                    me.resent_after_refusal = false;
                    me.state.last_acked = stamp;
                    me.save(ctx);
                    me.set_status(SyncStatus::Synced(chrono::Utc::now().timestamp()), ctx);
                }
                Ok(Pushed::Refused(stored)) => {
                    // Something newer is stored: take what wins, then send
                    // the merge once.
                    let merged = merge(&me.state.doc, Some(&stored));
                    me.apply(&merged.doc, &merged.apply, ctx);
                    me.state.doc = merged.doc;
                    me.state.last_acked = me.state.last_acked.max(stored.stamp());
                    me.save(ctx);
                    if merged.push && !me.resent_after_refusal {
                        me.resent_after_refusal = true;
                        me.push(ctx);
                    } else {
                        me.resent_after_refusal = false;
                        me.set_status(SyncStatus::Synced(chrono::Utc::now().timestamp()), ctx);
                    }
                }
                Err(error) => me.failed(error, ctx),
            }
        });
    }

    fn failed(&mut self, error: SyncError, ctx: &mut ModelContext<Self>) {
        match error {
            SyncError::Refused(message) => {
                self.set_status(SyncStatus::Refused(message), ctx);
            }
            SyncError::Network(_) => {
                self.set_status(SyncStatus::Offline, ctx);
                let delay = RETRY_DELAYS[self.retry_level.min(RETRY_DELAYS.len() - 1)];
                self.retry_level += 1;
                let generation = self.generation;
                let _ = ctx.spawn(Timer::after(delay), move |me, _, ctx| {
                    if me.generation == generation && me.status == SyncStatus::Offline {
                        me.sync_now(ctx);
                    }
                });
            }
            SyncError::Other(message) => self.set_status(SyncStatus::Failed(message), ctx),
        }
    }

    /// Values never changed here have no stamp; give them the oldest one, so
    /// the first sync sends them when nothing is stored yet but any value
    /// stored from another machine wins.
    fn seed_missing(&mut self, ctx: &mut ModelContext<Self>) {
        if self.state.doc.theme.is_none()
            && let Some(value) = model::theme_value(ThemeSettings::as_ref(ctx).theme_kind.value())
        {
            self.state.doc.theme = Some(Stamped { value, at: 1 });
        }
        if self.state.doc.voice.is_none() {
            let value = MadnessVoiceSettings::as_ref(ctx).voice.value().clone();
            self.state.doc.voice = Some(Stamped { value, at: 1 });
        }
    }

    fn theme_changed_locally(&mut self, ctx: &mut ModelContext<Self>) {
        // Picking a theme by hand stops following the web app's palette.
        if follow_palette(ctx) {
            MadnessSyncSettings::handle(ctx).update(ctx, |settings, ctx| {
                let _ = settings.follow_palette.set_value(false, ctx);
            });
        }
        let kind = ThemeSettings::as_ref(ctx).theme_kind.value().clone();
        let Some(value) = model::theme_value(&kind) else {
            return;
        };
        self.state.doc.theme = Some(Stamped {
            value,
            at: now_ms(),
        });
        self.local_change(ctx);
    }

    fn voice_changed_locally(&mut self, ctx: &mut ModelContext<Self>) {
        if follow_voice(ctx) {
            MadnessSyncSettings::handle(ctx).update(ctx, |settings, ctx| {
                let _ = settings.follow_voice.set_value(false, ctx);
            });
        }
        let voice = MadnessVoiceSettings::as_ref(ctx).voice.value().clone();
        self.state.doc.voice = Some(Stamped {
            value: voice,
            at: now_ms(),
        });
        self.local_change(ctx);
    }

    /// Stamps are kept even with sync off, so turning it on later sends the
    /// newest local values rather than older remote ones.
    fn local_change(&mut self, ctx: &mut ModelContext<Self>) {
        self.save(ctx);
        if !sync_enabled(ctx) || api_key(ctx).is_none() {
            return;
        }
        self.set_status(SyncStatus::Pending, ctx);
        self.push_generation += 1;
        let push_generation = self.push_generation;
        let _ = ctx.spawn(Timer::after(PUSH_DELAY), move |me, _, ctx| {
            if me.push_generation == push_generation {
                me.push(ctx);
            }
        });
    }

    fn account_changed(&mut self, ctx: &mut ModelContext<Self>) {
        self.generation += 1;
        self.retry_level = 0;
        // Signed out keeps the state, so signing back in to the same
        // account carries on where it left off.
        if let Some(account) = account_display(ctx)
            && account != self.state.account
        {
            // Another account's stored copy says nothing about this one.
            self.state.account = account;
            self.state.last_acked = 0;
            self.save(ctx);
        }
        self.sync_now(ctx);
        self.schedule_poll(self.generation, ctx);
    }

    /// Schedules the next check, cancelling any already scheduled.
    fn schedule_poll(&mut self, generation: u64, ctx: &mut ModelContext<Self>) {
        self.poll_generation += 1;
        let poll_generation = self.poll_generation;
        let _ = ctx.spawn(Timer::after(pull_interval(ctx)), move |me, _, ctx| {
            if me.generation != generation || me.poll_generation != poll_generation {
                return;
            }
            if !matches!(me.status, SyncStatus::Refused(_)) {
                me.sync_now(ctx);
            }
            me.schedule_poll(generation, ctx);
        });
    }

    /// Checks again now and restarts the poll timer, after the interval
    /// setting changed.
    pub fn restart_polling(&mut self, ctx: &mut ModelContext<Self>) {
        self.schedule_poll(self.generation, ctx);
    }

    fn wants_pull(&self, ctx: &AppContext) -> bool {
        sync_enabled(ctx) || follow_palette(ctx) || follow_voice(ctx)
    }

    fn save(&mut self, ctx: &mut ModelContext<Self>) {
        if !ctx.has_singleton_model::<MadnessSyncSettings>() {
            return;
        }
        let raw = self.state.to_json();
        MadnessSyncSettings::handle(ctx).update(ctx, |settings, ctx| {
            if settings.state.value() != &raw {
                let _ = settings.state.set_value(raw, ctx);
            }
        });
    }

    fn set_status(&mut self, status: SyncStatus, ctx: &mut ModelContext<Self>) {
        if self.status != status {
            self.status = status;
            ctx.emit(SettingsSyncEvent::StatusChanged);
            ctx.notify();
        }
    }
}

fn sync_enabled(ctx: &AppContext) -> bool {
    ctx.has_singleton_model::<MadnessSyncSettings>()
        && *MadnessSyncSettings::as_ref(ctx).enabled.value()
}

fn follow_palette(ctx: &AppContext) -> bool {
    ctx.has_singleton_model::<MadnessSyncSettings>()
        && *MadnessSyncSettings::as_ref(ctx).follow_palette.value()
}

fn follow_voice(ctx: &AppContext) -> bool {
    ctx.has_singleton_model::<MadnessSyncSettings>()
        && *MadnessSyncSettings::as_ref(ctx).follow_voice.value()
}

/// The signed-in Madness account's key, if there is one.
fn api_key(ctx: &AppContext) -> Option<String> {
    if !ctx.has_singleton_model::<MadnessAccount>() {
        return None;
    }
    let account = MadnessAccount::as_ref(ctx);
    matches!(account.state(), MadnessAccountState::SignedIn { .. })
        .then(|| account.api_key().map(str::to_owned))
        .flatten()
}

fn account_display(ctx: &AppContext) -> Option<String> {
    if !ctx.has_singleton_model::<MadnessAccount>() {
        return None;
    }
    match MadnessAccount::as_ref(ctx).state() {
        MadnessAccountState::SignedIn { display } => Some(display.clone()),
        _ => None,
    }
}

/// Sets the theme as a synced value (not a local change, so it isn't sent
/// back), and turns off following the OS light/dark theme, which would hide it.
fn set_theme(kind: ThemeKind, ctx: &mut ModelContext<SettingsSync>) {
    ThemeSettings::handle(ctx).update(ctx, |settings, ctx| {
        let _ = settings
            .use_system_theme
            .set_value_from_cloud_sync(false, ctx);
        let _ = settings.theme_kind.set_value_from_cloud_sync(kind, ctx);
    });
}

/// Sets the voice as a synced value; it takes effect after a restart.
fn set_voice(id: String, ctx: &mut ModelContext<SettingsSync>) {
    MadnessVoiceSettings::handle(ctx).update(ctx, |settings, ctx| {
        let _ = settings.voice.set_value_from_cloud_sync(id, ctx);
    });
}

fn palette_ids() -> Vec<&'static str> {
    madness_themes::palettes()
        .iter()
        .map(|palette| palette.id.as_str())
        .collect()
}

fn voice_ids() -> Vec<&'static str> {
    madness_themes::voices()
        .iter()
        .map(|voice| voice.id.as_str())
        .collect()
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Whether the voice setting differs from the voice in use (it takes effect
/// at the next start).
pub fn voice_pending_restart(ctx: &AppContext) -> Option<String> {
    if !ctx.has_singleton_model::<MadnessVoiceSettings>() {
        return None;
    }
    let wanted = MadnessVoiceSettings::as_ref(ctx).voice.value();
    (wanted != &madness_themes::active_voice().id).then(|| {
        madness_themes::voices()
            .iter()
            .find(|voice| &voice.id == wanted)
            .map_or_else(|| wanted.clone(), |voice| voice.name.clone())
    })
}

/// How long to wait between checks: the interval setting, kept to 1 minute
/// to a day.
fn pull_interval(ctx: &AppContext) -> Duration {
    let minutes = if ctx.has_singleton_model::<MadnessSyncSettings>() {
        *MadnessSyncSettings::as_ref(ctx)
            .pull_interval_minutes
            .value()
    } else {
        10
    };
    Duration::from_secs(u64::from(clamp_minutes(minutes)) * 60)
}
