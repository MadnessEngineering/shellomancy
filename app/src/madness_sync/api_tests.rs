use serde_json::json;

use super::*;
use crate::madness_sync::model::Stamped;

#[test]
fn pull_asks_for_shellomancy_and_appearance() {
    let url = pull_url();
    assert_eq!(url.path(), "/api/user-settings/sync");
    assert_eq!(url.query(), Some("categories=shellomancy%2Cappearance"));
}

#[test]
fn push_sends_the_doc_under_its_category_with_its_stamp() {
    let doc = SyncDoc {
        v: 1,
        theme: None,
        voice: Some(Stamped {
            value: "dwarf".to_owned(),
            at: 77,
        }),
    };
    assert_eq!(
        push_body(&doc),
        json!({
            "category": "shellomancy",
            "value": {"v": 1, "voice": {"value": "dwarf", "at": 77}},
            "updatedAt": 77,
        })
    );
}

#[test]
fn pull_reads_both_categories() {
    let reply = json!({"success": true, "categories": {
        "shellomancy": {"value": {"v": 1, "voice": {"value": "dwarf", "at": 5}}, "updatedAt": 5},
        "appearance": {"value": {"chromamancyTheme": "labops"}, "updatedAt": 3}
    }});

    let pulled = parse_pull(&reply);

    assert_eq!(
        pulled.doc.and_then(|doc| doc.voice).map(|v| v.value),
        Some("dwarf".to_owned())
    );
    assert_eq!(pulled.appearance, json!({"chromamancyTheme": "labops"}));
}

#[test]
fn pull_with_nothing_stored_has_no_doc() {
    let reply = json!({"success": true, "categories": {
        "shellomancy": {"value": null, "updatedAt": 0},
        "appearance": {"value": null, "updatedAt": 0}
    }});

    let pulled = parse_pull(&reply);

    assert_eq!(pulled.doc, None);
    assert_eq!(pulled.appearance, Value::Null);
}

#[test]
fn push_replies_are_read() {
    assert_eq!(
        parse_push(
            &json!({"success": true, "applied": true, "updatedAt": 9}),
            9
        ),
        Ok(Pushed::Applied(9))
    );
    let refused = parse_push(
        &json!({"success": true, "applied": false, "updatedAt": 12,
                "value": {"v": 1, "voice": {"value": "banana", "at": 12}}}),
        9,
    );
    match refused {
        Ok(Pushed::Refused(doc)) => assert_eq!(doc.stamp(), 12),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(matches!(
        parse_push(&json!({"success": false, "message": "nope"}), 9),
        Err(SyncError::Other(_))
    ));
}

#[test]
fn missing_endpoint_or_refused_key_is_not_retried() {
    for status in [401, 403, 404] {
        assert!(matches!(
            check(status, &json!({"message": "x"})),
            Err(SyncError::Refused(_))
        ));
    }
    assert!(matches!(check(500, &Value::Null), Err(SyncError::Other(_))));
    assert_eq!(check(200, &Value::Null), Ok(()));
}
