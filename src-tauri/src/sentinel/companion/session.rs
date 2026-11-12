//! The companion's footprint on the integrity session row.
//!
//! A phone can pair only while the session is open and undecided: not
//! ended, no trust profile, nothing committed, no attestation stored. The
//! accepted record goes on `attestation_json` (with its binding), and the
//! level it verified to on `assurance_level`, so profile selection sees an
//! attested device exactly as it would on a phone.

use rusqlite::{params, OptionalExtension};

use super::{Accepted, CompanionBinding};
use crate::sentinel::attestation::DeviceAttestation;

/// The chain link a pairing adds ahead of the selection.
pub(crate) fn chain_link(session_id: &str, attestation_json: &str) -> String {
    format!("companion|{session_id}|{attestation_json}")
}

/// The session's start instant, when a phone may still pair with it.
pub(crate) fn pairable_started_at(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<String, String> {
    type Row = (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let (started_at, ended_at, profile, root, attestation): Row = conn
        .query_row(
            "SELECT started_at, ended_at, trust_profile, commitment_root, attestation_json
               FROM integrity_sessions WHERE id = ?1",
            params![session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("session not found")?;
    if ended_at.is_some() {
        return Err("session has ended".into());
    }
    if profile.is_some() || root.is_some() {
        return Err("a phone pairs before the trust profile is chosen".into());
    }
    if attestation.is_some() {
        return Err("session already carries an attestation".into());
    }
    Ok(started_at)
}

/// Store an accepted pairing. Returns the session's assurance level.
pub(crate) fn store_pairing(
    connection: &rusqlite::Connection,
    session_id: &str,
    accepted: &Accepted,
) -> Result<String, String> {
    let tx =
        rusqlite::Transaction::new_unchecked(connection, rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
    pairable_started_at(&tx, session_id)?;
    if accepted.record.companion.is_none() {
        return Err("not a companion attestation".into());
    }
    let json = serde_json::to_string(&accepted.record).map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE integrity_sessions SET attestation_json = ?2, assurance_level = ?3 WHERE id = ?1",
        params![session_id, json, accepted.assurance_level],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(accepted.assurance_level.clone())
}

/// The companion binding of a stored attestation, if it is a phone's.
pub(crate) fn stored_binding(attestation_json: Option<&str>) -> Option<CompanionBinding> {
    serde_json::from_str::<DeviceAttestation>(attestation_json?)
        .ok()?
        .companion
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::sentinel::attestation::{self as att, TrustConfig};
    use crate::sentinel::companion::tests::{did_of, hello_from, learner, offer, trust, PHONE};
    use crate::sentinel::companion::{accept_hello, tests::SESSION, tests::STARTED};
    use crate::sentinel::scoring::{Capabilities, TrustProfile};
    use crate::sentinel::trust_profile::{select_for_session, ProfileRequest};

    const NOW: i64 = 1_760_000_000;

    fn desktop_session() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute(
                "INSERT INTO integrity_sessions (id, status, purpose, started_at)
                 VALUES (?1, 'active', 'assessment', ?2)",
                params![SESSION, STARTED],
            )
            .unwrap();
        crate::settings::SettingsStore::set(
            db.conn(),
            crate::settings::registry::keys::IDENTITY_LOCAL_DID,
            did_of(&learner()),
        )
        .unwrap();
        db
    }

    fn paired(db: &Database) -> TrustConfig {
        let offer = offer(NOW);
        let (w, hello) = hello_from(&offer, &learner(), NOW);
        let accepted = accept_hello(&offer, &hello, PHONE, &trust(&w), NOW).unwrap();
        assert_eq!(
            store_pairing(db.conn(), SESSION, &accepted).unwrap(),
            att::ASSURANCE_DEVICE_ATTESTED
        );
        trust(&w)
    }

    fn companion_consented() -> ProfileRequest {
        ProfileRequest {
            consent: Capabilities {
                companion_phone: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn a_desktop_paired_with_an_attested_phone_runs_attested_and_issues_device_attested() {
        let db = desktop_session();
        let trust = paired(&db);

        let chosen = select_for_session(db.conn(), SESSION, &companion_consented(), None).unwrap();
        assert_eq!(chosen.record.selection.profile, TrustProfile::Attested);

        // The pairing is the first link of the chain; the selection folds
        // on top of it.
        let (root, attestation, selection): (String, String, String) = db
            .conn()
            .query_row(
                "SELECT commitment_root, attestation_json, profile_selection_json
                   FROM integrity_sessions WHERE id = ?1",
                [SESSION],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let fold = crate::domain::integrity_commitment::fold_commitment;
        let first = fold("", format!("companion|{SESSION}|{attestation}").as_bytes());
        assert_eq!(
            root,
            fold(
                &first,
                format!("trust_profile|{SESSION}|{selection}").as_bytes()
            )
        );

        let (assertion, _) = crate::credentials::issue::build_integrity_assertion(
            db.conn(),
            SESSION,
            "2026-11-12T11:00:00Z",
            &trust,
        )
        .unwrap();
        assert_eq!(assertion.assurance_level, att::ASSURANCE_DEVICE_ATTESTED);
        // The paired phone is the desktop's device axis in the assertion.
        assert_eq!(assertion.trust_profile.as_deref(), Some("attested"));
        assert!(assertion
            .signals_run
            .iter()
            .any(|s| s == "device_attestation"));
        assert!(!assertion
            .signals_degraded
            .iter()
            .any(|s| s == "device_attestation"));

        // Pairing again once the profile is locked is refused.
        assert!(pairable_started_at(db.conn(), SESSION)
            .unwrap_err()
            .contains("before the trust profile"));
    }

    #[test]
    fn a_paired_phone_without_companion_consent_does_not_make_the_session_attested() {
        let db = desktop_session();
        let trust = paired(&db);
        let chosen =
            select_for_session(db.conn(), SESSION, &ProfileRequest::default(), None).unwrap();
        assert_eq!(chosen.record.selection.profile, TrustProfile::Core);
        assert!(!chosen.record.selection.inputs.device_attested);

        // The unconsented pairing is dropped, so the credential cannot
        // claim it either.
        let (assertion, _) = crate::credentials::issue::build_integrity_assertion(
            db.conn(),
            SESSION,
            "2026-11-12T11:00:00Z",
            &trust,
        )
        .unwrap();
        assert_eq!(assertion.assurance_level, att::ASSURANCE_LOCAL);
        assert!(!assertion
            .signals_run
            .iter()
            .any(|s| s == "device_attestation"));
    }

    #[test]
    fn a_session_pairs_once_and_only_before_anything_is_committed() {
        let db = desktop_session();
        paired(&db);
        assert!(pairable_started_at(db.conn(), SESSION)
            .unwrap_err()
            .contains("already carries"));

        let ended = desktop_session();
        ended
            .conn()
            .execute(
                "UPDATE integrity_sessions SET ended_at = datetime('now')",
                [],
            )
            .unwrap();
        assert!(pairable_started_at(ended.conn(), SESSION).is_err());
    }
}
