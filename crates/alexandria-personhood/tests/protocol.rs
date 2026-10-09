use alexandria_personhood::{decode, groth16, Groth16Proof};
#[test]
fn independently_verifies_native_proof_and_rejects_every_changed_signal() {
    let proof: Groth16Proof = decode(include_bytes!("fixtures/benchmark.proof.json")).unwrap();
    let signals: [String; 9] = decode(include_bytes!("fixtures/benchmark.public.json")).unwrap();
    groth16::verify(&proof, &signals).unwrap();
    for i in 0..9 {
        let mut changed = signals.clone();
        changed[i] = if changed[i] == "0" { "1" } else { "0" }.into();
        assert!(groth16::verify(&proof, &changed).is_err(), "signal {i}");
    }
}

use alexandria_personhood::{
    canonical, issue_challenge, local_verifier_key, signal_hash, store, verify_receipt,
    verify_submission, Policy, SignedChallenge, Submission, FIXTURE_TIMESTAMP,
};
use ed25519_dalek::SigningKey;

fn fixture() -> (SigningKey, SigningKey, Policy, SignedChallenge, Submission) {
    let account = SigningKey::from_bytes(&[1; 32]);
    let verifier = local_verifier_key(&account).unwrap();
    let policy = Policy::synthetic("test-network", &verifier.verifying_key());
    let challenge = issue_challenge(
        &policy,
        &account.verifying_key(),
        &verifier,
        [2; 32],
        [3; 32],
        FIXTURE_TIMESTAMP + 1,
    )
    .unwrap();
    let proof = decode(include_bytes!("fixtures/bound.proof.json")).unwrap();
    let signals = decode(include_bytes!("fixtures/bound.public.json")).unwrap();
    let submission = Submission::signed(challenge.clone(), proof, signals, &account).unwrap();
    (account, verifier, policy, challenge, submission)
}

#[test]
fn bound_proof_produces_signed_private_receipt() {
    let (a, v, p, c, s) = fixture();
    let vector: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/bound.vector.json")).unwrap();
    assert_eq!(signal_hash(&c.body).unwrap(), vector["signal_hash"]);
    assert_eq!(serde_json::to_value(&c).unwrap(), vector["challenge"]);
    let verified = verify_submission(&p, &c, s, &a.verifying_key(), FIXTURE_TIMESTAMP + 2).unwrap();
    let receipt = verified
        .receipt(&p, &a.verifying_key(), &v, FIXTURE_TIMESTAMP + 3)
        .unwrap();
    verify_receipt(&receipt, &v.verifying_key()).unwrap();
    assert_ne!(a.verifying_key(), v.verifying_key());
    assert!(verify_receipt(&receipt, &a.verifying_key()).is_err());
    let mut altered = receipt.clone();
    altered.body.subject_did.push('x');
    assert!(verify_receipt(&altered, &v.verifying_key()).is_err());
    let mut revoked = p.clone();
    revoked.issuer_revoked = true;
    assert!(verified
        .receipt(&revoked, &a.verifying_key(), &v, FIXTURE_TIMESTAMP + 3)
        .is_err());
    assert!(verified
        .receipt(&p, &a.verifying_key(), &v, c.body.expires_at)
        .is_err());
}

