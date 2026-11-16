//! Inventorium's settings-sync endpoints, and reading their replies.
//!
//! `GET  /api/user-settings/sync?categories=shellomancy,appearance`
//!   -> `{ success, categories: { <c>: { value | null, updatedAt } } }`
//! `POST /api/user-settings/sync { category: "shellomancy", value, updatedAt }`
//!   -> `{ success, applied, updatedAt[, value] }` (a refused, older write
//!   gets the stored value back)
//!
//! An omni_ key may only write the `shellomancy` category and read it plus
//! three `appearance` fields.

use std::time::Duration;

use futures::future::BoxFuture;
use serde_json::{Value, json};
use url::Url;

use super::model::SyncDoc;
use crate::madness_account::{MADNESS_API_ROOT, error_reason};

pub const CATEGORY: &str = "shellomancy";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a sync request failed.
#[derive(Clone, Debug, PartialEq)]
pub enum SyncError {
    /// Inventorium refused or doesn't have the endpoint (401/403/404): it
    /// doesn't take settings sync from this key (yet). Retrying won't help.
    Refused(String),
    /// Couldn't reach madnessinteractive.cc; worth retrying later.
    Network(String),
    /// Any other failure.
    Other(String),
}

/// What a pull brought back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pulled {
    /// Shellomancy's own document, if one was ever stored.
    pub doc: Option<SyncDoc>,
    /// The web app's appearance fields (for following), or `Value::Null`.
    pub appearance: Value,
}

/// What a push came to.
#[derive(Clone, Debug, PartialEq)]
pub enum Pushed {
    /// Stored; Inventorium now holds this stamp.
    Applied(i64),
    /// Inventorium holds something newer, which came back with the refusal.
    Refused(SyncDoc),
}

/// The network side, behind a trait so tests never touch it.
pub trait Transport: Send + Sync {
    fn pull(&self, key: String) -> BoxFuture<'static, Result<Pulled, SyncError>>;
    fn push(&self, key: String, doc: SyncDoc) -> BoxFuture<'static, Result<Pushed, SyncError>>;
}

/// The real thing: HTTPS to madnessinteractive.cc.
pub struct HttpTransport;

impl Transport for HttpTransport {
    fn pull(&self, key: String) -> BoxFuture<'static, Result<Pulled, SyncError>> {
        Box::pin(async move {
            let response = http_client::Client::new_external()
                .get(pull_url())
                .bearer_auth(&key)
                .timeout(REQUEST_TIMEOUT)
                .send()
                .await
                .map_err(|err| SyncError::Network(err.to_string()))?;
            let status = response.status().as_u16();
            let reply: Value = response.json().await.unwrap_or_default();
            check(status, &reply)?;
            Ok(parse_pull(&reply))
        })
    }

    fn push(&self, key: String, doc: SyncDoc) -> BoxFuture<'static, Result<Pushed, SyncError>> {
        Box::pin(async move {
            let body = push_body(&doc);
            let response = http_client::Client::new_external()
                .post(sync_url())
                .bearer_auth(&key)
                .timeout(REQUEST_TIMEOUT)
                .json(&body)
                .send()
                .await
                .map_err(|err| SyncError::Network(err.to_string()))?;
            let status = response.status().as_u16();
            let reply: Value = response.json().await.unwrap_or_default();
            check(status, &reply)?;
            parse_push(&reply, doc.stamp())
        })
    }
}

fn sync_url() -> Url {
    Url::parse(&format!("{MADNESS_API_ROOT}/user-settings/sync"))
        .expect("the API root is a valid URL")
}

pub fn pull_url() -> Url {
    let mut url = sync_url();
    url.query_pairs_mut()
        .append_pair("categories", &format!("{CATEGORY},appearance"));
    url
}

pub fn push_body(doc: &SyncDoc) -> Value {
    json!({
        "category": CATEGORY,
        "value": doc,
        "updatedAt": doc.stamp(),
    })
}

/// Sorts an HTTP status into success or the kind of failure.
pub fn check(status: u16, reply: &Value) -> Result<(), SyncError> {
    match status {
        200..=299 => Ok(()),
        401 | 403 | 404 => Err(SyncError::Refused(error_reason(status, reply))),
        _ => Err(SyncError::Other(error_reason(status, reply))),
    }
}

pub fn parse_pull(reply: &Value) -> Pulled {
    let categories = &reply["categories"];
    let doc = match &categories[CATEGORY]["value"] {
        Value::Null => None,
        value => Some(SyncDoc::from_value(value)),
    };
    Pulled {
        doc,
        appearance: categories["appearance"]["value"].clone(),
    }
}

pub fn parse_push(reply: &Value, sent_stamp: i64) -> Result<Pushed, SyncError> {
    if reply["success"] != true {
        return Err(SyncError::Other(
            reply["message"]
                .as_str()
                .unwrap_or("the server didn't say why")
                .to_owned(),
        ));
    }
    if reply["applied"] == false {
        return Ok(Pushed::Refused(SyncDoc::from_value(&reply["value"])));
    }
    Ok(Pushed::Applied(
        reply["updatedAt"].as_i64().unwrap_or(sent_stamp),
    ))
}

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
