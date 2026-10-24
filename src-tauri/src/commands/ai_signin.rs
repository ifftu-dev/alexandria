//! Commands for signing in to an AI provider in the browser.
//!
//! The flows themselves live in [`crate::ai_auth`]. These commands check the
//! profile's age rules first, run one sign-in at a time, and save the result
//! as an ordinary model connection usable by chat, the lesson tutor, and
//! Studio. Sign-in needs a desktop browser that can reach a loopback
//! listener, so mobile builds offer API keys only.

use std::sync::Mutex;

use alexandria_ai::endpoint::{self, Location};
use alexandria_ai::{Adapter, Protocol};
use alexandria_studio::model::{ConnectionAuth, StudioConnection, StudioDocument};
use alexandria_studio::store as studio_store;
use serde::Serialize;
use tokio::sync::oneshot;

use crate::ai_auth::{self, chatgpt, openrouter, DonePage, SignInError};
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// The sign-in in progress, if any: its cancel switch.
static ACTIVE: Mutex<Option<oneshot::Sender<()>>> = Mutex::new(None);

/// Context window assumed for ChatGPT plan models, which are large.
const CHATGPT_CONTEXT_TOKENS: u32 = 128_000;

async fn with_conn<T, F>(
    state: &State<'_, AppState>,
    label: &'static str,
    f: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&rusqlite::Connection) -> Result<T, String> + Send + 'static,
{
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            label,
            move |db| f(db.conn()),
        )
        .await
}

/// Claims the single sign-in slot. Dropping the guard frees it.
struct Slot;

impl Slot {
    fn claim() -> Result<(Self, oneshot::Receiver<()>), String> {
        let mut active = ACTIVE
            .lock()
            .map_err(|_| "signin_failed: busy".to_string())?;
        if active.as_ref().is_some_and(|tx| !tx.is_closed()) {
            return Err("signin_busy: another sign-in is already open in the browser".into());
        }
        let (tx, rx) = oneshot::channel();
        *active = Some(tx);
        Ok((Slot, rx))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE.lock() {
            *active = None;
        }
    }
}

fn signed_in_chatgpt(
    conn: &rusqlite::Connection,
) -> Result<Option<(StudioDocument<StudioConnection>, chatgpt::Credential)>, String> {
    let docs: Vec<StudioDocument<StudioConnection>> =
        studio_store::list(conn, "connection").map_err(|e| e.to_string())?;
    for doc in docs {
        if doc.value.auth == ConnectionAuth::ChatgptPlan {
            let secret = studio_store::secret(conn, &doc.id).map_err(|e| e.to_string())?;
            if let Some(cred) = secret.and_then(|s| chatgpt::Credential::parse(&s).ok()) {
                return Ok(Some((doc, cred)));
            }
        }
    }
    Ok(None)
}

/// Signs in to `provider` (`chatgpt` or `openrouter`) in the system browser
/// and saves the connection. `page` is shown in the browser afterwards.
#[tauri::command]
pub async fn ai_signin(
    state: State<'_, AppState>,
    provider: String,
    page: DonePage,
) -> Result<StudioDocument<StudioConnection>, String> {
    if cfg!(mobile) {
        return Err(
            "unsupported: sign-in with a browser is available on desktop; add an API key instead"
                .into(),
        );
    }
    match provider.as_str() {
        "chatgpt" => sign_in_chatgpt(&state, &page).await,
        "openrouter" => sign_in_openrouter(&state, &page).await,
        _ => Err("invalid_input: unknown provider".into()),
    }
}

async fn sign_in_chatgpt(
    state: &State<'_, AppState>,
    page: &DonePage,
) -> Result<StudioDocument<StudioConnection>, String> {
    let existing = with_conn(state, "ai-signin.chatgpt-check", |conn| {
        super::ai_chat::remote_allowed_for_age(conn, chatgpt::API_BASE)?;
        signed_in_chatgpt(conn)
    })
    .await?;
    let (_slot, mut cancel) = Slot::claim()?;
    let host_id = existing
        .as_ref()
        .map(|(_, cred)| cred.host_id.clone())
        .unwrap_or_else(chatgpt::new_host_id);
    let credential = chatgpt::sign_in(
        &chatgpt::Endpoints::default(),
        existing.as_ref().map(|(_, cred)| cred),
        &host_id,
        page,
        &mut cancel,
        ai_auth::open_browser,
    )
    .await
    .map_err(|e| e.to_string())?;

    let endpoint = endpoint::resolve(chatgpt::API_BASE, Location::Remote)
        .await
        .map_err(|e| format!("unreachable: {e}"))?;
    let models = Adapter::new(
        Protocol::Responses,
        endpoint,
        Some(credential.access_token.clone()),
    )
    .list_models()
    .await
    .map_err(|e| format!("{}: {e}", crate::ai_chat::error_code(&e)))?;
    let current = existing.as_ref().map(|(doc, _)| doc.value.model.clone());
    let model = current
        .filter(|m| models.contains(m))
        .or_else(|| models.first().cloned())
        .ok_or("model_unavailable: no models are available with this ChatGPT plan")?;
    let (id, revision, name) = match &existing {
        Some((doc, _)) => (doc.id.clone(), doc.revision, doc.value.name.clone()),
        None => (
            format!("chatgpt-{}", uuid::Uuid::new_v4()),
            0,
            "ChatGPT".to_string(),
        ),
    };
    let doc = StudioDocument {
        id: id.clone(),
        revision,
        value: StudioConnection {
            id,
            name,
            endpoint: chatgpt::API_BASE.into(),
            model,
            location: "cloud".into(),
            capability: "text".into(),
            enabled: true,
            has_key: true,
            context_tokens: Some(CHATGPT_CONTEXT_TOKENS),
            protocol: Protocol::Responses,
            auth: ConnectionAuth::ChatgptPlan,
        },
    };
    let encoded = credential.encode();
    with_conn(state, "ai-signin.chatgpt-save", move |conn| {
        studio_store::save_signed_in_connection(conn, &doc, &encoded).map_err(|e| e.to_string())
    })
    .await
}

