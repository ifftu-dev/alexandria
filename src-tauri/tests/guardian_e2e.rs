//! Guardian-link end-to-end test.
//!
//! Models two people on two devices — a minor learner ("child") and
//! their parent ("guardian") — with REAL cryptography end to end:
//! freshly generated wallets, real did:key derivation, a real
//! Ed25519-signed guardianship RoleCredential issued through the
//! production `issue_credential_impl` pipeline, and real signature
//! verification inside the child's `handle_guardian_request`.
//!
//! It drives the production protocol path exactly as bytes would
//! arrive over `/alexandria/guardian/1.0` (the transport is a
//! transparent carrier for these sealed payloads, same convention as
//! `settings_sync_e2e.rs`):
//!
//!   1. Child onboards as a minor → profile gated (`pending_guardian`).
//!   2. Child generates a single-use invite.
//!   3. Parent accepts: issues the guardianship VC, sends `Link`.
//!   4. Child verifies the VC, stores the link, ACTIVATES.
//!   5. Child's learning activity pushes to the parent, sealed.
//!   6. Parent pulls a fresh snapshot on demand.
//!   7. A replayed invite is refused.
//!   8. Guardian revokes → still-minor child re-gates.
//!   9. The same revoke against an 18-year-old leaves them active.

use app_lib::commands::credentials::{issue_credential_impl, IssueCredentialRequest};
use app_lib::crypto::did::derive_did_key;
use app_lib::crypto::guardian as invite_codec;
use app_lib::crypto::wallet;
use app_lib::db::Database;
use app_lib::domain::vc::{Claim, CredentialType, RoleClaim};
use app_lib::p2p::guardian::{
    apply_activity_snapshot, build_activity_snapshot, handle_guardian_request, open,
    record_pending_invite, seal, GuardianActivityPayload, GuardianRequest, GuardianResponse,
};

use ed25519_dalek::SigningKey;
use rusqlite::Connection;

const PARENT_PEER: &str = "12D3KooWParentDevice";
const CHILD_PEER: &str = "12D3KooWChildDevice";

struct Person {
    db: Database,
    signing_key: SigningKey,
    did: String,
    stake: String,
}

/// Stand up one person's device: fresh wallet, migrated DB, identity
/// row, cached DID — the state `create_profile` + unlock leave behind.
fn person(role: &str, birthdate: Option<&str>, activation: &str) -> Person {
    let w = wallet::generate_wallet().expect("generate wallet");
    let signing_key = SigningKey::from_bytes(&w.signing_key.to_bytes());
    let did = derive_did_key(&signing_key).as_str().to_string();

    let db = Database::open_in_memory().expect("in-memory db");
    db.run_migrations().expect("migrations");
    db.conn()
        .execute(
            "INSERT INTO local_identity \
             (id, stake_address, payment_address, display_name, account_role, birthdate, activation_state) \
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                w.stake_address,
                w.payment_address,
                format!("{role}-person"),
                role,
                birthdate,
                activation
            ],
        )
        .expect("seed identity");
    // The app caches the local DID in app_settings at unlock.
    db.conn()
        .execute(
            "INSERT INTO app_settings (key, value, scope, updated_at) \
             VALUES ('identity.local_did', ?1, 'device', datetime('now'))",
            rusqlite::params![did],
        )
        .expect("cache DID");

    Person {
        db,
        signing_key,
        did,
        stake: w.stake_address.clone(),
    }
}

