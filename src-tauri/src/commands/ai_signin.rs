//! Commands for signing in to an AI provider in the browser.
//!
//! The flows themselves live in [`crate::ai_auth`]. These commands check the
//! profile's age rules first, run one sign-in at a time, and save the result
//! as an ordinary model connection usable by chat, the lesson tutor, and
//! Studio. Sign-in needs a desktop browser that can reach a loopback
//! listener, so mobile builds offer API keys only.

use std::sync::Mutex;

use alexandria_ai::model::Limits;
use alexandria_ai::{Connection, Location, Protocol};
use alexandria_studio::model::{ConnectionAuth, StudioConnection, StudioDocument};
use alexandria_studio::store as studio_store;
use serde::Serialize;
use tokio::sync::oneshot;

use crate::ai_access;
use crate::ai_auth::{self, chatgpt, openrouter, DonePage, SignInError};
use crate::ai_chat::vocab::refused;
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// The sign-in in progress, if any: its cancel switch.
static ACTIVE: Mutex<Option<oneshot::Sender<()>>> = Mutex::new(None);

/// Context window assumed for ChatGPT plan models, which are large.
const CHATGPT_CONTEXT_TOKENS: u32 = 128_000;

/// The connection a sign-in to `api_base` creates, before it has a model or
/// a key: a remote text connection. The age rules are checked against it
/// before the browser opens, and the saved connection is built from it.
pub(crate) fn sign_in_target(api_base: &str) -> StudioConnection {
    StudioConnection {
        id: String::new(),
        name: String::new(),
        endpoint: api_base.into(),
        model: String::new(),
        location: "cloud".into(),
        capability: "text".into(),
        enabled: true,
        has_key: true,
        context_tokens: None,
        protocol: Protocol::default(),
        auth: ConnectionAuth::default(),
    }
}

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
    let result = match provider.as_str() {
        "chatgpt" => sign_in_chatgpt(&state, &page).await,
        "openrouter" => sign_in_openrouter(&state, &page).await,
        _ => Err("invalid_input: unknown provider".into()),
    };
    // Messages are sanitized (codes and short reasons, never tokens), so they
    // are safe to log; they are what a failed sign-in leaves behind.
    match &result {
        Ok(doc) => log::info!("ai sign-in ({provider}): connected {}", doc.id),
        Err(e) if e.starts_with("signin_cancelled") => {}
        Err(e) => log::warn!("ai sign-in ({provider}) failed: {e}"),
    }
    result
}

/// Logs which step of a sign-in failed, then passes the error on.
fn step(name: &'static str) -> impl Fn(String) -> String {
    move |e| {
        if !e.starts_with("signin_cancelled") {
            log::warn!("ai sign-in step '{name}' failed: {e}");
        }
        e
    }
}

