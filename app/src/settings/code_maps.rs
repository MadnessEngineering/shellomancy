//! Shellomancy: Code Maps (cartogomancy) settings.
use settings::macros::define_settings_group;
use settings::{SupportedPlatforms, SyncToCloud};

define_settings_group!(CodeMapsSettings, settings: [
    upload_enabled: CodeMapsUploadEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "CodeMapsUploadEnabled",
        toml_path: "code_maps.upload",
        description: "Whether new code maps are sent to Inventorium (madnessinteractive.cc) while a Madness account is signed in.",
    },
    auto_remap_enabled: CodeMapsAutoRemapEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "CodeMapsAutoRemapEnabled",
        toml_path: "code_maps.auto_remap",
        description: "Whether a mapped repo is mapped again on its own when its default branch moves or its tags change, at most once an hour.",
    },
]);