fn activation_state(conn: &Connection) -> String {
    conn.query_row(
        "SELECT activation_state FROM local_identity WHERE id = 1",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

/// Child generates an invite exactly as `guardian_create_invite` does:
/// fresh shared key, encoded code, hash recorded as pending.
fn child_makes_invite(child: &Person) -> (String, invite_codec::GuardianInvite) {
    let shared_key = app_lib::crypto::pairing::generate_shared_key();
    let invite = invite_codec::GuardianInvite {
        child_did: child.did.clone(),
        child_stake_address: child.stake.clone(),
        child_peer_id: CHILD_PEER.into(),
        addresses: vec!["/ip4/192.168.1.10/tcp/4001".into()],
        shared_key,
        display_name: Some("learner-person".into()),
    };
    let code = invite_codec::encode(&invite).expect("encode invite");
    record_pending_invite(
        child.db.conn(),
        &invite_codec::code_hash(&code),
        &shared_key,
        3600,
    )
    .expect("record pending invite");
    (code, invite)
}

/// Parent-side accept: issue the real guardianship VC and build the
/// `Link` request `guardian_accept_invite` would send.
fn parent_builds_link(
    parent: &Person,
    invite: &invite_codec::GuardianInvite,
    code: &str,
    link_id: &str,
) -> GuardianRequest {
    let parent_did = derive_did_key(&parent.signing_key);
    let req = IssueCredentialRequest {
        credential_type: CredentialType::RoleCredential,
        subject: app_lib::crypto::did::Did(invite.child_did.clone()),
        claim: Claim::Role(RoleClaim {
            role: "guardian".into(),
            scope: Some(parent.did.clone()),
        }),
        evidence_refs: vec![],
        expiration_date: None,
        supersedes: None,
        integrity_session_id: None,
        integrity_policy: None,
    };
    let now = chrono::Utc::now().to_rfc3339();
    let vc = issue_credential_impl(
        parent.db.conn(),
        &parent.signing_key,
        &parent_did,
        &req,
        &now,
    )
    .expect("parent issues guardianship VC");
    let vc_json = serde_json::to_string(&vc).expect("serialize VC");

    // Parent records its side of the link (status pending until Linked).
    parent
        .db
        .conn()
        .execute(
            "INSERT INTO guardian_links \
             (id, side, peer_did, peer_stake_address, peer_peer_id, shared_key, status) \
             VALUES (?1, 'guardian', ?2, ?3, ?4, ?5, 'pending')",
            rusqlite::params![
                link_id,
                invite.child_did,
                invite.child_stake_address,
                CHILD_PEER,
                &invite.shared_key[..]
            ],
        )
        .expect("parent stores pending link");

    GuardianRequest::Link {
        code_hash: invite_codec::code_hash(code),
        link_id: link_id.into(),
        guardian_did: parent.did.clone(),
        guardian_stake_address: parent.stake.clone(),
        guardian_display_name: Some("parent-person".into()),
        guardian_vc_json: vc_json,
    }
}

/// The full happy path both later tests build on. Returns the linked
/// pair plus the shared key.
fn establish_link(birthdate: &str) -> (Person, Person, [u8; 32], String) {
    let child = person("learner", Some(birthdate), "pending_guardian");
    let parent = person("parent", None, "active");
    let link_id = "link-e2e-1".to_string();

    let (code, invite) = child_makes_invite(&child);
    let link_req = parent_builds_link(&parent, &invite, &code, &link_id);

    // The Link request crosses the wire; the child's node handles it.
    let resp = handle_guardian_request(child.db.conn(), PARENT_PEER, &link_req);
    let GuardianResponse::Linked { sealed_snapshot } = resp else {
        panic!("expected Linked, got {resp:?}");
    };

    // Parent opens the initial snapshot and goes active.
    let snapshot: GuardianActivityPayload =
        open(&invite.shared_key, &sealed_snapshot).expect("parent opens initial snapshot");
    assert_eq!(snapshot.child_did, child.did);
    assert_eq!(snapshot.birthdate.as_deref(), Some(birthdate));
    parent
        .db
        .conn()
        .execute(
            "UPDATE guardian_links SET status = 'active' WHERE id = ?1",
            rusqlite::params![link_id],
        )
        .unwrap();
    apply_activity_snapshot(parent.db.conn(), &link_id, &snapshot)
        .expect("parent applies initial snapshot");

    (child, parent, invite.shared_key, link_id)
}

#[test]
fn full_guardian_lifecycle_with_real_crypto() {
    // ── 1–4: gated minor → invite → link → activation ────────────
    let (child, parent, key, link_id) = establish_link("2012-03-15");

    assert_eq!(
        activation_state(child.db.conn()),
        "active",
        "child must activate the moment the guardian VC verifies"
    );

    // The child stored the parent-issued credential, signed and typed.
    let (issuer, ctype): (String, String) = child
        .db
        .conn()
        .query_row(
            "SELECT issuer_did, credential_type FROM credentials WHERE subject_did = ?1",
            rusqlite::params![child.did],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("guardianship VC stored on child");
    assert_eq!(issuer, parent.did);
    assert_eq!(ctype, "RoleCredential");

    // Ward-side link row active.
    let (side, status): (String, String) = child
        .db
        .conn()
        .query_row(
            "SELECT side, status FROM guardian_links WHERE id = ?1",
            rusqlite::params![link_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((side.as_str(), status.as_str()), ("ward", "active"));

    // ── 5: child learns; activity pushes to the parent ───────────
    child
        .db
        .conn()
        .execute(
            "INSERT INTO courses (id, title, author_address) VALUES ('c1', 'Algebra I', 'stake_teacher')",
            [],
        )
        .unwrap();
    child
        .db
        .conn()
        .execute(
            "INSERT INTO enrollments (id, course_id, status) VALUES ('en1', 'c1', 'active')",
            [],
        )
        .unwrap();
    child
        .db
        .conn()
        .execute(
            "INSERT INTO course_chapters (id, course_id, title, position) VALUES ('ch1', 'c1', 'Ch', 0)",
            [],
        )
        .unwrap();
    child
        .db
        .conn()
        .execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
             VALUES ('el1', 'ch1', 'Lesson 1', 'text', 0)",
            [],
        )
        .unwrap();
    child
        .db
        .conn()
        .execute(
            "INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent) \
             VALUES ('p1', 'en1', 'el1', 'completed', 0.9, 300)",
            [],
        )
        .unwrap();

    let push = GuardianRequest::ActivityPush {
        link_id: link_id.clone(),
        sealed: seal(
            &key,
            &build_activity_snapshot(child.db.conn()).expect("child builds snapshot"),
        )
        .expect("child seals"),
    };
    let resp = handle_guardian_request(parent.db.conn(), CHILD_PEER, &push);
    let GuardianResponse::Merged { rows } = resp else {
        panic!("expected Merged, got {resp:?}");
    };
    assert!(
        rows >= 3,
        "enrollment + progress + course rows merged, got {rows}"
    );

    // The parent's mirror now shows the enrollment, the completed
    // element with its score, and the course title to render.
    let mirrored_progress: String = parent
        .db
        .conn()
        .query_row(
            "SELECT payload_json FROM guardian_activity_rows \
             WHERE link_id = ?1 AND table_name = 'element_progress' AND entity_id = 'p1'",
            rusqlite::params![link_id],
            |r| r.get(0),
        )
        .expect("progress row mirrored to parent");
    assert!(mirrored_progress.contains("\"completed\""));
    assert!(mirrored_progress.contains("0.9"));
    let mirrored_course: String = parent
        .db
        .conn()
        .query_row(
            "SELECT payload_json FROM guardian_activity_rows \
             WHERE link_id = ?1 AND table_name = 'courses' AND entity_id = 'c1'",
            rusqlite::params![link_id],
            |r| r.get(0),
        )
        .expect("course metadata mirrored to parent");
    assert!(mirrored_course.contains("Algebra I"));

    // ── 6: parent pulls on demand; child serves a sealed snapshot ─
    let pull = GuardianRequest::ActivityPull {
        link_id: link_id.clone(),
    };
    let resp = handle_guardian_request(child.db.conn(), PARENT_PEER, &pull);
    let GuardianResponse::Sealed { sealed } = resp else {
        panic!("expected Sealed, got {resp:?}");
    };
    let pulled: GuardianActivityPayload = open(&key, &sealed).expect("parent opens pull");
    let progress_rows = pulled
        .tables
        .iter()
        .find(|(t, _)| t == "element_progress")
        .map(|(_, rows)| rows.len())
        .unwrap_or(0);
    assert_eq!(progress_rows, 1, "pull carries the child's progress");

    // ── 7: a second guardian replaying the same invite is refused ─
    let mallory = person("parent", None, "active");
    let (_, replay_invite) = {
        // Rebuild the SAME invite bytes: code was consumed in step 3.
        let invite = invite_codec::GuardianInvite {
            child_did: child.did.clone(),
            child_stake_address: child.stake.clone(),
            child_peer_id: CHILD_PEER.into(),
            addresses: vec![],
            shared_key: key,
            display_name: None,
        };
        (invite_codec::encode(&invite).unwrap(), invite)
    };
    let replay_code = invite_codec::encode(&replay_invite).unwrap();
    let replay = parent_builds_link(&mallory, &replay_invite, &replay_code, "link-mallory");
    let resp = handle_guardian_request(child.db.conn(), "12D3KooWMallory", &replay);
    assert!(
        matches!(resp, GuardianResponse::Unauthorized),
        "replayed/unknown invite must be refused, got {resp:?}"
    );

    // ── 8: guardian revokes → still-minor child re-gates ─────────
    let revoke = GuardianRequest::Revoke {
        link_id: link_id.clone(),
        sealed_marker: seal(&key, &format!("revoke:{link_id}")).unwrap(),
    };
    let resp = handle_guardian_request(child.db.conn(), PARENT_PEER, &revoke);
    assert!(matches!(resp, GuardianResponse::Merged { .. }));
    assert_eq!(
        activation_state(child.db.conn()),
        "pending_guardian",
        "revoking a still-minor ward must re-gate the profile"
    );
}

#[test]
fn tampered_guardian_vc_never_activates_the_child() {
    let child = person("learner", Some("2012-03-15"), "pending_guardian");
    let parent = person("parent", None, "active");
    let (code, invite) = child_makes_invite(&child);
    let link_req = parent_builds_link(&parent, &invite, &code, "link-tampered");

    // Mallory intercepts and swaps the subject: the signature no
    // longer covers the payload, so verification must fail closed.
    let GuardianRequest::Link {
        code_hash,
        link_id,
        guardian_did,
        guardian_stake_address,
        guardian_display_name,
        guardian_vc_json,
    } = link_req
    else {
        unreachable!()
    };
    let tampered_json = guardian_vc_json.replace(&child.did, "did:key:zMalloryKid");
    assert_ne!(tampered_json, guardian_vc_json, "tamper must change bytes");
    let tampered = GuardianRequest::Link {
        code_hash,
        link_id,
        guardian_did,
        guardian_stake_address,
        guardian_display_name,
        guardian_vc_json: tampered_json,
    };

    let resp = handle_guardian_request(child.db.conn(), PARENT_PEER, &tampered);
    assert!(
        matches!(resp, GuardianResponse::Error(_)),
        "tampered VC must be rejected, got {resp:?}"
    );
    assert_eq!(
        activation_state(child.db.conn()),
        "pending_guardian",
        "child must stay gated after a tampered link attempt"
    );
}

#[test]
fn adult_ward_survives_guardian_revoke() {
    // Same lifecycle, but the ward is now over 18 (e.g. linked years
    // ago); a guardian-side revoke must NOT re-gate them.
    let (child, _parent, key, link_id) = establish_link("2000-01-01");
    assert_eq!(activation_state(child.db.conn()), "active");

    let revoke = GuardianRequest::Revoke {
        link_id: link_id.clone(),
        sealed_marker: seal(&key, &format!("revoke:{link_id}")).unwrap(),
    };
    let resp = handle_guardian_request(child.db.conn(), PARENT_PEER, &revoke);
    assert!(matches!(resp, GuardianResponse::Merged { .. }));
    assert_eq!(
        activation_state(child.db.conn()),
        "active",
        "an adult ward keeps their active profile after unlink"
    );
}
