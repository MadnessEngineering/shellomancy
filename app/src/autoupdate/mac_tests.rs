use super::*;

const LOCAL_KEY: &str =
    "identifier \"cc.madnessinteractive.shellomancy\" and certificate leaf = H\"ac9c93aa\"";

#[test]
fn the_designated_requirement_is_read_from_codesign_output() {
    let printed = format!(
        "Executable=/Applications/Shellomancy.app/Contents/MacOS/warp-oss\ndesignated => {LOCAL_KEY}\n"
    );
    assert_eq!(designated_requirement(&printed), Some(LOCAL_KEY));
    assert_eq!(designated_requirement("Executable=/x\n"), None);
    assert_eq!(designated_requirement("designated => \n"), None);
}

#[test]
fn an_update_must_be_signed_like_the_running_app() {
    assert_eq!(oss_update_requirement(LOCAL_KEY, None).unwrap(), LOCAL_KEY);
    // A blank or odd team id is ignored rather than spliced in.
    assert_eq!(
        oss_update_requirement(LOCAL_KEY, Some(" ")).unwrap(),
        LOCAL_KEY
    );
    assert_eq!(
        oss_update_requirement(LOCAL_KEY, Some("AB\" or true")).unwrap(),
        LOCAL_KEY
    );
}

#[test]
fn a_release_team_is_trusted_alongside_the_running_apps_signer() {
    assert_eq!(
        oss_update_requirement(LOCAL_KEY, Some("ABCDE12345")).unwrap(),
        format!(
            "({LOCAL_KEY}) or (anchor apple generic and identifier \"cc.madnessinteractive.shellomancy\" \
             and certificate leaf[subject.OU] = \"ABCDE12345\")"
        )
    );
}

#[test]
fn an_ad_hoc_build_refuses_to_update() {
    let error = oss_update_requirement("cdhash H\"0123abcd\"", Some("ABCDE12345")).unwrap_err();
    assert!(error.to_string().contains("signed ad hoc"));
}

#[test]
fn checksum_files_are_read_like_shasum_writes_them() {
    let hash = "A".repeat(64);
    assert_eq!(
        expected_sha256(&format!("{hash}  Shellomancy-arm64.dmg\n")),
        Some("a".repeat(64))
    );
    assert_eq!(expected_sha256(&"a".repeat(64)), Some("a".repeat(64)));
    assert_eq!(expected_sha256("not-a-hash  file.dmg"), None);
    assert_eq!(expected_sha256(""), None);
}

#[test]
fn shellomancy_releases_are_named_after_the_product() {
    assert_eq!(app_name(Channel::Oss), "Shellomancy.app");
    assert_eq!(
        super::super::channel_versions::oss_channel_versions_url(
            "https://github.com/MadnessEngineering/shellomancy/releases"
        ),
        "https://github.com/MadnessEngineering/shellomancy/releases/latest/download/channel_versions.json"
    );
}
