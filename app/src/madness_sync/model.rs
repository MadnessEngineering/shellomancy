//! The synced document and how two copies of it merge. Pure, so it's tested
//! without an app or a network.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::themes::theme::ThemeKind;

/// Bumped if the document's shape changes incompatibly.
pub const DOC_VERSION: u64 = 1;

/// One synced value and when it was last changed (Unix milliseconds).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stamped<T> {
    pub value: T,
    pub at: i64,
}

/// What Shellomancy keeps in Inventorium's `shellomancy` settings category.
/// Each value carries its own stamp, so a theme changed offline on one
/// machine survives a voice changed later on another; Inventorium only
/// compares whole-category stamps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncDoc {
    #[serde(default)]
    pub v: u64,
    /// The theme as `ThemeKind` serializes it: `"Dark"`, `{"Madness":"labops"}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Stamped<Value>>,
    /// A MadnessThemes voice id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<Stamped<String>>,
}

impl SyncDoc {
    /// The category stamp: the newest value stamp.
    pub fn stamp(&self) -> i64 {
        let theme = self.theme.as_ref().map_or(0, |theme| theme.at);
        let voice = self.voice.as_ref().map_or(0, |voice| voice.at);
        theme.max(voice)
    }

    pub fn is_empty(&self) -> bool {
        self.theme.is_none() && self.voice.is_none()
    }

    /// Reads a stored or downloaded document; anything unreadable is empty.
    pub fn from_value(value: &Value) -> SyncDoc {
        serde_json::from_value(value.clone()).unwrap_or_default()
    }
}

/// Which synced value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Theme,
    Voice,
}

/// The result of merging the local and remote documents.
#[derive(Clone, Debug, PartialEq)]
pub struct Merge {
    pub doc: SyncDoc,
    /// Values the remote copy won; apply them locally (if this build can).
    pub apply: Vec<Key>,
    /// The local copy has something the remote doesn't; send the merged doc.
    pub push: bool,
}

/// Per value, the newer stamp wins; a tie goes to the remote copy (as on the
/// web). A remote value this build can't use is still kept in the merged doc,
/// so the machine that set it doesn't lose it.
pub fn merge(local: &SyncDoc, remote: Option<&SyncDoc>) -> Merge {
    let empty = SyncDoc::default();
    let remote = remote.unwrap_or(&empty);
    let mut apply = Vec::new();
    let mut push = false;
    let theme = pick(
        &local.theme,
        &remote.theme,
        Key::Theme,
        &mut apply,
        &mut push,
    );
    let voice = pick(
        &local.voice,
        &remote.voice,
        Key::Voice,
        &mut apply,
        &mut push,
    );
    Merge {
        doc: SyncDoc {
            v: DOC_VERSION,
            theme,
            voice,
        },
        apply,
        push,
    }
}

fn pick<T: Clone + PartialEq>(
    local: &Option<Stamped<T>>,
    remote: &Option<Stamped<T>>,
    key: Key,
    apply: &mut Vec<Key>,
    push: &mut bool,
) -> Option<Stamped<T>> {
    match (local, remote) {
        (None, None) => None,
        (Some(local), None) => {
            *push = true;
            Some(local.clone())
        }
        (None, Some(remote)) => {
            apply.push(key);
            Some(remote.clone())
        }
        (Some(local), Some(remote)) if local.at > remote.at => {
            *push = true;
            Some(local.clone())
        }
        (Some(local), Some(remote)) => {
            if local.value != remote.value {
                apply.push(key);
            }
            Some(remote.clone())
        }
    }
}

/// Whether a theme means the same thing on another machine. Custom themes
/// are local files, and in-memory themes don't outlive the app.
pub fn is_portable_theme(kind: &ThemeKind) -> bool {
    !matches!(
        kind,
        ThemeKind::Custom(_) | ThemeKind::CustomBase16(_) | ThemeKind::InMemory(_)
    )
}

/// A theme to store in the document, if it's portable.
pub fn theme_value(kind: &ThemeKind) -> Option<Value> {
    is_portable_theme(kind)
        .then(|| serde_json::to_value(kind).ok())
        .flatten()
}

/// The theme a synced value names, if this build has it: portable, and for a
/// MadnessThemes palette, compiled in (an unknown kind would silently fall
/// back to Dark).
pub fn theme_from_value(value: &Value, palette_ids: &[&str]) -> Option<ThemeKind> {
    let kind: ThemeKind = serde_json::from_value(value.clone()).ok()?;
    match &kind {
        ThemeKind::Madness(id) if !palette_ids.contains(&id.as_str()) => None,
        kind if !is_portable_theme(kind) => None,
        _ => Some(kind),
    }
}

/// A voice id, if this build has that voice.
pub fn voice_from_value(value: &str, voice_ids: &[&str]) -> Option<String> {
    voice_ids.contains(&value).then(|| value.to_owned())
}

/// What following madnessinteractive.cc's appearance would set: its colour
/// scheme (`appearance.chromamancyTheme`, a palette id) and its text voice
/// (`appearance.colorTheme`, despite the name). Missing or unknown values
/// give `None`, never a default.
pub fn follow_targets(
    appearance: &Value,
    palette_ids: &[&str],
    voice_ids: &[&str],
) -> (Option<ThemeKind>, Option<String>) {
    let palette = appearance["chromamancyTheme"]
        .as_str()
        .filter(|id| palette_ids.contains(id))
        .map(|id| ThemeKind::Madness(id.to_owned()));
    let voice = appearance["colorTheme"]
        .as_str()
        .and_then(|id| voice_from_value(id, voice_ids));
    (palette, voice)
}

/// What's kept locally between runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LocalState {
    /// This machine's copy of the document, with when each value changed.
    #[serde(default)]
    pub doc: SyncDoc,
    /// The category stamp Inventorium last accepted from us; anything newer
    /// in `doc` hasn't been sent yet.
    #[serde(default)]
    pub last_acked: i64,
    /// The account the state was synced with; a different account starts over.
    #[serde(default)]
    pub account: String,
}

impl LocalState {
    pub fn from_json(raw: &str) -> LocalState {
        serde_json::from_str(raw).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Whether there are local changes Inventorium hasn't accepted yet.
    pub fn has_unsent(&self) -> bool {
        self.doc.stamp() > self.last_acked
    }
}

/// The longest check interval allowed, a day.
pub const MAX_PULL_MINUTES: u32 = 24 * 60;

/// A check interval kept to 1 minute to a day, whatever settings.toml says.
pub fn clamp_minutes(minutes: u32) -> u32 {
    minutes.clamp(1, MAX_PULL_MINUTES)
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
