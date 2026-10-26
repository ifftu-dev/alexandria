//! The bearer value a connection sends, ready to use.
//!
//! An API key is sent as stored. A ChatGPT sign-in stores a token set; its
//! access token lasts an hour, so it is refreshed shortly before expiry.
//! Refreshes are serialized process-wide and re-read the stored credential
//! after taking the lock, because the refresh token rotates: two refreshes
//! racing with the same token would end the sign-in.

use std::future::Future;
use std::sync::OnceLock;

use alexandria_studio::model::{ConnectionAuth, StudioConnection};

use super::chatgpt::{self, Credential, RefreshError};
use super::now_secs;
use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::profile::scope::ProfileLease;

fn refresh_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub const SIGN_IN_AGAIN: &str =
    "signin_required: the ChatGPT sign-in has ended; sign in again from the connection settings";

/// The bearer value for `connection`, refreshing a ChatGPT sign-in first
/// when needed. `load` re-reads the stored secret and `save` replaces it.
/// Hosts call [`bearer_for`]; this is separate only so tests can stand in
/// for the database.
async fn bearer<L, LF, S, SF>(
    endpoints: &chatgpt::Endpoints,
    connection: &StudioConnection,
    stored: Option<String>,
    load: L,
    save: S,
) -> Result<Option<String>, String>
where
    L: FnOnce() -> LF,
    LF: Future<Output = Result<Option<String>, String>>,
    S: FnOnce(String) -> SF,
    SF: Future<Output = Result<(), String>>,
{
    if connection.auth != ConnectionAuth::ChatgptPlan {
        return Ok(stored);
    }
    let parse = |s: Option<String>| -> Result<Credential, String> {
        Credential::parse(&s.ok_or_else(|| SIGN_IN_AGAIN.to_string())?)
            .map_err(|_| SIGN_IN_AGAIN.to_string())
    };
    let current = parse(stored)?;
    if !current.needs_refresh(now_secs()) {
        return Ok(Some(current.access_token));
    }
    let _guard = refresh_lock().lock().await;
    let current = parse(load().await?)?;
    if !current.needs_refresh(now_secs()) {
        return Ok(Some(current.access_token));
    }
    match chatgpt::refresh(endpoints, &current).await {
        Ok(next) => {
            save(next.encode()).await?;
            Ok(Some(next.access_token))
        }
        Err(RefreshError::SignInAgain) => Err(SIGN_IN_AGAIN.into()),
        // Keep the credential; the current token may still have minutes left.
        Err(RefreshError::Temporary) if current.expires_at > now_secs() => {
            Ok(Some(current.access_token))
        }
        Err(RefreshError::Temporary) => {
            Err("unreachable: ChatGPT could not be reached to renew the sign-in".into())
        }
    }
}

