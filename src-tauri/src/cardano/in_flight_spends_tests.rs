//! Features that spend one wallet, driven through their real ticks in one
//! background pass, against one journal and one in-memory ledger whose
//! mempool rejects conflicting spends and whose index lags.

use std::collections::HashSet;

use pallas_traverse::MultiEraTx;

use super::anchor_queue::{self, AnchorStatus, BatchPolicy};
use super::completion_queue::{self, WitnessStatus};
use super::completion_recovery::CompletionContext;
use super::fake_ledger::{ada, FakeLedger};
use super::test_chain::{self, TestProfile};
use crate::credentials::{issue::IssueCredentialRequest, Issuer};
use crate::domain::vc::{Claim, CredentialType, SkillClaim};

const TIP: u64 = 120_000_000;
const ISSUED: &str = "2026-11-10T00:00:00Z";
const PASS: &str = "2026-11-10T07:00:00Z";

/// Every `hash#index` the signed bytes spend or pledge.
fn spends(bytes: &[u8]) -> HashSet<String> {
    let tx = MultiEraTx::decode(bytes).unwrap();
    tx.inputs()
        .iter()
        .chain(tx.collateral().iter())
        .map(|input| format!("{}#{}", input.hash(), input.index()))
        .collect()
}

#[tokio::test]
async fn a_completion_mint_and_an_issuer_paid_anchor_in_one_pass_spend_different_utxos() {
    let profile = TestProfile::migrated();
    let wallet = test_chain::wallet();
    let ledger = FakeLedger::at_tip(TIP);
    ledger.set_utxos(
        &wallet.payment_address,
        vec![
            ada(0x11, 0, 40_000_000),
            ada(0x22, 0, 30_000_000),
            ada(0x33, 0, 20_000_000),
        ],
    );

    // A completion witness is requested and a credential is queued.
    let context = CompletionContext {
        version: 1,
        policy_id: super::script_refs::COMPLETION_MINTING_SCRIPT_HASH.to_owned(),
        course_id: "course".into(),
        subject_pubkey: *wallet.signing_key.verifying_key().as_bytes(),
        payment_key_hash: wallet.payment_key_hash,
        leaves: vec![[9; 32]],
        root: [9; 32],
        mean_score: 0.8,
        timestamp_ms: 1_714_000_000_000,
    };
    let issuer = Issuer::from_wallet(&wallet);
    let request = IssueCredentialRequest {
        credential_type: CredentialType::FormalCredential,
        subject: alexandria_verify::did::derive_did_key(&ed25519_dalek::SigningKey::from_bytes(
            &[9; 32],
        )),
        claim: Claim::Skill(SkillClaim {
            skill_id: "skill".into(),
            level: 2,
            score: 0.5,
            evidence_refs: vec![],
            rubric_version: None,
            assessment_method: None,
            provenance: None,
        }),
        evidence_refs: vec![],
        expiration_date: None,
        supersedes: None,
        integrity_session_id: None,
        integrity_policy: None,
    };
    let credential = profile.with_conn(|conn| {
        conn.execute(
            "INSERT INTO completion_claims (id, subject_did, course_id, completion_root, credential_ids_json)
             VALUES ('claim', 'subject', 'course', 'claim', '[]')",
            [],
        )
        .unwrap();
        completion_queue::enqueue(conn, "claim", &context).unwrap();
        crate::credentials::issue(conn, &issuer, &request, ISSUED).unwrap()
    });

    // One background pass, as the chain loop runs it: the completion queue,
    // then the anchor queue. The index still lists what the mint spends.
    let journal = profile.background();
    completion_queue::tick(&journal, &ledger, &wallet)
        .await
        .unwrap();
    anchor_queue::tick(
        &journal,
        Some(&ledger),
        Some(&wallet),
        None,
        &BatchPolicy::default(),
        PASS,
    )
    .await
    .unwrap();

    let submissions = ledger.submissions();
    assert_eq!(submissions.len(), 2);
    let (mint, anchor) = (spends(&submissions[0].1), spends(&submissions[1].1));
    assert!(mint.is_disjoint(&anchor), "{mint:?} and {anchor:?} overlap");
    assert_eq!(
        anchor,
        HashSet::from([format!("{}#0", "33".repeat(32))]),
        "the anchor pays from the UTxO the mint leaves free"
    );

    // Both were accepted by the mempool.
    profile.with_conn(|conn| {
        let witness = completion_queue::state(conn, "claim").unwrap();
        assert_eq!(witness.status, WitnessStatus::Submitted);
        assert_eq!(witness.tx_hash.as_deref(), Some(submissions[0].0.as_str()));
        let anchored = anchor_queue::state(conn, credential.id.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(anchored.status, AnchorStatus::Submitted);
        assert_eq!(anchored.tx_hash.as_deref(), Some(submissions[1].0.as_str()));
    });
}
