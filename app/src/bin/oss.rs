// On Windows, we don't want to display a console window when the application is running in release
// builds. See https://doc.rust-lang.org/reference/runtime.html#the-windows_subsystem-attribute.
#![cfg_attr(feature = "release_bundle", windows_subsystem = "windows")]

use anyhow::Result;
use warp_core::AppId;
use warp_core::channel::{
    AutoupdateConfig, Channel, ChannelConfig, ChannelState, OzConfig, WarpServerConfig,
};
use warp_core::features::FeatureFlag;

/// Where Shellomancy's releases live: each tag's assets are
/// `channel_versions.json`, `Shellomancy-arm64.dmg` and its `.sha256`.
const RELEASES_URL: &str = "https://github.com/MadnessEngineering/shellomancy/releases";

// Simple wrapper around warp::run() for Shellomancy (the Warp OSS channel, rebranded).
fn main() -> Result<()> {
    let mut state = ChannelState::new(
        Channel::Oss,
        ChannelConfig {
            app_id: AppId::new("cc", "madnessinteractive", "shellomancy"),
            logfile_name: "shellomancy.log".into(),
            server_config: WarpServerConfig::offline(),
            oz_config: OzConfig::offline(),
            telemetry_config: None,
            crash_reporting_config: None,
            // The only network call Shellomancy makes on its own, and only
            // when asked (Check for updates) or allowed (Settings > Account).
            autoupdate_config: Some(AutoupdateConfig {
                releases_base_url: RELEASES_URL.into(),
                show_autoupdate_menu_items: true,
            }),
            mcp_static_config: None,
        },
    );
    if cfg!(debug_assertions) {
        state = state.with_additional_features(warp_core::features::DEBUG_FLAGS);
    }
    // Offline, except that updates come from our GitHub releases. (The TUI
    // keeps the whole list: its updater still talks to Warp's server.)
    let disabled: Vec<FeatureFlag> = warp_core::channel::OFFLINE_DISABLED_FEATURES
        .iter()
        .copied()
        .filter(|flag| *flag != FeatureFlag::Autoupdate)
        .collect();
    state = state.with_disabled_features(&disabled);
    ChannelState::set(state);

    warp::run()
}

// If we're not using an external plist, embed the following as the Info.plist.
#[cfg(all(not(feature = "extern_plist"), target_os = "macos"))]
embed_plist::embed_info_plist_bytes!(r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0">
    <dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>English</string>
    <key>CFBundleDisplayName</key>
    <string>Shellomancy</string>
    <key>CFBundleExecutable</key>
    <string>warp-oss</string>
    <key>CFBundleIdentifier</key>
    <string>cc.madnessinteractive.shellomancy</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Shellomancy</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.developer-tools</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>UIDesignRequiresCompatibility</key>
    <true/>
    <key>CFBundleURLTypes</key>
    <array><dict><key>CFBundleURLName</key><string>Custom App</string><key>CFBundleURLSchemes</key><array><string>shellomancy</string></array></dict></array>
    <key>NSHumanReadableCopyright</key>
    <string>© 2026, Denver Technologies, Inc</string>
    </dict>
    </plist>
"#.as_bytes());
