use url::Url;

use super::*;
use crate::features::FeatureFlag;

fn assert_loopback(raw: &str) {
    let url = Url::parse(raw).unwrap_or_else(|err| panic!("{raw} should parse: {err}"));
    assert_eq!(url.host_str(), Some("127.0.0.1"), "{raw} must stay on loopback");
}

#[test]
fn offline_server_config_never_points_at_warp() {
    let server = WarpServerConfig::offline();
    assert_loopback(&server.server_root_url);
    assert_loopback(&server.rtc_server_url);
    assert_eq!(server.session_sharing_server_url, None);
    assert_eq!(server.prewarmed_sandbox_control_url, None);
    assert!(server.firebase_auth_api_key.is_empty());

    let oz = OzConfig::offline();
    assert_loopback(&oz.oz_root_url);
    assert_eq!(oz.workload_audience_url, None);

    let all = format!(
        "{} {} {}",
        server.server_root_url, server.rtc_server_url, oz.oz_root_url
    );
    assert!(!all.contains("warp.dev"), "offline config leaked a Warp host: {all}");
}

#[test]
fn offline_disabled_features_cover_every_logged_out_server_call() {
    for flag in [
        FeatureFlag::Autoupdate,
        FeatureFlag::Changelog,
        FeatureFlag::AgentOnboarding,
        FeatureFlag::SshRemoteServer,
        FeatureFlag::CrashReporting,
    ] {
        assert!(
            OFFLINE_DISABLED_FEATURES.contains(&flag),
            "{flag:?} reaches Warp's servers while logged out and must stay disabled"
        );
    }
}

#[test]
fn offline_disabled_features_drop_oz_cloud_but_keep_third_party_cli_agents() {
    for flag in [
        FeatureFlag::CloudMode,
        FeatureFlag::AmbientAgentsRTC,
        FeatureFlag::ScheduledAmbientAgents,
        FeatureFlag::OzPlatformSkills,
        FeatureFlag::OzHandoff,
        FeatureFlag::AgentManagementView,
        FeatureFlag::AgentMode,
        FeatureFlag::AgentView,
        FeatureFlag::AIContextMenuEnabled,
    ] {
        assert!(OFFLINE_DISABLED_FEATURES.contains(&flag), "{flag:?} should be off");
    }
    // Claude Code / Codex running inside the terminal must keep working.
    for flag in [
        FeatureFlag::CLIAgentRichInput,
        FeatureFlag::CtrlCCancelsThirdPartyHarness,
    ] {
        assert!(!OFFLINE_DISABLED_FEATURES.contains(&flag), "{flag:?} must stay on");
    }
}
