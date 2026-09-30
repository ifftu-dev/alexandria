use crate::profile::scope::ProfileState as State;
use crate::AppState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonhoodPrivateReceipt {
    pub id: String,
    pub kind: String,
    pub subject_did: String,
    pub network_id: String,
    pub created_at: u64,
    pub expires_at: u64,
}

fn require_enabled() -> Result<(), String> {
    if !super::personhood_lab::enabled() {
        return Err(
            "Private synthetic receipts require an explicitly enabled Android debug build".into(),
        );
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Invalid receipt challenge identifier".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn personhood_receipt_prepare(state: State<'_, AppState>) -> Result<String, String> {
    require_enabled()?;
    #[cfg(feature = "personhood-lab")]
    {
        implementation::prepare(state).await
    }
    #[cfg(not(feature = "personhood-lab"))]
    {
        let _ = state;
        Err("Personhood Lab is disabled".into())
    }
}

#[tauri::command]
pub async fn personhood_receipt_prove(
    state: State<'_, AppState>,
    challenge_id: String,
) -> Result<PersonhoodPrivateReceipt, String> {
    require_enabled()?;
    validate_id(&challenge_id)?;
    #[cfg(feature = "personhood-lab")]
    {
        implementation::prove(state, challenge_id).await
    }
    #[cfg(not(feature = "personhood-lab"))]
    {
        let _ = (state, challenge_id);
        Err("Personhood Lab is disabled".into())
    }
}

#[tauri::command]
pub async fn personhood_receipt_cancel(
    state: State<'_, AppState>,
    challenge_id: String,
) -> Result<(), String> {
    require_enabled()?;
    validate_id(&challenge_id)?;
    #[cfg(feature = "personhood-lab")]
    {
        implementation::cancel(state, challenge_id).await
    }
    #[cfg(not(feature = "personhood-lab"))]
    {
        let _ = (state, challenge_id);
        Err("Personhood Lab is disabled".into())
    }
}

#[tauri::command]
pub async fn personhood_receipt_list(
    state: State<'_, AppState>,
) -> Result<Vec<PersonhoodPrivateReceipt>, String> {
    require_enabled()?;
    #[cfg(feature = "personhood-lab")]
    {
        implementation::list(state).await
    }
    #[cfg(not(feature = "personhood-lab"))]
    {
        let _ = state;
        Err("Personhood Lab is disabled".into())
    }
}

#[cfg(feature = "personhood-lab")]
mod implementation {
    use super::*;
    use crate::commands::personhood_lab::invoke_native;
    use crate::db::executor::DatabaseWorkload;
    use alexandria_personhood::{
        self as protocol, store, Policy, Receipt, SignedChallenge, Submission,
    };
    use ed25519_dalek::SigningKey;
    use rand::RngCore;
    use serde::Deserialize;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use zeroize::Zeroizing;

    static PROVER: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

    fn now() -> Result<u64, String> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|t| t.as_secs())
            .map_err(|e| e.to_string())
    }

    async fn account(state: &State<'_, AppState>) -> Result<SigningKey, String> {
        let guard = state.keystore.lock().await;
        let mnemonic = Zeroizing::new(
            guard
                .as_ref()
                .ok_or("Profile is locked")?
                .retrieve_mnemonic()
                .map_err(|e| e.to_string())?,
        );
        drop(guard);
        let wallet =
            crate::crypto::wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
        Ok(wallet.signing_key.clone())
    }

    fn policy(account: &SigningKey) -> Result<(Policy, SigningKey), String> {
        let verifier = protocol::local_verifier_key(account).map_err(|e| e.to_string())?;
        let network = crate::network_profile::embedded_preprod().map_err(|e| e.to_string())?;
        Ok((
            Policy::synthetic(&network.network_id, &verifier.verifying_key()),
            verifier,
        ))
    }

    fn current_account(db: &rusqlite::Connection, account: &SigningKey) -> Result<(), String> {
        let did = alexandria_verify::did::did_from_verifying_key(&account.verifying_key());
        let time = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let current = crate::crypto::key_registry::resolve_key_at(db, &did, &time)
            .map_err(|e| e.to_string())?;
        if let Some(entry) = current {
            if entry.public_key_bytes != account.verifying_key().as_bytes() {
                return Err("Account signing key is no longer current".into());
            }
        } else {
            let registered: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM key_registry WHERE did = ?1)",
                    [&did.0],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if registered {
                return Err("Account has no currently valid registered key".into());
            }
        }
        Ok(())
    }

    pub async fn prepare(state: State<'_, AppState>) -> Result<String, String> {
        let account = account(&state).await?;
        let (policy, verifier) = policy(&account)?;
        let mut session_nonce = [0; 32];
        let mut nonce = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut session_nonce);
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let challenge = protocol::issue_challenge(
            &policy,
            &account.verifying_key(),
            &verifier,
            session_nonce,
            nonce,
            now()?,
        )
        .map_err(|e| e.to_string())?;
        let id = challenge.body.nonce.clone();
        let lease = state.profile_lease();
        state
            .db_executor
            .execute(
                DatabaseWorkload::Learner,
                lease.clone(),
                "personhood.prepare",
                move |db| {
                    current_account(db.conn(), &account)?;
                    lease.with_current(|| store::prepare(db.conn(), lease.session_id(), &challenge))
                },
            )
            .await?;
        Ok(id)
    }

    #[derive(Deserialize)]
    struct NativeReceipt {
        job_id: Option<String>,
        phase: String,
        error: Option<String>,
        proof: Option<protocol::Groth16Proof>,
        public_signals: Option<[String; 9]>,
    }

    fn native(action: &str, id: &str) -> Result<NativeReceipt, String> {
        invoke_native(&serde_json::json!({"action":action,"job_id":id}).to_string())
    }
    struct NativeGuard(String);
    impl Drop for NativeGuard {
        fn drop(&mut self) {
            let _ = native("receipt_discard", &self.0);
        }
    }

    async fn pending(
        state: &State<'_, AppState>,
        id: String,
        policy: Policy,
        account: SigningKey,
    ) -> Result<SignedChallenge, String> {
        let lease = state.profile_lease();
        state
            .db_executor
            .execute(
                DatabaseWorkload::Learner,
                lease.clone(),
                "personhood.pending",
                move |db| {
                    current_account(db.conn(), &account)?;
                    store::pending(
                        db.conn(),
                        lease.session_id(),
                        &id,
                        &policy,
                        &account.verifying_key(),
                        now()?,
                    )
                },
            )
            .await
    }

    pub async fn prove(
        state: State<'_, AppState>,
        id: String,
    ) -> Result<PersonhoodPrivateReceipt, String> {
        let _permit = PROVER
            .try_acquire()
            .map_err(|_| "A private receipt proof is already running")?;
        let account = account(&state).await?;
        let (policy, verifier) = policy(&account)?;
        let lookup_id = id.clone();
        let lookup_policy = policy.clone();
        let lookup_account = account.clone();
        let lookup_lease = state.profile_lease();
        let existing = state
            .db_executor
            .execute(
                DatabaseWorkload::Learner,
                lookup_lease.clone(),
                "personhood.retry",
                move |db| {
                    current_account(db.conn(), &lookup_account)?;
                    store::existing(
                        db.conn(),
                        lookup_lease.session_id(),
                        &lookup_id,
                        &lookup_account.verifying_key(),
                        &lookup_policy,
                    )
                },
            )
            .await?;
        if let Some(receipt) = existing {
            return Ok(summary(receipt));
        }
        let challenge = pending(&state, id.clone(), policy.clone(), account.clone()).await?;
        let _cleanup = NativeGuard(id.clone());
        let start: NativeReceipt = invoke_native(&serde_json::json!({"action":"receipt_prove", "job_id":id,
            "signal_hash":protocol::signal_hash(&challenge.body).map_err(|e| e.to_string())?, "nullifier_seed":policy.nullifier_seed}).to_string())?;
        if start.job_id.as_deref() != Some(&id) {
            return Err("The native lab is busy or unavailable".into());
        }
        let completed = loop {
            if !state.profile_lease().is_current() {
                return Err("Profile locked during receipt generation".into());
            }
            pending(&state, id.clone(), policy.clone(), account.clone()).await?;
            let response = native("receipt_status", &id)?;
            if response.job_id.as_deref() != Some(&id) {
                return Err("Receipt worker changed".into());
            }
            match response.phase.as_str() {
                "complete" => break response,
                "error" | "cancelled" => {
                    return Err(response
                        .error
                        .unwrap_or_else(|| "Receipt generation cancelled".into()))
                }
                _ => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        };
        let submission = Submission::signed(
            challenge.clone(),
            completed.proof.ok_or("Missing native proof")?,
            completed
                .public_signals
                .ok_or("Missing native public signals")?,
            &account,
        )
        .map_err(|e| e.to_string())?;
        let verification_policy = policy.clone();
        let public_key = account.verifying_key();
        // Keep the lease alive and join the CPU work, even if the profile closes.
        let verification_lease = state.profile_lease();
        let verified = tauri::async_runtime::spawn_blocking(move || {
            let _lease = verification_lease;
            protocol::verify_submission(
                &verification_policy,
                &challenge,
                submission,
                &public_key,
                now()?,
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
        let lease = state.profile_lease();
        let receipt = state
            .db_executor
            .execute(
                DatabaseWorkload::Learner,
                lease.clone(),
                "personhood.consume",
                move |db| {
                    let tx = db
                        .conn()
                        .unchecked_transaction()
                        .map_err(|e| e.to_string())?;
                    current_account(&tx, &account)?;
                    let receipt = store::consume(
                        &tx,
                        lease.session_id(),
                        &verified,
                        &policy,
                        &account.verifying_key(),
                        &verifier,
                        now()?,
                    )?;
                    lease.with_current(|| tx.commit().map_err(|e| e.to_string()))?;
                    Ok(receipt)
                },
            )
            .await?;
        Ok(summary(receipt))
    }

    pub async fn cancel(state: State<'_, AppState>, id: String) -> Result<(), String> {
        let lease = state.profile_lease();
        let nonce = id.clone();
        state
            .db_executor
            .execute(
                DatabaseWorkload::Learner,
                lease.clone(),
                "personhood.cancel",
                move |db| store::cancel(db.conn(), lease.session_id(), &nonce),
            )
            .await?;
        native("receipt_discard", &id)?;
        Ok(())
    }

    fn summary(receipt: Receipt) -> PersonhoodPrivateReceipt {
        PersonhoodPrivateReceipt {
            id: receipt.body.id,
            kind: "synthetic_diagnostic".into(),
            subject_did: receipt.body.subject_did,
            network_id: receipt.body.network_id,
            created_at: receipt.body.created_at,
            expires_at: receipt.body.expires_at,
        }
    }

    pub async fn list(state: State<'_, AppState>) -> Result<Vec<PersonhoodPrivateReceipt>, String> {
        let account = account(&state).await?;
        let (policy, verifier) = policy(&account)?;
        let did = alexandria_verify::did::did_from_verifying_key(&account.verifying_key()).0;
        state
            .db_executor
            .execute(
                DatabaseWorkload::Learner,
                state.profile_lease(),
                "personhood.list",
                move |db| {
                    current_account(db.conn(), &account)?;
                    store::receipts(db.conn(), &did, &policy.network_id)?
                        .into_iter()
                        .map(|receipt| {
                            protocol::verify_receipt(&receipt, &verifier.verifying_key())
                                .map_err(|e| e.to_string())?;
                            Ok(summary(receipt))
                        })
                        .collect()
                },
            )
            .await
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn expired_registered_key_does_not_fall_back_to_did_key() {
            let db = crate::db::Database::open_in_memory().unwrap();
            db.run_migrations().unwrap();
            let account = SigningKey::from_bytes(&[1; 32]);
            current_account(db.conn(), &account).unwrap();
            let did = alexandria_verify::did::did_from_verifying_key(&account.verifying_key()).0;
            db.conn().execute("INSERT INTO key_registry(did, key_id, public_key_hex, valid_from, valid_until) VALUES (?1,'test',?2,'2000-01-01T00:00:00Z','2001-01-01T00:00:00Z')", rusqlite::params![did,hex::encode(account.verifying_key().as_bytes())]).unwrap();
            assert!(current_account(db.conn(), &account).is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_identifiers_are_bounded_before_native_dispatch() {
        assert!(validate_id(&"a1".repeat(32)).is_ok());
        for id in ["", "{}", "../run", &"A".repeat(64), &"0".repeat(65)] {
            assert!(validate_id(id).is_err());
        }
    }
}
