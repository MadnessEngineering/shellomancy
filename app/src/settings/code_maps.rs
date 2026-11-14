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
]);
