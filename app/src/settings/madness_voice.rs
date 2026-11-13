//! Shellomancy: which MadnessThemes voice the UI text is written in.
//!
//! The voice is applied once at startup (`lib.rs`, before any view builds its
//! strings), so a change takes effect after a restart.
use settings::macros::define_settings_group;
use settings::{SupportedPlatforms, SyncToCloud};

define_settings_group!(MadnessVoiceSettings, settings: [
    voice: MadnessVoice {
        type: String,
        default: madness_themes::DEFAULT_VOICE.to_owned(),
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "MadnessVoice",
        toml_path: "appearance.voice",
        description: "The MadnessThemes voice Shellomancy's text is written in (shellomancer, standard, mad-wizard, debug...). Applies after a restart.",
    },
]);
