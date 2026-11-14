use crate::channel::ChannelState;

// Shellomancy: our own docs and issues, not Warp's.
pub const USER_DOCS_URL: &str = "https://github.com/MadnessEngineering/shellomancy/tree/HEAD/docs";
pub const GITHUB_ISSUES_URL: &str = "https://github.com/MadnessEngineering/shellomancy/issues";
pub const SLACK_URL: &str = "http://go.warp.dev/join-preview";
pub const PRIVACY_POLICY_URL: &str = "https://www.warp.dev/privacy";

pub fn feedback_form_url() -> String {
    // Shellomancy: feedback goes to our issues, with the version filled in.
    if ChannelState::channel() == crate::channel::Channel::Oss {
        let mut url =
            url::Url::parse(&format!("{GITHUB_ISSUES_URL}/new")).expect("Should not fail to parse");
        let body = format!(
            "\n\n---\nShellomancy {}, {} {}",
            ChannelState::app_version().unwrap_or("(unknown version)"),
            os_info::get().os_type(),
            os_info::get().version(),
        );
        url.query_pairs_mut().append_pair("body", &body);
        return url.to_string();
    }
    let mut url = url::Url::parse("https://github.com/warpdotdev/Warp/issues/new/choose")
        .expect("Should not fail to parse");
    if let Some(version) = ChannelState::app_version() {
        url.query_pairs_mut().append_pair("warp-version", version);
    }
    url.query_pairs_mut()
        .append_pair("os-version", &os_info::get().version().to_string());
    url.to_string()
}
