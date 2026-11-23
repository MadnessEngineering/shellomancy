//! Shellomancy: settings sync through Inventorium (madnessinteractive.cc).
//!
//! Everything here is off by default. See `crate::madness_sync`.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::macros::define_settings_group;
use settings::{SupportedPlatforms, SyncToCloud};

/// Where a todo goes when you open it from the Mana Bar. The dashboard's
/// `ui.todoEditBehavior` picks between editing in place and the todo's own
/// page; a terminal can also hand the todo to an agent.
#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    Default,
    JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "Where a todo opens from the Mana Bar.",
    rename_all = "snake_case"
)]
pub enum TodoOpenTarget {
    /// A quest's todo unrolls under its row with all its fields; cards
    /// already unroll, so their open button goes to the site.
    InPlace,
    /// The todo's page on madnessinteractive.cc.
    #[default]
    Browser,
    /// A new tab running `claude` on the todo.
    AgentTab,
}

impl TodoOpenTarget {
    pub const ALL: [TodoOpenTarget; 3] = [
        TodoOpenTarget::InPlace,
        TodoOpenTarget::Browser,
        TodoOpenTarget::AgentTab,
    ];

    pub fn voice_key(self) -> &'static str {
        match self {
            TodoOpenTarget::InPlace => "shellomancy.settings.manaBar.openInPlace",
            TodoOpenTarget::Browser => "shellomancy.settings.manaBar.openBrowser",
            TodoOpenTarget::AgentTab => "shellomancy.settings.manaBar.openAgentTab",
        }
    }
}

/// The order of the Mana Bar's todos: the dashboard's sort modes, in the
/// order its sort button cycles through them.
#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    Default,
    JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "The order of the Mana Bar's todos.",
    rename_all = "snake_case"
)]
pub enum TodoSort {
    #[default]
    Newest,
    Oldest,
    /// Critical first; within a priority, in progress before pending,
    /// blocked and review; then newest first.
    Priority,
    /// Shuffled, once each time it's picked.
    Madness,
}

impl TodoSort {
    pub const ALL: [TodoSort; 4] = [
        TodoSort::Newest,
        TodoSort::Oldest,
        TodoSort::Priority,
        TodoSort::Madness,
    ];

    /// The mode the sort button moves on to.
    pub fn next(self) -> Self {
        match self {
            TodoSort::Newest => TodoSort::Oldest,
            TodoSort::Oldest => TodoSort::Priority,
            TodoSort::Priority => TodoSort::Madness,
            TodoSort::Madness => TodoSort::Newest,
        }
    }

    pub fn voice_key(self) -> &'static str {
        match self {
            TodoSort::Newest => "shellomancy.manaBar.sortNewest",
            TodoSort::Oldest => "shellomancy.manaBar.sortOldest",
            TodoSort::Priority => "shellomancy.manaBar.sortPriority",
            TodoSort::Madness => "shellomancy.manaBar.sortMadness",
        }
    }
}

define_settings_group!(MadnessSyncSettings, settings: [
    enabled: MadnessSettingsSyncEnabled {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "MadnessSettingsSyncEnabled",
        toml_path: "madness.settings_sync",
        description: "Whether your theme and voice sync through your Madness Interactive account.",
    },
    follow_palette: FollowInventoriumPalette {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "FollowInventoriumPalette",
        toml_path: "appearance.themes.follow_inventorium",
        description: "Whether the theme follows the colour scheme picked on madnessinteractive.cc.",
    },
    follow_voice: FollowInventoriumVoice {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "FollowInventoriumVoice",
        toml_path: "appearance.follow_inventorium_voice",
        description: "Whether the voice follows the one picked on madnessinteractive.cc (applies after a restart).",
    },
    pull_interval_minutes: MadnessSyncIntervalMinutes {
        type: u32,
        default: 10,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "MadnessSyncIntervalMinutes",
        toml_path: "madness.sync_interval_minutes",
        description: "How often, in minutes, settings sync checks madnessinteractive.cc for changes (1 to 1440).",
    },
    todo_open_target: TodoOpenTargetSetting {
        type: TodoOpenTarget,
        default: TodoOpenTarget::Browser,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "MadnessTodoOpenTarget",
        toml_path: "madness.todo_open_target",
        description: "Where a todo opens from the Mana Bar: in_place, browser or agent_tab.",
    },
    todo_sort: MadnessTodoSort {
        type: TodoSort,
        default: TodoSort::Newest,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "MadnessTodoSort",
        toml_path: "madness.todo_sort",
        description: "The order of the Mana Bar's todos: newest, oldest, priority or madness (shuffled).",
    },
    log_commits: MadnessLogCommits {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "MadnessLogCommits",
        toml_path: "madness.log_commits",
        description: "Whether the Mana Bar's Log mixes in the focused repo's git commits.",
    },
    check_for_updates_automatically: MadnessCheckForUpdatesAutomatically {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "MadnessCheckForUpdatesAutomatically",
        toml_path: "madness.check_for_updates_automatically",
        description: "Whether Shellomancy checks GitHub for a new release on its own (Check for updates always works).",
    },
    // What was last synced and when each value changed, as JSON. Not a
    // user-visible setting.
    state: MadnessSyncState {
        type: String,
        default: String::new(),
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
]);
