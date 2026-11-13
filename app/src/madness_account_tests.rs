use serde_json::json;

use super::*;

#[test]
fn only_omni_keys_are_tried() {
    assert!(looks_like_api_key("omni_abc123"));
    assert!(!looks_like_api_key("omni_"));
    assert!(!looks_like_api_key("sk-ant-abc"));
    assert!(!looks_like_api_key("omni_abc 123"));
    assert!(!looks_like_api_key(""));
}

#[test]
fn display_prefers_email_then_name_then_id() {
    let full =
        json!({"data": {"user": {"email": "dan@example.com", "name": "Dan", "id": "auth0|x"}}});
    assert_eq!(display_name_from_user_info(&full), "dan@example.com");

    let no_email = json!({"data": {"user": {"email": "", "name": "Dan", "id": "auth0|x"}}});
    assert_eq!(display_name_from_user_info(&no_email), "Dan");

    let id_only = json!({"data": {"user": {"id": "auth0|x"}}});
    assert_eq!(display_name_from_user_info(&id_only), "auth0|x");

    assert_eq!(display_name_from_user_info(&json!({})), "your account");
}

#[test]
fn stored_account_round_trips() {
    let stored = StoredAccount {
        api_key: "omni_abc".to_owned(),
        display: "dan@example.com".to_owned(),
    };
    let raw = serde_json::to_string(&stored).unwrap();
    assert_eq!(serde_json::from_str::<StoredAccount>(&raw).unwrap(), stored);
}