async fn sign_in_chatgpt(
    state: &State<'_, AppState>,
    page: &DonePage,
) -> Result<StudioDocument<StudioConnection>, String> {
    let existing = with_conn(state, "ai-signin.chatgpt-check", |conn| {
        ai_access::may_connect(conn, &sign_in_target(chatgpt::API_BASE)).map_err(refused)?;
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
    .map_err(|e| e.to_string())
    .map_err(step("chatgpt authorize and verify"))?;

    let target = Connection {
        endpoint: chatgpt::API_BASE.into(),
        location: Location::Remote,
        protocol: Protocol::Responses,
    };
    let adapter = alexandria_ai::connect(
        &target,
        Some(credential.access_token.as_str()),
        Limits::default(),
    )
    .await
    .map_err(|e| format!("{}: {e}", crate::ai_chat::error_code(&e)))
    .map_err(step("connect api.openai.com"))?;
    let models = adapter
        .list_models()
        .await
        .map_err(|e| format!("{}: {e}", crate::ai_chat::error_code(&e)))
        .map_err(step("list ChatGPT plan models"))?;
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
            model,
            context_tokens: Some(CHATGPT_CONTEXT_TOKENS),
            protocol: Protocol::Responses,
            auth: ConnectionAuth::ChatgptPlan,
            ..sign_in_target(chatgpt::API_BASE)
        },
    };
    let encoded = credential.encode();
    let bearer = credential.access_token;
    with_conn(state, "ai-signin.chatgpt-save", move |conn| {
        studio_store::save_signed_in_connection(conn, &doc, &encoded, &bearer)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(step("save ChatGPT connection"))
}

async fn sign_in_openrouter(
    state: &State<'_, AppState>,
    page: &DonePage,
) -> Result<StudioDocument<StudioConnection>, String> {
    with_conn(state, "ai-signin.openrouter-check", |conn| {
        ai_access::may_connect(conn, &sign_in_target(openrouter::API_BASE)).map_err(refused)
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
    .map_err(|e| e.to_string())
    .map_err(step("openrouter authorize and exchange"))?;
    save_openrouter(state, key).await
}

/// Saves an OpenRouter key issued by its sign-in as a connection.
async fn save_openrouter(
    state: &State<'_, AppState>,
    key: String,
) -> Result<StudioDocument<StudioConnection>, String> {
    let id = format!("openrouter-{}", uuid::Uuid::new_v4());
    let doc = StudioDocument {
        id: id.clone(),
        revision: 0,
        value: StudioConnection {
            id,
            name: "OpenRouter".into(),
            model: openrouter::DEFAULT_MODEL.into(),
            protocol: Protocol::Compatible,
            auth: ConnectionAuth::ApiKey,
            ..sign_in_target(openrouter::API_BASE)
        },
    };
    with_conn(state, "ai-signin.openrouter-save", move |conn| {
        studio_store::save_signed_in_connection(conn, &doc, &key, &key).map_err(|e| e.to_string())
    })
    .await
    .map_err(step("save OpenRouter connection"))
}

/// A paste-the-code sign-in waiting for its code: the PKCE verifier and when
/// it started. One at a time; starting again replaces it.
static PENDING_CODE: Mutex<Option<(String, std::time::Instant)>> = Mutex::new(None);

/// How long a paste-the-code sign-in waits for its code.
const CODE_LIFETIME: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Starts the paste-the-code sign-in (`openrouter` only), used on phones:
/// opens OpenRouter in the browser, which then shows a code for the user to
/// paste back with [`ai_signin_code_finish`].
#[tauri::command]
pub async fn ai_signin_code_start(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    provider: String,
) -> Result<(), String> {
    if provider != "openrouter" {
        return Err("invalid_input: unknown provider".into());
    }
    with_conn(&state, "ai-signin.code-check", |conn| {
        ai_access::may_connect(conn, &sign_in_target(openrouter::API_BASE)).map_err(refused)
    })
    .await?;
    let (verifier, challenge) = ai_auth::pkce();
    let url = openrouter::code_url(&openrouter::Endpoints::default(), &challenge)
        .map_err(|e| e.to_string())?;
    *PENDING_CODE
        .lock()
        .map_err(|_| "signin_failed: busy".to_string())? =
        Some((verifier, std::time::Instant::now()));
    ai_auth::open_browser_on_main(&app, url)
        .await
        .map_err(|e| e.to_string())
        .map_err(step("open OpenRouter"))
}

/// Finishes the paste-the-code sign-in with the code OpenRouter showed.
#[tauri::command]
pub async fn ai_signin_code_finish(
    state: State<'_, AppState>,
    code: String,
) -> Result<StudioDocument<StudioConnection>, String> {
    let code = openrouter::clean_code(&code)
        .ok_or_else(|| SignInError::CodeRejected.to_string())?
        .to_owned();
    with_conn(&state, "ai-signin.code-check", |conn| {
        ai_access::may_connect(conn, &sign_in_target(openrouter::API_BASE)).map_err(refused)
    })
    .await?;
    let verifier = take_pending(std::time::Instant::now()).map_err(|e| e.to_string())?;
    let key = openrouter::exchange(&openrouter::Endpoints::default(), &code, &verifier)
        .await
        .map_err(|e| e.to_string())
        .map_err(step("openrouter code exchange"))?;
    save_openrouter(&state, key).await
}

/// The waiting verifier, once: a code is single-use and expires.
fn take_pending(now: std::time::Instant) -> Result<String, SignInError> {
    let pending = PENDING_CODE
        .lock()
        .map_err(|_| SignInError::Expired)?
        .take();
    match pending {
        Some((verifier, started)) if now.duration_since(started) <= CODE_LIFETIME => Ok(verifier),
        _ => Err(SignInError::Expired),
    }
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
    app: tauri::AppHandle,
    page: String,
) -> Result<(), String> {
    let url = match page.as_str() {
        "chatgpt_usage" => chatgpt::MANAGE_USAGE_URL,
        "openrouter_keys" => openrouter::KEYS_URL,
        _ => return Err("invalid_input: unknown page".into()),
    };
    ai_auth::open_browser_on_main(&app, url.into())
        .await
        .map_err(|e: SignInError| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_code_needs_a_live_single_use_sign_in() {
        let now = std::time::Instant::now();
        *PENDING_CODE.lock().unwrap() = Some(("v1".into(), now));
        assert_eq!(take_pending(now).unwrap(), "v1");
        assert_eq!(
            take_pending(now).unwrap_err(),
            SignInError::Expired,
            "single use"
        );
        *PENDING_CODE.lock().unwrap() = Some(("v2".into(), now));
        let later = now + CODE_LIFETIME + std::time::Duration::from_secs(1);
        assert_eq!(take_pending(later).unwrap_err(), SignInError::Expired);
    }
}