/// [`bearer`] against the profile database: the one way the app obtains the
/// bearer a connection sends. `allowed` runs inside each database job,
/// before a sign-in is re-read for its refresh and before the renewed one is
/// saved; an error from it stops there and is returned. Studio passes its
/// profile guard; a chat run is bounded by its lease and passes `|| Ok(())`.
pub(crate) async fn bearer_for<G>(
    executor: &DatabaseExecutor,
    lease: &ProfileLease,
    connection: &StudioConnection,
    stored: Option<String>,
    allowed: G,
) -> Result<Option<String>, String>
where
    G: Fn() -> Result<(), String> + Clone + Send + 'static,
{
    let id = connection.id.clone();
    let save_id = connection.id.clone();
    let allowed_to_load = allowed.clone();
    bearer(
        &chatgpt::Endpoints::default(),
        connection,
        stored,
        || {
            executor.execute(
                DatabaseWorkload::Learner,
                lease.clone(),
                "ai-auth.load",
                move |db| {
                    allowed_to_load()?;
                    alexandria_studio::store::secret(db.conn(), &id).map_err(|e| e.to_string())
                },
            )
        },
        |next| {
            executor.execute(
                DatabaseWorkload::Learner,
                lease.clone(),
                "ai-auth.save",
                move |db| {
                    allowed()?;
                    alexandria_studio::store::replace_secret(db.conn(), &save_id, &next)
                        .map_err(|e| e.to_string())
                },
            )
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn connection(auth: ConnectionAuth) -> StudioConnection {
        StudioConnection {
            id: "c".into(),
            name: "ChatGPT".into(),
            endpoint: chatgpt::API_BASE.into(),
            model: "m".into(),
            location: "cloud".into(),
            capability: "text".into(),
            enabled: true,
            has_key: true,
            context_tokens: None,
            protocol: alexandria_ai::Protocol::Responses,
            auth,
        }
    }

    fn credential(expires_at: i64) -> Credential {
        Credential {
            client_id: "oaiapp_9".into(),
            sub: "u".into(),
            email: None,
            host_id: "h".into(),
            access_token: "fresh".into(),
            refresh_token: "dead".into(),
            id_token: "i".into(),
            expires_at,
            scopes: vec!["chatgpt.tokens.use.direct".into()],
        }
    }

    /// An endpoint set nothing listens on: any refresh fails as temporary.
    fn unreachable() -> chatgpt::Endpoints {
        chatgpt::Endpoints {
            token: "http://127.0.0.1:9/token".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn api_keys_pass_through_and_fresh_tokens_skip_the_refresh() {
        let key = bearer(
            &unreachable(),
            &connection(ConnectionAuth::ApiKey),
            Some("sk-1".into()),
            || async { panic!("no reload for a key") },
            |_| async { panic!("no save for a key") },
        )
        .await
        .unwrap();
        assert_eq!(key.as_deref(), Some("sk-1"));

        let fresh = credential(now_secs() + 3000).encode();
        let token = bearer(
            &unreachable(),
            &connection(ConnectionAuth::ChatgptPlan),
            Some(fresh),
            || async { panic!("no reload for a fresh token") },
            |_| async { panic!("no save for a fresh token") },
        )
        .await
        .unwrap();
        assert_eq!(token.as_deref(), Some("fresh"));
    }

    #[tokio::test]
    async fn an_expiring_token_rereads_the_store_and_survives_a_brief_outage() {
        // Another task refreshed while this one waited: no second refresh.
        let stale = credential(now_secs() + 10).encode();
        let mut refreshed = credential(now_secs() + 3000);
        refreshed.access_token = "rotated".into();
        let reloaded = refreshed.encode();
        let token = bearer(
            &unreachable(),
            &connection(ConnectionAuth::ChatgptPlan),
            Some(stale.clone()),
            || async move { Ok(Some(reloaded)) },
            |_| async { panic!("nothing to save") },
        )
        .await
        .unwrap();
        assert_eq!(token.as_deref(), Some("rotated"));

        // The refresh cannot reach OpenAI, but the token has seconds left.
        let saved = Arc::new(Mutex::new(None));
        let token = bearer(
            &unreachable(),
            &connection(ConnectionAuth::ChatgptPlan),
            Some(stale.clone()),
            || async move { Ok(Some(stale)) },
            |next| {
                let saved = saved.clone();
                async move {
                    *saved.lock().unwrap() = Some(next);
                    Ok(())
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(token.as_deref(), Some("fresh"));
        assert!(saved.lock().unwrap().is_none());

        // Expired and unreachable: say so rather than send a dead token.
        let expired = credential(now_secs() - 10).encode();
        let err = bearer(
            &unreachable(),
            &connection(ConnectionAuth::ChatgptPlan),
            Some(expired.clone()),
            || async move { Ok(Some(expired)) },
            |_| async { Ok(()) },
        )
        .await
        .unwrap_err();
        assert!(err.starts_with("unreachable"), "{err}");

        let err = bearer(
            &unreachable(),
            &connection(ConnectionAuth::ChatgptPlan),
            None,
            || async { Ok(None) },
            |_| async { Ok(()) },
        )
        .await
        .unwrap_err();
        assert!(err.starts_with("signin_required"));
    }
}
