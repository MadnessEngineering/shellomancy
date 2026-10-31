use enum_iterator::Sequence;
use serde::{Deserialize, Serialize};
use warp_core::settings::macros::define_settings_group;
use warp_core::settings::{SupportedPlatforms, SyncToCloud};

/// The app icon to use (mac-only).
///
/// IMPORTANT NOTE: If you add a new icon, you will need to update the logic in WarpDockTilePlugin.m
/// to read the new icon and also add the icon to app/DockTilePlugin/Resources.
#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Serialize,
    Deserialize,
    Sequence,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "The app icon displayed in the dock.",
    rename_all = "snake_case"
)]
pub enum AppIcon {
    /// Shellomancy nautilus, as bundled in the app (respects macOS icon tinting).
    #[default]
    #[schemars(description = "Default")]
    Default,
    /// Deep-sea blue nautilus.
    #[schemars(description = "Abyss")]
    Abyss,
    /// Ink nautilus on parchment.
    #[schemars(description = "Bone")]
    Bone,
    /// Forge-orange nautilus.
    #[schemars(description = "Ember")]
    Ember,
    /// White nautilus on black.
    #[schemars(description = "Mono")]
    Mono,
    /// Acid-green nautilus.
    #[schemars(description = "Toxic")]
    Toxic,
}

impl std::fmt::Display for AppIcon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match &self {
            AppIcon::Default => "Default",
            AppIcon::Abyss => "Abyss",
            AppIcon::Bone => "Bone",
            AppIcon::Ember => "Ember",
            AppIcon::Mono => "Mono",
            AppIcon::Toxic => "Toxic",
        };
        write!(f, "{value}")
    }
}

impl AppIconSettings {
    /// Shellomancy: Warp's alternate icons were replaced with nautilus palettes.
    /// Keep in sync with WarpDockTilePlugin.m and app/DockTilePlugin/Resources.
    pub fn get_base_icon_file_name(icon: AppIcon) -> &'static str {
        match icon {
            AppIcon::Default => "shellomancy",
            AppIcon::Abyss => "abyss",
            AppIcon::Bone => "bone",
            AppIcon::Ember => "ember",
            AppIcon::Mono => "mono",
            AppIcon::Toxic => "toxic",
        }
    }
}

define_settings_group!(AppIconSettings, settings: [
    app_icon: AppIconState {
        type: AppIcon,
        default: AppIcon::Default,
        supported_platforms: SupportedPlatforms::MAC,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "AppIcon",
        toml_path: "appearance.icon.app_icon",
        description: "The app icon displayed in the dock.",
    },
    show_dock_icon: ShowDockIconState {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::MAC,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "ShowDockIcon",
        toml_path: "appearance.icon.show_dock_icon",
        description: "Whether Warp is shown in the macOS Dock and Cmd-Tab switcher.",
    },
]);