#[test]
fn rejects_cross_account_network_policy_session_nonce_and_purpose() {
    let (a, _, p, c, s) = fixture();
    let other = SigningKey::from_bytes(&[4; 32]);
    assert!(verify_submission(
        &p,
        &c,
        s.clone(),
        &other.verifying_key(),
        FIXTURE_TIMESTAMP + 2
    )
    .is_err());
    for field in [
        "network_id",
        "purpose",
        "session_nonce",
        "nonce",
        "subject_did",
        "subject_public_key",
        "policy_digest",
        "circuit_id",
        "verifier_public_key",
    ] {
        let mut json = serde_json::to_value(&c).unwrap();
        json["body"][field] = serde_json::json!("00".repeat(32));
        let changed: SignedChallenge = serde_json::from_value(json).unwrap();
        let submission = Submission::signed(
            changed.clone(),
            s.proof.clone(),
            s.public_signals.clone(),
            &a,
        )
        .unwrap();
        assert!(
            verify_submission(
                &p,
                &c,
                submission.clone(),
                &a.verifying_key(),
                FIXTURE_TIMESTAMP + 2
            )
            .is_err(),
            "{field} stored binding"
        );
        assert!(
            verify_submission(
                &p,
                &changed,
                submission,
                &a.verifying_key(),
                FIXTURE_TIMESTAMP + 2
            )
            .is_err(),
            "{field} signed binding"
        );
    }
    let wrong =
        Submission::signed(c.clone(), s.proof.clone(), s.public_signals.clone(), &other).unwrap();
    assert!(verify_submission(&p, &c, wrong, &a.verifying_key(), FIXTURE_TIMESTAMP + 2).is_err());
    for time in [FIXTURE_TIMESTAMP, c.body.expires_at] {
        assert!(verify_submission(&p, &c, s.clone(), &a.verifying_key(), time).is_err());
    }
    let mut p2 = p.clone();
    p2.issuer_revoked = true;
    assert!(verify_submission(
        &p2,
        &c,
        s.clone(),
        &a.verifying_key(),
        FIXTURE_TIMESTAMP + 2
    )
    .is_err());
    p2 = p.clone();
    p2.issuer_hash = "1".into();
    assert!(verify_submission(&p2, &c, s, &a.verifying_key(), FIXTURE_TIMESTAMP + 2).is_err());
}

