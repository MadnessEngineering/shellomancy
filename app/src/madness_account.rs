//! Shellomancy: a Madness Interactive account (madnessinteractive.cc).
//!
//! The user signs in by pasting an Omnispindle API key (`omni_…`), which
//! Inventorium's API accepts as a bearer token. The key and the identity it
//! resolved to are kept in secure storage (the Keychain on macOS).
//!
//! Offline by default: nothing here touches the network until the user signs
//! in, and then only `GET /api/auth/user-info` to check the key. A stored
//! sign-in is restored from secure storage at startup without a request.
//!
//! This is the foundation for the Madness integrations (sending spells to
//! Omnispindle, syncing settings through Inventorium, the Mana Bar); each
//! takes its key from [`MadnessAccount::api_key`].

use serde::{Deserialize, Serialize};
use warpui::{Entity, ModelContext, SingletonEntity};
use warpui_extras::secure_storage;

/// Inventorium's REST API.
pub const MADNESS_API_ROOT: &str = "https://madnessinteractive.cc/api";
/// Where to mint an API key.
pub const MADNESS_KEYS_URL: &str = "https://madnessinteractive.cc/dashboard";
const API_KEY_PREFIX: &str = "omni_";
const SECURE_STORAGE_KEY: &str = "MadnessAccount";

/// What is kept in secure storage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct StoredAccount {
    api_key: String,
    /// What the account is shown as: its email, else its name, else its id.
    display: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MadnessAccountState {
    SignedOut,
    Checking,
    SignedIn {
        display: String,
    },
    /// A sign-in attempt failed; the message is ready to show.
    Failed(String),
}

#[derive(Clone, Copy, Debug)]
pub enum MadnessAccountEvent {
    StateChanged,
}

pub struct MadnessAccount {
    state: MadnessAccountState,
    api_key: Option<String>,
}

impl Entity for MadnessAccount {
    type Event = MadnessAccountEvent;
}

impl SingletonEntity for MadnessAccount {}

impl MadnessAccount {
    /// Restores a stored sign-in, if any. Needs secure storage registered.
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let stored = secure_storage::Model::handle(ctx)
            .as_ref(ctx)
            .read_value(SECURE_STORAGE_KEY)
            .ok()
            .and_then(|raw| serde_json::from_str::<StoredAccount>(&raw).ok());
        match stored {
            Some(stored) => Self {
                state: MadnessAccountState::SignedIn {
                    display: stored.display,
                },
                api_key: Some(stored.api_key),
            },
            None => Self {
                state: MadnessAccountState::SignedOut,
                api_key: None,
            },
        }
    }

    pub fn state(&self) -> &MadnessAccountState {
        &self.state
    }

    /// The signed-in API key, for integrations to send as a bearer token.
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    /// Checks `key` against madnessinteractive.cc and, if it's accepted,
    /// keeps it.
    pub fn sign_in(&mut self, key: String, ctx: &mut ModelContext<Self>) {
        let key = key.trim().to_owned();
        if !looks_like_api_key(&key) {
            self.set_state(
                MadnessAccountState::Failed(
                    madness_themes::t("shellomancy.account.madness.errorNotKey").to_owned(),
                ),
                ctx,
            );
            return;
        }
        self.set_state(MadnessAccountState::Checking, ctx);
        let request_key = key.clone();
        let _ = ctx.spawn(
            async move { fetch_display_name(&request_key).await },
            move |me, result, ctx| match result {
                Ok(display) => {
                    let stored = StoredAccount {
                        api_key: key,
                        display: display.clone(),
                    };
                    let saved = serde_json::to_string(&stored)
                        .map_err(|err| err.to_string())
                        .and_then(|raw| {
                            secure_storage::Model::handle(ctx)
                                .as_ref(ctx)
                                .write_value_with_owner_only_fallback(SECURE_STORAGE_KEY, &raw)
                                .map_err(|err| err.to_string())
                        });
                    match saved {
                        Ok(()) => {
                            me.api_key = Some(stored.api_key);
                            me.set_state(MadnessAccountState::SignedIn { display }, ctx);
                        }
                        Err(error) => me.set_state(
                            MadnessAccountState::Failed(madness_themes::tf(
                                "shellomancy.account.madness.errorStorage",
                                &[("error", &error)],
                            )),
                            ctx,
                        ),
                    }
                }
                Err(message) => me.set_state(MadnessAccountState::Failed(message), ctx),
            },
        );
    }

    /// Forgets the key, here and in secure storage.
    pub fn sign_out(&mut self, ctx: &mut ModelContext<Self>) {
        if let Err(err) = secure_storage::Model::handle(ctx)
            .as_ref(ctx)
            .remove_value(SECURE_STORAGE_KEY)
        {
            log::warn!("Could not remove the Madness account from secure storage: {err:?}");
        }
        self.api_key = None;
        self.set_state(MadnessAccountState::SignedOut, ctx);
    }

    fn set_state(&mut self, state: MadnessAccountState, ctx: &mut ModelContext<Self>) {
        self.state = state;
        ctx.emit(MadnessAccountEvent::StateChanged);
        ctx.notify();
    }
}

fn looks_like_api_key(key: &str) -> bool {
    key.len() > API_KEY_PREFIX.len()
        && key.starts_with(API_KEY_PREFIX)
        && !key.chars().any(char::is_whitespace)
}

/// `GET /api/auth/user-info` with the key; returns how to show the account, or
/// a message ready to show.
async fn fetch_display_name(key: &str) -> Result<String, String> {
    let response = http_client::Client::new_external()
        .get(format!("{MADNESS_API_ROOT}/auth/user-info"))
        .bearer_auth(key)
        .send()
        .await
        .map_err(|err| {
            madness_themes::tf(
                "shellomancy.account.madness.errorNetwork",
                &[("error", &err.to_string())],
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(madness_themes::tf(
            "shellomancy.account.madness.errorRejected",
            &[("status", &status.as_u16().to_string())],
        ));
    }
    let body: serde_json::Value = response.json().await.map_err(|err| {
        madness_themes::tf(
            "shellomancy.account.madness.errorNetwork",
            &[("error", &err.to_string())],
        )
    })?;
    Ok(display_name_from_user_info(&body))
}

/// The account's email, else name, else id, from a user-info response
/// (`{ "data": { "user": { ... } } }`).
fn display_name_from_user_info(body: &serde_json::Value) -> String {
    let user = &body["data"]["user"];
    ["email", "name", "id"]
        .iter()
        .find_map(|field| user[*field].as_str().filter(|value| !value.is_empty()))
        .unwrap_or("your account")
        .to_owned()
}

#[cfg(test)]
#[path = "madness_account_tests.rs"]
mod tests;
