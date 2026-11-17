//! Shellomancy: settings sync through Inventorium (madnessinteractive.cc).
//!
//! Everything here is off by default. See `crate::madness_sync`.
use settings::macros::define_settings_group;
use settings::{SupportedPlatforms, SyncToCloud};

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