async fn sign_in_openrouter(
    state: &State<'_, AppState>,
    page: &DonePage,
) -> Result<StudioDocument<StudioConnection>, String> {
    with_conn(state, "ai-signin.openrouter-check", |conn| {
        super::ai_chat::remote_allowed_for_age(conn, openrouter::API_BASE)
    })
    .await?;
    let (_slot, mut cancel) = Slot::claim()?;
    let key = openrouter::sign_in(
        &openrouter::Endpoints::default(),
        page,
        &mut cancel,
        ai_auth::open_browser,
    )
    .await
    .map_err(|e| e.to_string())?;
    let id = format!("openrouter-{}", uuid::Uuid::new_v4());
    let doc = StudioDocument {
        id: id.clone(),
        revision: 0,
        value: StudioConnection {
            id,
            name: "OpenRouter".into(),
            endpoint: openrouter::API_BASE.into(),
            model: openrouter::DEFAULT_MODEL.into(),
            location: "cloud".into(),
            capability: "text".into(),
            enabled: true,
            has_key: true,
            context_tokens: None,
            protocol: Protocol::Compatible,
            auth: ConnectionAuth::ApiKey,
        },
    };
    with_conn(state, "ai-signin.openrouter-save", move |conn| {
        let url = alexandria_ai::endpoint::parse(openrouter::API_BASE, Location::Remote)
            .map_err(|e| e.to_string())?;
        alexandria_ai::terms::check_credential(&url, &key, false).map_err(|e| e.to_string())?;
        studio_store::save_signed_in_connection(conn, &doc, &key).map_err(|e| e.to_string())
    })
    .await
}

/// Stops the sign-in waiting for the browser, if any.
#[tauri::command]
pub async fn ai_signin_cancel(_state: State<'_, AppState>) -> Result<(), String> {
    if let Ok(mut active) = ACTIVE.lock() {
        if let Some(tx) = active.take() {
            let _ = tx.send(());
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct Disconnected {
    /// For a ChatGPT sign-in: whether OpenAI confirmed the session ended.
    /// When false, the user can also disconnect Alexandria in ChatGPT
    /// settings.
    pub revoked: bool,
}

/// Removes a connection. A ChatGPT sign-in is revoked with OpenAI first.
#[tauri::command]
pub async fn ai_disconnect(
    state: State<'_, AppState>,
    connection_id: String,
) -> Result<Disconnected, String> {
    let id = connection_id.clone();
    let (doc, secret) = with_conn(&state, "ai-signin.disconnect-load", move |conn| {
        let doc = studio_store::connection(conn, &id).map_err(|e| e.to_string())?;
        let secret = studio_store::secret(conn, &id).map_err(|e| e.to_string())?;
        Ok((doc, secret))
    })
    .await?;
    let mut revoked = true;
    if doc.value.auth == ConnectionAuth::ChatgptPlan {
        revoked = match secret.as_deref().map(chatgpt::Credential::parse) {
            Some(Ok(cred)) => chatgpt::revoke(&chatgpt::Endpoints::default(), &cred).await,
            _ => false,
        };
    }
    with_conn(&state, "ai-signin.disconnect", move |conn| {
        studio_store::delete_connection(conn, &connection_id).map_err(|e| e.to_string())
    })
    .await?;
    Ok(Disconnected { revoked })
}

/// Opens a fixed provider page in the system browser.
#[tauri::command]
pub async fn ai_open_provider_page(
    _state: State<'_, AppState>,
    page: String,
) -> Result<(), String> {
    let url = match page.as_str() {
        "chatgpt_usage" => chatgpt::MANAGE_USAGE_URL,
        "openrouter_keys" => openrouter::KEYS_URL,
        _ => return Err("invalid_input: unknown page".into()),
    };
    ai_auth::open_browser(url).map_err(|e: SignInError| e.to_string())
}
