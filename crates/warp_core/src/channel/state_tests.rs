use super::{derive_http_origin_from_ws_url, logfile_name_for_profile, plist_warp_version};

#[test]
fn wss_becomes_https_and_strips_path() {
    let got = derive_http_origin_from_ws_url("wss://rtc.app.warp.dev/graphql/v2");
    assert_eq!(got.as_deref(), Some("https://rtc.app.warp.dev"));
}

#[test]
fn ws_becomes_http_and_preserves_port() {
    let got = derive_http_origin_from_ws_url("ws://localhost:8080/graphql/v2");
    assert_eq!(got.as_deref(), Some("http://localhost:8080"));
}

#[test]
fn unparseable_input_returns_none() {
    assert!(derive_http_origin_from_ws_url("not a url").is_none());
    assert!(derive_http_origin_from_ws_url("https://app.warp.dev").is_none());
}

#[test]
fn the_bundle_version_is_read_from_an_xml_plist() {
    let plist = "<dict>\n\t<key>CFBundleName</key>\n\t<string>Shellomancy</string>\n\
                 \t<key>WarpVersion</key>\n\t<string>v0.2026.11.19.18.30.local_00</string>\n</dict>";
    assert_eq!(
        plist_warp_version(plist).as_deref(),
        Some("v0.2026.11.19.18.30.local_00")
    );
    assert_eq!(plist_warp_version("<dict></dict>"), None);
    assert_eq!(
        plist_warp_version("<key>WarpVersion</key><string>0.1.0</string>"),
        None
    );
    assert_eq!(
        plist_warp_version("<key>WarpVersion</key><integer>3</integer>"),
        None
    );
}

#[test]
fn a_data_profile_gets_its_own_log_file() {
    assert_eq!(
        logfile_name_for_profile("shellomancy.log", "dev"),
        "shellomancy-dev.log"
    );
    assert_eq!(
        logfile_name_for_profile("warp_local.log", "x"),
        "warp_local-x.log"
    );
    assert_eq!(
        logfile_name_for_profile("shellomancy", "dev"),
        "shellomancy-dev"
    );
}