#[test]
fn rejects_noncanonical_scalars_points_and_ambiguous_json() {
    let (_, _, _, _, s) = fixture();
    for value in [
        "01",
        "-1",
        "+1",
        "1e0",
        "",
        "21888242871839275222246405745257275088548364400416034343698204186575808495617",
    ] {
        assert!(groth16::scalar(value).is_err());
    }
    for slot in 0..3 {
        let mut proof = s.proof.clone();
        proof.pi_a[slot] = "0".into();
        assert!(groth16::verify(&proof, &s.public_signals).is_err());
    }
    let mut proof = s.proof.clone();
    proof.pi_b[0].swap(0, 1);
    assert!(groth16::verify(&proof, &s.public_signals).is_err());
    let mut value = serde_json::to_value(&s).unwrap();
    value["extra"] = serde_json::json!(true);
    assert!(decode::<Submission>(&canonical(&value).unwrap()).is_err());
    assert!(decode::<serde_json::Value>(br#"{"a":1,"a":2}"#).is_err());
    assert!(decode::<Submission>(&vec![b' '; 17000]).is_err());
}

#[test]
fn challenge_consumption_is_atomic_session_bound_and_cancellable() {
    let (a, v, p, c, s) = fixture();
    let db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch(store::SCHEMA).unwrap();
    store::prepare(&db, "session-a", &c).unwrap();
    assert!(store::pending(
        &db,
        "session-b",
        &c.body.nonce,
        &p,
        &a.verifying_key(),
        FIXTURE_TIMESTAMP + 2
    )
    .is_err());
    let verified = verify_submission(&p, &c, s, &a.verifying_key(), FIXTURE_TIMESTAMP + 2).unwrap();
    {
        let tx = db.unchecked_transaction().unwrap();
        store::consume(
            &tx,
            "session-a",
            &verified,
            &p,
            &a.verifying_key(),
            &v,
            FIXTURE_TIMESTAMP + 3,
        )
        .unwrap();
        // An interrupted commit rolls back both consumption and receipt.
    }
    assert!(store::receipts(&db, &c.body.subject_did, &p.network_id)
        .unwrap()
        .is_empty());
    store::pending(
        &db,
        "session-a",
        &c.body.nonce,
        &p,
        &a.verifying_key(),
        FIXTURE_TIMESTAMP + 3,
    )
    .unwrap();
    let tx = db.unchecked_transaction().unwrap();
    store::consume(
        &tx,
        "session-a",
        &verified,
        &p,
        &a.verifying_key(),
        &v,
        FIXTURE_TIMESTAMP + 3,
    )
    .unwrap();
    tx.commit().unwrap();
    let tx = db.unchecked_transaction().unwrap();
    let replay = store::consume(
        &tx,
        "session-a",
        &verified,
        &p,
        &a.verifying_key(),
        &v,
        FIXTURE_TIMESTAMP + 3,
    )
    .unwrap();
    assert_eq!(replay.body.id, c.body.nonce);
    drop(tx);
    store::cancel(&db, "session-a", &c.body.nonce).unwrap();
    assert_eq!(
        store::receipts(&db, &c.body.subject_did, &p.network_id)
            .unwrap()
            .len(),
        1
    );
    assert!(store::receipts(&db, "different-account", &p.network_id)
        .unwrap()
        .is_empty());
    assert!(
        store::receipts(&db, &c.body.subject_did, "different-network")
            .unwrap()
            .is_empty()
    );
    let next = issue_challenge(
        &p,
        &a.verifying_key(),
        &v,
        [2; 32],
        [5; 32],
        FIXTURE_TIMESTAMP + 4,
    )
    .unwrap();
    store::prepare(&db, "session-a", &next).unwrap();
    store::cancel(&db, "session-a", &next.body.nonce).unwrap();
    assert!(store::pending(
        &db,
        "session-a",
        &next.body.nonce,
        &p,
        &a.verifying_key(),
        FIXTURE_TIMESTAMP + 5
    )
    .is_err());
}

#[test]
fn rejects_stale_synthetic_document_before_issuing_a_receipt() {
    let (a, v, mut p, _, s) = fixture();
    p.max_document_age_seconds = 1;
    let c = issue_challenge(
        &p,
        &a.verifying_key(),
        &v,
        [2; 32],
        [3; 32],
        FIXTURE_TIMESTAMP + 1,
    )
    .unwrap();
    let mut signals = s.public_signals;
    signals[8] = signal_hash(&c.body).unwrap();
    let submission = Submission::signed(c.clone(), s.proof, signals, &a).unwrap();
    assert!(matches!(
        verify_submission(
            &p,
            &c,
            submission,
            &a.verifying_key(),
            FIXTURE_TIMESTAMP + 2
        ),
        Err(alexandria_personhood::Error::Expired)
    ));
}

#[test]
fn concurrent_duplicate_submissions_store_one_receipt() {
    use std::sync::{Arc, Barrier};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("receipts.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(store::SCHEMA).unwrap();
    let (a, v, p, c, s) = fixture();
    store::prepare(&db, "session", &c).unwrap();
    let verified =
        Arc::new(verify_submission(&p, &c, s, &a.verifying_key(), FIXTURE_TIMESTAMP + 2).unwrap());
    let barrier = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let (path, verified, barrier, a, v, p) = (
            path.clone(),
            verified.clone(),
            barrier.clone(),
            a.clone(),
            v.clone(),
            p.clone(),
        );
        workers.push(std::thread::spawn(move || {
            let mut db = rusqlite::Connection::open(path).unwrap();
            db.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
            barrier.wait();
            let tx = db
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .unwrap();
            let receipt = store::consume(
                &tx,
                "session",
                &verified,
                &p,
                &a.verifying_key(),
                &v,
                FIXTURE_TIMESTAMP + 3,
            )
            .unwrap();
            tx.commit().unwrap();
            receipt
        }));
    }
    let receipts: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(receipts[0], receipts[1]);
    assert_eq!(
        store::receipts(&db, &c.body.subject_did, &p.network_id)
            .unwrap()
            .len(),
        1
    );
}
