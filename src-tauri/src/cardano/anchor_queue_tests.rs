//! The batch anchorer through its public functions, against an in-memory
//! ledger (spec: sponsored, batched anchoring, testing seam 2).

use pallas_primitives::Metadatum;
use pallas_traverse::MultiEraTx;

use super::*;
use crate::cardano::fake_ledger::{ada, FakeLedger};
use crate::cardano::submission::EXPIRY_MARGIN_SLOTS;
use crate::cardano::test_chain::{self, TestProfile};
use crate::cardano::types::UTxO;
use crate::credentials::{issue::IssueCredentialRequest, Issuer};
use crate::crypto::wallet::Wallet;
use crate::domain::vc::{
    verify_credential_db, Claim, CredentialType, SkillClaim, VerifiableCredential,
    VerificationPolicy,
};
use crate::network_profile::AnchorSponsorProfile;

const T0: &str = "2026-11-10T00:00:00Z";
const T0_PLUS_1H: &str = "2026-11-10T01:00:00Z";
const T0_PLUS_7H: &str = "2026-11-10T07:00:00Z";
const TIP: u64 = 120_000_000;

fn issue(profile: &TestProfile, issuer: &Issuer, skill: &str) -> VerifiableCredential {
    let req = IssueCredentialRequest {
        credential_type: CredentialType::FormalCredential,
        subject: alexandria_verify::did::derive_did_key(&ed25519_dalek::SigningKey::from_bytes(
            &[9; 32],
        )),
        claim: Claim::Skill(SkillClaim {
            skill_id: skill.into(),
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
    profile.with_conn(|conn| crate::credentials::issue(conn, issuer, &req, T0).unwrap())
}

/// Store a credential received from elsewhere, as an import would.
fn store_received(conn: &rusqlite::Connection, vc: &VerifiableCredential) {
    conn.execute(
        "INSERT INTO credentials (id, issuer_did, subject_did, credential_type, claim_kind, \
         issuance_date, signed_vc_json, integrity_hash) \
         VALUES (?1, ?2, ?3, 'FormalCredential', 'skill', ?4, ?5, ?6)",
        rusqlite::params![
            vc.id,
            vc.issuer.as_str(),
            vc.credential_subject.id.as_str(),
            vc.valid_from,
            serde_json::to_string(vc).unwrap(),
            crate::domain::vc::integrity_hash_of(vc).unwrap(),
        ],
    )
    .unwrap();
}

fn funded(ledger: &FakeLedger, wallet: &Wallet) {
    ledger.set_utxos(&wallet.payment_address, vec![ada(0x11, 0, 20_000_000)]);
}

fn sponsor() -> AnchorSponsorProfile {
    AnchorSponsorProfile {
        script_hash: "5a".repeat(28),
        address: script_address(&"5a".repeat(28)),
        reference_input: format!("{}#0", "7e".repeat(32)),
        fee_cap_lovelace: 2_000_000,
        window_ms: 3_600_000,
        max_validity_ms: 600_000,
        // Preprod: Shelley slot 86 400 began at 2022-06-21T00:00:00Z.
        slot_offset_secs: 1_655_683_200,
    }
}

fn script_address(script_hash: &str) -> String {
    use pallas_addresses::{
        Address, Network, ShelleyAddress, ShelleyDelegationPart, ShelleyPaymentPart,
    };
    let hash: [u8; 28] = hex::decode(script_hash).unwrap().try_into().unwrap();
    Address::from(ShelleyAddress::new(
        Network::Testnet,
        ShelleyPaymentPart::script_hash(hash.into()),
        ShelleyDelegationPart::Null,
    ))
    .to_bech32()
    .unwrap()
}

/// A sponsor UTxO whose `next_allowed` is `next_allowed_ms`.
fn sponsor_utxo(byte: u8, next_allowed_ms: u64) -> UTxO {
    let mut utxo = ada(byte, 0, 50_000_000);
    let datum = crate::cardano::plutus_data::encode_sponsor_datum(next_allowed_ms as i64).unwrap();
    utxo.inline_datum = Some(hex::encode(datum));
    utxo
}

struct Node {
    profile: TestProfile,
    ledger: FakeLedger,
    wallet: Wallet,
    issuer: Issuer,
    sponsor: Option<AnchorSponsorProfile>,
    policy: BatchPolicy,
}

impl Node {
    fn new() -> Self {
        let wallet = test_chain::wallet();
        let issuer = Issuer::from_wallet(&wallet);
        Self {
            profile: TestProfile::migrated(),
            ledger: FakeLedger::at_tip(TIP),
            wallet,
            issuer,
            sponsor: None,
            policy: BatchPolicy::default(),
        }
    }

    async fn tick(&self, now: &str) -> u32 {
        tick(
            &self.profile.background(),
            Some(&self.ledger),
            Some(&self.wallet),
            self.sponsor.as_ref(),
            &self.policy,
            now,
        )
        .await
        .unwrap()
    }

    fn state(&self, vc: &VerifiableCredential) -> CredentialAnchorState {
        self.profile
            .with_conn(|conn| state(conn, vc.id.as_deref().unwrap()))
            .unwrap()
            .unwrap()
    }

    fn anchored(&self, vc: &VerifiableCredential) -> bool {
        self.profile.with_conn(|conn| {
            verify_credential_db(conn, vc, T0_PLUS_7H, &VerificationPolicy::default())
                .integrity_anchored
        })
    }
}

/// Decoded fields of a submitted anchor transaction.
struct Submitted {
    hash: String,
    inputs: Vec<(String, u64)>,
    collateral: Vec<(String, u64)>,
    signers: Vec<String>,
    root: String,
    leaf_count: i64,
    continuing_datum: Option<Vec<u8>>,
    valid_from: Option<u64>,
}

fn decode(hash: &str, bytes: &[u8]) -> Submitted {
    let tx = MultiEraTx::decode(bytes).unwrap();
    let Some(Metadatum::Map(fields)) = tx.metadata().find(1697).cloned() else {
        panic!("no anchor metadata");
    };
    let field = |name: &str| {
        fields
            .iter()
            .find(|(k, _)| matches!(k, Metadatum::Text(k) if k == name))
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    let Metadatum::Text(root) = field("root") else {
        panic!()
    };
    let Metadatum::Int(count) = field("leaf_count") else {
        panic!()
    };
    let refs = |inputs: Vec<pallas_traverse::MultiEraInput<'_>>| {
        inputs
            .iter()
            .map(|i| (i.hash().to_string(), i.index()))
            .collect::<Vec<_>>()
    };
    let continuing_datum = tx.outputs().first().and_then(|o| match o.datum() {
        Some(pallas_primitives::conway::PseudoDatumOption::Data(d)) => {
            Some(d.0.raw_cbor().to_vec())
        }
        _ => None,
    });
    Submitted {
        hash: hash.to_owned(),
        inputs: refs(tx.inputs()),
        collateral: refs(tx.collateral()),
        signers: tx
            .required_signers()
            .as_alonzo()
            .map(|s| s.iter().map(|h| h.to_string()).collect())
            .unwrap_or_default(),
        root,
        leaf_count: i64::try_from(i128::from(count)).unwrap(),
        continuing_datum,
        valid_from: tx.validity_start(),
    }
}

fn only_submission(node: &Node) -> Submitted {
    let submissions = node.ledger.submissions();
    assert_eq!(submissions.len(), 1, "exactly one anchor transaction");
    decode(&submissions[0].0, &submissions[0].1)
}

#[tokio::test]
async fn credentials_wait_for_the_batch_window_then_anchor_in_one_transaction() {
    let node = Node::new();
    funded(&node.ledger, &node.wallet);
    let vcs: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|s| issue(&node.profile, &node.issuer, s))
        .collect();

    node.tick(T0_PLUS_1H).await;
    assert!(node.ledger.submissions().is_empty(), "inside the window");
    assert_eq!(node.state(&vcs[0]).status, AnchorStatus::Pending);

    node.tick(T0_PLUS_7H).await;
    let tx = only_submission(&node);
    assert_eq!(tx.leaf_count, 3);
    assert_eq!(tx.inputs, vec![("11".repeat(32), 0)], "the wallet paid");
    assert_eq!(
        tx.signers,
        vec![crate::cardano::anchor_tx::did_key_hash(&node.wallet).to_string()]
    );
    for vc in &vcs {
        let state = node.state(vc);
        assert_eq!(state.status, AnchorStatus::Submitted);
        assert_eq!(state.tx_hash.as_deref(), Some(tx.hash.as_str()));
        assert!(!node.anchored(vc), "submitted is not anchored");
    }
}

#[tokio::test]
async fn a_full_batch_flushes_before_the_window() {
    let mut node = Node::new();
    node.policy.cap = 2;
    funded(&node.ledger, &node.wallet);
    let vcs: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|s| issue(&node.profile, &node.issuer, s))
        .collect();
    node.tick(T0_PLUS_1H).await;
    let tx = only_submission(&node);
    assert_eq!(tx.leaf_count, 2);
    let statuses: Vec<_> = vcs.iter().map(|vc| node.state(vc).status).collect();
    let count = |s: AnchorStatus| statuses.iter().filter(|x| **x == s).count();
    assert_eq!(count(AnchorStatus::Submitted), 2);
    assert_eq!(count(AnchorStatus::Pending), 1);
}

#[tokio::test]
async fn confirmation_stores_receipts_the_verifier_accepts() {
    let node = Node::new();
    funded(&node.ledger, &node.wallet);
    let a = issue(&node.profile, &node.issuer, "a");
    let b = issue(&node.profile, &node.issuer, "b");
    node.tick(T0_PLUS_7H).await;
    let tx = only_submission(&node);
    node.ledger.include(&tx.hash, TIP + 20);
    node.tick(T0_PLUS_7H).await;

    for vc in [&a, &b] {
        let state = node.state(vc);
        assert_eq!(state.status, AnchorStatus::Confirmed);
        assert!(state.confirmed_at.is_some());
        assert!(node.anchored(vc));
        let receipt = node
            .profile
            .with_conn(|conn| receipt_for(conn, vc).unwrap())
            .unwrap();
        assert_eq!(receipt.root, tx.root);
        assert_eq!(receipt.tx_hash, tx.hash);
    }
    // The anchor names the stored content: an altered copy is not anchored.
    let mut altered = a.clone();
    altered.valid_from = "2026-11-11T00:00:00Z".into();
    assert!(!node.anchored(&altered));
}

#[tokio::test]
async fn a_spendable_sponsor_pays_and_the_issuer_signs_and_lends_collateral() {
    let mut node = Node::new();
    let profile = sponsor();
    node.sponsor = Some(profile.clone());
    funded(&node.ledger, &node.wallet);
    let now_ms = profile.slot_to_posix_ms(TIP);
    node.ledger.set_utxos(
        &profile.address,
        vec![
            sponsor_utxo(0x21, now_ms + 60_000), // rate-limited
            sponsor_utxo(0x22, now_ms - 1),      // spendable
        ],
    );
    let vc = issue(&node.profile, &node.issuer, "a");
    node.tick(T0_PLUS_7H).await;

    let tx = only_submission(&node);
    assert_eq!(
        tx.inputs,
        vec![("22".repeat(32), 0)],
        "only the sponsor UTxO"
    );
    assert_eq!(tx.collateral, vec![("11".repeat(32), 0)]);
    assert_eq!(
        tx.signers,
        vec![crate::cardano::anchor_tx::did_key_hash(&node.wallet).to_string()]
    );
    assert_eq!(tx.valid_from, Some(TIP));
    let next_allowed = crate::cardano::plutus_data::decode_sponsor_datum(
        &tx.continuing_datum.expect("continuing datum"),
    );
    assert_eq!(next_allowed, Some((now_ms + profile.window_ms) as i64));
    assert!(node.ledger.evaluations() > 0, "fee from evaluated units");
    assert_eq!(node.state(&vc).status, AnchorStatus::Submitted);
}

#[tokio::test]
async fn without_a_spendable_sponsor_the_issuers_wallet_pays() {
    let mut node = Node::new();
    let profile = sponsor();
    node.sponsor = Some(profile.clone());
    funded(&node.ledger, &node.wallet);
    let now_ms = profile.slot_to_posix_ms(TIP);
    node.ledger
        .set_utxos(&profile.address, vec![sponsor_utxo(0x21, now_ms + 60_000)]);
    issue(&node.profile, &node.issuer, "a");
    node.tick(T0_PLUS_7H).await;
    let tx = only_submission(&node);
    assert_eq!(tx.inputs, vec![("11".repeat(32), 0)]);
    assert!(tx.collateral.is_empty());
}

#[tokio::test]
async fn with_no_way_to_pay_the_credential_stays_queued_with_the_reason() {
    let mut node = Node::new();
    node.sponsor = Some(sponsor());
    let vc = issue(&node.profile, &node.issuer, "a");
    node.tick(T0_PLUS_7H).await;
    let state = node.state(&vc);
    assert_eq!(state.status, AnchorStatus::Pending);
    assert_eq!(state.attempts, 0, "nothing reached the ledger");
    assert!(state.needs_funds);
    assert!(state.last_error.unwrap().contains("lovelace"));
    assert!(node.ledger.submissions().is_empty());

    // Funded later, it anchors on the next try without the user's help.
    funded(&node.ledger, &node.wallet);
    node.tick(T0_PLUS_7H).await;
    assert!(
        node.ledger.submissions().is_empty(),
        "waits out the retry delay"
    );
    node.tick("2026-11-10T07:31:00Z").await;
    let state = node.state(&vc);
    assert_eq!(state.status, AnchorStatus::Submitted);
    assert!(!state.needs_funds);
}

#[tokio::test]
async fn batches_that_fail_on_chain_end_in_failure_until_retried() {
    let node = Node::new();
    funded(&node.ledger, &node.wallet);
    let vc = issue(&node.profile, &node.issuer, "a");
    let mut now = chrono::DateTime::parse_from_rfc3339(T0_PLUS_7H).unwrap();
    // A failed transaction keeps its input reserved until its window passes,
    // so each attempt comes one window after the last.
    let window = crate::cardano::tx_builder::TTL_OFFSET + EXPIRY_MARGIN_SLOTS + 1;
    let tip_of = |attempt: u32| TIP + window * u64::from(attempt);
    for attempt in 1..=MAX_ATTEMPTS {
        node.ledger.set_tip(tip_of(attempt));
        node.tick(&now.to_rfc3339()).await;
        let submissions = node.ledger.submissions();
        let (hash, _) = submissions.last().unwrap();
        node.ledger.include_failed(hash, tip_of(attempt) + 5);
        node.tick(&now.to_rfc3339()).await;
        let state = node.state(&vc);
        assert_eq!(state.attempts, attempt);
        assert!(!state.needs_funds);
        let expected = if attempt < MAX_ATTEMPTS {
            AnchorStatus::Pending
        } else {
            AnchorStatus::Failed
        };
        assert_eq!(state.status, expected);
        now += chrono::Duration::hours(2); // past any backoff
    }
    assert_eq!(node.ledger.submissions().len(), MAX_ATTEMPTS as usize);

    // Retrying a failure starts over.
    node.profile
        .with_conn(|conn| enqueue_or_retry(conn, vc.id.as_deref().unwrap()))
        .unwrap();
    let state = node.state(&vc);
    assert_eq!((state.status, state.attempts), (AnchorStatus::Pending, 0));
    node.tick(&now.to_rfc3339()).await;
    let state = node.state(&vc);
    assert_eq!(
        (state.status, state.attempts),
        (AnchorStatus::Pending, 0),
        "the last failure's input is still reserved: wait"
    );
    node.ledger.set_tip(tip_of(MAX_ATTEMPTS + 1));
    now += chrono::Duration::hours(1);
    node.tick(&now.to_rfc3339()).await;
    assert_eq!(node.state(&vc).status, AnchorStatus::Submitted);
}

#[tokio::test]
async fn an_unacknowledged_batch_is_reconciled_and_never_rebuilt() {
    let node = Node::new();
    funded(&node.ledger, &node.wallet);
    let vc = issue(&node.profile, &node.issuer, "a");
    node.ledger.reject_submissions("connection reset");
    node.tick(T0_PLUS_7H).await;
    let tx = only_submission(&node);
    assert_eq!(node.state(&vc).status, AnchorStatus::OutcomeUnknown);

    node.tick(T0_PLUS_7H).await;
    assert_eq!(node.ledger.submissions().len(), 1, "no rebuild, no resend");
    node.ledger.include(&tx.hash, TIP + 5);
    node.tick(T0_PLUS_7H).await;
    assert_eq!(node.state(&vc).status, AnchorStatus::Confirmed);
    assert!(node.anchored(&vc));
}

#[tokio::test]
async fn an_expired_batch_releases_its_credentials_into_a_new_batch() {
    let node = Node::new();
    funded(&node.ledger, &node.wallet);
    let vc = issue(&node.profile, &node.issuer, "a");
    node.ledger.reject_submissions("bad inputs");
    node.tick(T0_PLUS_7H).await;
    let first = only_submission(&node).hash;

    // Still inside its validity interval: wait.
    node.ledger.set_tip(TIP + 60);
    node.tick(T0_PLUS_7H).await;
    assert_eq!(node.ledger.submissions().len(), 1);

    // Long past it and never included: the journal proves it can no longer
    // land, and the batch is released by that status.
    node.ledger
        .set_tip(TIP + crate::cardano::tx_builder::TTL_OFFSET + EXPIRY_MARGIN_SLOTS + 1);
    node.tick(T0_PLUS_7H).await;
    let state = node.state(&vc);
    assert_eq!(state.status, AnchorStatus::Pending);
    assert_eq!(state.tx_hash, None);
    assert_eq!(state.attempts, 1);
    let journaled = node.profile.with_conn(|conn| {
        let operation = Operation {
            kind: BATCH_KIND,
            id: &first,
        };
        (
            submission::lookup(conn, operation).unwrap().unwrap().status,
            submission::unapplied_operations(conn, BATCH_KIND, 10).unwrap(),
        )
    });
    assert_eq!(
        journaled,
        (SubmissionStatus::Expired, vec![]),
        "expired in the journal, and the release finished its projection"
    );

    let ledger_ok = FakeLedger::at_tip(TIP + 10_000);
    funded(&ledger_ok, &node.wallet);
    // After the release's backoff.
    tick(
        &node.profile.background(),
        Some(&ledger_ok),
        Some(&node.wallet),
        None,
        &node.policy,
        "2026-11-10T08:00:00Z",
    )
    .await
    .unwrap();
    let second = &ledger_ok.submissions()[0].0;
    assert_ne!(&first, second);
    assert_eq!(node.state(&vc).tx_hash.as_deref(), Some(second.as_str()));
}

#[tokio::test]
async fn a_script_failure_on_chain_releases_the_batch() {
    let node = Node::new();
    funded(&node.ledger, &node.wallet);
    let vc = issue(&node.profile, &node.issuer, "a");
    node.tick(T0_PLUS_7H).await;
    let tx = only_submission(&node);
    node.ledger.include_failed(&tx.hash, TIP + 5);
    node.tick(T0_PLUS_7H).await;
    let state = node.state(&vc);
    assert_eq!(state.status, AnchorStatus::Pending);
    assert!(state.last_error.unwrap().contains("failed on chain"));
    assert!(!node.anchored(&vc));
}

#[tokio::test]
async fn credentials_of_another_issuer_are_never_anchored() {
    let node = Node::new();
    funded(&node.ledger, &node.wallet);
    let other = Issuer::from_key(ed25519_dalek::SigningKey::from_bytes(&[5; 32]));
    let vc = issue(&node.profile, &other, "a");
    node.tick(T0_PLUS_7H).await;
    assert!(node.ledger.submissions().is_empty());
    assert_eq!(node.state(&vc).status, AnchorStatus::Pending);
}

#[tokio::test]
async fn an_idle_node_without_a_ledger_does_nothing() {
    let profile = TestProfile::migrated();
    let processed = tick::<FakeLedger>(
        &profile.background(),
        None,
        None,
        None,
        &BatchPolicy::default(),
        T0,
    )
    .await
    .unwrap();
    assert_eq!(processed, 0);
}

#[tokio::test]
async fn an_imported_receipt_is_observed_on_the_ledger_and_then_verifies() {
    // The issuer's node anchors and confirms.
    let issuer_node = Node::new();
    funded(&issuer_node.ledger, &issuer_node.wallet);
    let vc = issue(&issuer_node.profile, &issuer_node.issuer, "a");
    issuer_node.tick(T0_PLUS_7H).await;
    let tx = only_submission(&issuer_node);
    issuer_node.ledger.include(&tx.hash, TIP + 5);
    issuer_node.tick(T0_PLUS_7H).await;
    let receipt = issuer_node
        .profile
        .with_conn(|conn| receipt_for(conn, &vc).unwrap())
        .unwrap();

    // A holder's node imports the credential and its receipt, then looks
    // the transaction up on the same ledger.
    let holder = TestProfile::migrated();
    holder.with_conn(|conn| {
        store_received(conn, &vc);
        // A made-up receipt arrives first, and more receipts naming
        // transactions the ledger never shows than one pass looks up.
        let mut forged = receipt.clone();
        forged.tx_hash = "ee".repeat(32);
        import_receipt(conn, &vc, &forged).unwrap();
        for n in 0..(OBSERVATIONS_PER_TICK as u8) {
            let mut stray = receipt.clone();
            stray.tx_hash = format!("{n:02x}").repeat(32);
            import_receipt(conn, &vc, &stray).unwrap();
        }
        import_receipt(conn, &vc, &receipt).unwrap();
        assert!(!is_queued(conn, vc.id.as_deref().unwrap()).unwrap());
    });
    let anchored = |profile: &TestProfile| {
        profile.with_conn(|conn| {
            verify_credential_db(conn, &vc, T0_PLUS_7H, &VerificationPolicy::default())
                .integrity_anchored
        })
    };
    assert!(!anchored(&holder), "not until the ledger is consulted");
    for _ in 0..2 {
        // The first pass finds nothing for the stray receipts and puts them
        // back; the second reaches the real one.
        tick(
            &holder.background(),
            Some(&issuer_node.ledger),
            None,
            None,
            &BatchPolicy::default(),
            T0_PLUS_7H,
        )
        .await
        .unwrap();
    }
    assert!(anchored(&holder));
    assert_eq!(
        holder
            .with_conn(|conn| receipt_for(conn, &vc).unwrap())
            .unwrap(),
        receipt,
        "the verified receipt is the one handed out"
    );
    let state = holder
        .with_conn(|conn| state(conn, vc.id.as_deref().unwrap()))
        .unwrap()
        .unwrap();
    assert_eq!(state.status, AnchorStatus::Confirmed);
    assert_eq!(state.tx_hash.as_deref(), Some(tx.hash.as_str()));
}

// -- in-flight spends -----------------------------------------------------------

/// Another feature's transaction (a completion mint, say) journaled and sent
/// in an earlier pass: spends `inputs`, pledges `collateral`, can land until
/// `ttl`. Returns its hash.
async fn in_flight_spend(node: &Node, inputs: &[&UTxO], collateral: &[&UTxO], ttl: u64) -> String {
    use crate::cardano::tx_builder;
    use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
    let input = |u: &UTxO| Input::new(tx_builder::parse_tx_hash(&u.tx_hash).unwrap(), u.tx_index);
    let mut staging = StagingTransaction::new()
        .output(Output::new(
            pallas_addresses::Address::from_bech32(&node.wallet.payment_address).unwrap(),
            2_000_000,
        ))
        .fee(200_000)
        .invalid_from_slot(ttl)
        .network_id(0);
    for u in inputs {
        staging = staging.input(input(u));
    }
    for u in collateral {
        staging = staging.collateral_input(input(u));
    }
    let built = staging.build_conway_raw().unwrap();
    let key = tx_builder::extended_private_key(&node.wallet.payment_key_extended).unwrap();
    let signed = tx_builder::sign_raw_tx(&built.tx_bytes.0, &key).unwrap();
    let hash = tx_builder::compute_tx_hash(&signed).unwrap();
    let sent = submission::submit_once(
        &node.profile.background(),
        &node.ledger,
        Operation {
            kind: "completion_mint",
            id: &hash,
        },
        &signed,
        "{}",
    )
    .await
    .unwrap();
    assert_eq!(
        sent.status,
        SubmissionStatus::Submitted,
        "the mempool took it"
    );
    hash
}

/// Anchor transactions submitted so far, oldest first; other features'
/// transactions are skipped.
fn anchor_submissions(node: &Node) -> Vec<Submitted> {
    node.ledger
        .submissions()
        .iter()
        .filter(|(_, bytes)| {
            MultiEraTx::decode(bytes)
                .unwrap()
                .metadata()
                .find(1697)
                .is_some()
        })
        .map(|(hash, bytes)| decode(hash, bytes))
        .collect()
}

#[tokio::test]
async fn an_issuer_paid_anchor_spends_a_utxo_no_in_flight_transaction_spends() {
    let node = Node::new();
    let largest = ada(0x11, 0, 50_000_000);
    let free = ada(0x12, 0, 20_000_000);
    node.ledger.set_utxos(
        &node.wallet.payment_address,
        vec![largest.clone(), free.clone()],
    );
    // A completion mint earlier in the pass took the largest UTxO; the
    // provider still lists it.
    in_flight_spend(&node, &[&largest], &[], TIP + 3_600).await;
    let vc = issue(&node.profile, &node.issuer, "a");
    node.tick(T0_PLUS_7H).await;

    let anchors = anchor_submissions(&node);
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].inputs, vec![(free.tx_hash.clone(), 0)]);
    assert_eq!(node.state(&vc).status, AnchorStatus::Submitted, "accepted");
}

#[tokio::test]
async fn a_sponsored_anchors_collateral_avoids_an_outpoint_a_pending_mint_spends() {
    let mut node = Node::new();
    let profile = sponsor();
    node.sponsor = Some(profile.clone());
    let smallest = ada(0x13, 0, 6_000_000);
    let free = ada(0x14, 0, 20_000_000);
    node.ledger.set_utxos(
        &node.wallet.payment_address,
        vec![smallest.clone(), free.clone()],
    );
    node.ledger.set_utxos(
        &profile.address,
        vec![sponsor_utxo(0x22, profile.slot_to_posix_ms(TIP) - 1)],
    );
    in_flight_spend(&node, &[&smallest], &[], TIP + 3_600).await;
    let vc = issue(&node.profile, &node.issuer, "a");
    node.tick(T0_PLUS_7H).await;

    let anchors = anchor_submissions(&node);
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].inputs, vec![("22".repeat(32), 0)], "sponsored");
    assert_eq!(anchors[0].collateral, vec![(free.tx_hash.clone(), 0)]);
    assert_eq!(node.state(&vc).status, AnchorStatus::Submitted, "accepted");
}

#[tokio::test]
async fn the_sponsor_pool_pick_skips_a_sponsor_utxo_this_nodes_pending_anchor_spends() {
    let mut node = Node::new();
    node.policy.cap = 1;
    let profile = sponsor();
    node.sponsor = Some(profile.clone());
    node.ledger.set_utxos(
        &node.wallet.payment_address,
        vec![
            ada(0x11, 0, 6_000_000),
            ada(0x12, 0, 7_000_000),
            ada(0x13, 0, 8_000_000),
        ],
    );
    let spendable = profile.slot_to_posix_ms(TIP) - 1;
    node.ledger.set_utxos(
        &profile.address,
        vec![sponsor_utxo(0x21, spendable), sponsor_utxo(0x22, spendable)],
    );
    let a = issue(&node.profile, &node.issuer, "a");
    let b = issue(&node.profile, &node.issuer, "b");
    // Two batches at the same tip: the issuer-and-tip hash alone would pick
    // the same sponsor UTxO twice.
    node.tick(T0_PLUS_7H).await;
    node.tick(T0_PLUS_7H).await;

    let anchors = anchor_submissions(&node);
    assert_eq!(anchors.len(), 2);
    let mut sponsor_inputs: Vec<_> = anchors.iter().flat_map(|tx| tx.inputs.clone()).collect();
    sponsor_inputs.sort();
    assert_eq!(
        sponsor_inputs,
        vec![("21".repeat(32), 0), ("22".repeat(32), 0)]
    );
    for vc in [&a, &b] {
        assert_eq!(node.state(vc).status, AnchorStatus::Submitted, "accepted");
    }
}

#[tokio::test]
async fn with_every_eligible_utxo_reserved_the_anchor_waits_without_spending_an_attempt() {
    let mut node = Node::new();
    let profile = sponsor();
    node.sponsor = Some(profile.clone());
    let only = ada(0x11, 0, 20_000_000);
    node.ledger
        .set_utxos(&node.wallet.payment_address, vec![only.clone()]);
    node.ledger.set_utxos(
        &profile.address,
        vec![sponsor_utxo(0x22, profile.slot_to_posix_ms(TIP) - 1)],
    );
    let mint_ttl = TIP + 3_600;
    let mint = in_flight_spend(&node, &[&only], &[], mint_ttl).await;
    let vc = issue(&node.profile, &node.issuer, "a");
    node.tick(T0_PLUS_7H).await;

    // Neither collateral for the sponsor nor an issuer-paid input is free.
    assert!(anchor_submissions(&node).is_empty(), "nothing sent");
    let state = node.state(&vc);
    assert_eq!((state.status, state.attempts), (AnchorStatus::Pending, 0));
    assert!(!state.needs_funds, "the wallet is busy, not short");
    assert_eq!(
        state.last_error.as_deref(),
        Some("wallet busy: inputs reserved by in-flight transactions")
    );

    // The mint never lands; once its window passes, the next pass after the
    // retry delay anchors.
    node.ledger.drop_tx(&mint);
    node.ledger.set_tip(mint_ttl + EXPIRY_MARGIN_SLOTS + 1);
    node.tick("2026-11-10T07:31:00Z").await;
    assert_eq!(anchor_submissions(&node).len(), 1);
    let state = node.state(&vc);
    assert_eq!((state.status, state.attempts), (AnchorStatus::Submitted, 0));
    assert_eq!(state.last_error, None);
}

/// A node whose sponsor pool holds one spendable UTxO, anchoring one
/// credential per batch.
fn one_sponsor_utxo_node() -> Node {
    let mut node = Node::new();
    node.policy.cap = 1;
    let profile = sponsor();
    node.ledger.set_utxos(
        &profile.address,
        vec![sponsor_utxo(0x21, profile.slot_to_posix_ms(TIP) - 1)],
    );
    node.sponsor = Some(profile);
    node
}

#[tokio::test]
async fn an_anchor_waits_while_its_own_in_flight_anchor_spends_the_only_sponsor_utxo() {
    let node = one_sponsor_utxo_node();
    node.ledger.set_utxos(
        &node.wallet.payment_address,
        vec![
            ada(0x11, 0, 6_000_000),
            ada(0x12, 0, 7_000_000),
            ada(0x13, 0, 20_000_000),
        ],
    );
    let a = issue(&node.profile, &node.issuer, "a");
    let b = issue(&node.profile, &node.issuer, "b");
    node.tick(T0_PLUS_7H).await;
    node.tick(T0_PLUS_7H).await;

    // The sponsor is only busy with this node's own anchor: the issuer's
    // wallet does not pay for the second batch.
    let anchors = anchor_submissions(&node);
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].inputs, vec![("21".repeat(32), 0)], "sponsored");
    let (first, waiting) = if node.state(&a).status == AnchorStatus::Submitted {
        (a, b)
    } else {
        (b, a)
    };
    assert_eq!(node.state(&first).status, AnchorStatus::Submitted);
    let state = node.state(&waiting);
    assert_eq!((state.status, state.attempts), (AnchorStatus::Pending, 0));
    assert!(!state.needs_funds);
    assert_eq!(
        state.last_error.as_deref(),
        Some("wallet busy: inputs reserved by in-flight transactions")
    );
}

#[tokio::test]
async fn a_busy_sponsor_does_not_hide_a_wallet_that_needs_funds() {
    let node = one_sponsor_utxo_node();
    node.ledger
        .set_utxos(&node.wallet.payment_address, vec![ada(0x11, 0, 6_000_000)]);
    let a = issue(&node.profile, &node.issuer, "a");
    let b = issue(&node.profile, &node.issuer, "b");
    node.tick(T0_PLUS_7H).await;
    assert_eq!(anchor_submissions(&node).len(), 1);
    let waiting = if node.state(&a).status == AnchorStatus::Pending {
        a
    } else {
        b
    };

    // The wallet is emptied: with the sponsor UTxO free the anchor still
    // could not pledge collateral, so the reason is funds, not a busy wallet.
    node.ledger.set_utxos(&node.wallet.payment_address, vec![]);
    node.tick(T0_PLUS_7H).await;
    assert_eq!(anchor_submissions(&node).len(), 1);
    let state = node.state(&waiting);
    assert_eq!((state.status, state.attempts), (AnchorStatus::Pending, 0));
    assert!(state.needs_funds, "{:?}", state.last_error);
}

/// Live preprod run of the whole anchorer: issue two credentials, flush them
/// as one batch, wait for confirmation, and check the verifier finds them
/// anchored. Ignored by default. Uses the network profile's sponsor unless
/// `ALEXANDRIA_ANCHOR_SPONSOR` (an `anchor_sponsor` JSON object) or
/// `ALEXANDRIA_ANCHOR_FUNDING=issuer` says otherwise. The wallet needs an
/// ADA-only UTxO of 5 tADA or more:
///   BLOCKFROST_PROJECT_ID=preprod… ALEXANDRIA_TEST_MNEMONIC_FILE=… \
///     cargo test -p alexandria-node live_batch_anchor_preprod -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn live_batch_anchor_preprod() {
    let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
    let mnemonic = std::fs::read_to_string(
        std::env::var("ALEXANDRIA_TEST_MNEMONIC_FILE").expect("ALEXANDRIA_TEST_MNEMONIC_FILE"),
    )
    .unwrap();
    let sponsor: Option<AnchorSponsorProfile> =
        match std::env::var("ALEXANDRIA_ANCHOR_FUNDING").as_deref() {
            Ok("issuer") => None,
            _ => match std::env::var("ALEXANDRIA_ANCHOR_SPONSOR") {
                Ok(json) => Some(serde_json::from_str(&json).expect("anchor sponsor JSON")),
                Err(_) => crate::network_profile::embedded_preprod()
                    .ok()
                    .and_then(|p| p.anchor_sponsor.clone()),
            },
        };
    let wallet = crate::crypto::wallet::wallet_from_mnemonic(mnemonic.trim()).unwrap();
    let issuer = Issuer::from_wallet(&wallet);
    let profile = TestProfile::migrated();
    let ledger = crate::cardano::blockfrost::BlockfrostClient::new(pid).unwrap();
    let policy = BatchPolicy {
        cap: 2,
        ..BatchPolicy::default()
    };
    let vcs = [
        issue(&profile, &issuer, "live-a"),
        issue(&profile, &issuer, "live-b"),
    ];
    let pass = || {
        let journal = profile.background();
        let (ledger, wallet, sponsor, policy) = (&ledger, &wallet, sponsor.as_ref(), &policy);
        async move {
            tick(
                &journal,
                Some(ledger),
                Some(wallet),
                sponsor,
                policy,
                &crate::clock::now_rfc3339(),
            )
            .await
            .unwrap()
        }
    };
    let state = |vc: &VerifiableCredential| {
        profile
            .with_conn(|conn| state(conn, vc.id.as_deref().unwrap()))
            .unwrap()
            .unwrap()
    };

    pass().await;
    let submitted = state(&vcs[0]);
    let funding: Option<String> = profile.with_conn(|conn| {
        conn.query_row("SELECT funding FROM anchor_batches", [], |r| r.get(0))
            .ok()
    });
    println!(
        "LIVE_BATCH: status={:?} tx={:?} funding={funding:?} error={:?}",
        submitted.status, submitted.tx_hash, submitted.last_error
    );
    assert!(
        matches!(
            submitted.status,
            AnchorStatus::Submitted | AnchorStatus::OutcomeUnknown
        ),
        "batch not sent: {:?}",
        submitted.last_error
    );
    if sponsor.is_some() {
        assert_eq!(funding.as_deref(), Some("sponsor"));
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
    while state(&vcs[0]).status != AnchorStatus::Confirmed {
        assert!(
            std::time::Instant::now() < deadline,
            "not confirmed in 10 minutes"
        );
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        pass().await;
    }
    for vc in &vcs {
        assert_eq!(state(vc).status, AnchorStatus::Confirmed);
        let anchored = profile.with_conn(|conn| {
            verify_credential_db(
                conn,
                vc,
                &crate::clock::now_rfc3339(),
                &VerificationPolicy::default(),
            )
            .integrity_anchored
        });
        assert!(anchored, "confirmed but not anchored");
    }
    // The ledger's own copy of the transaction reads back as the anchor.
    let tx_hash = state(&vcs[0]).tx_hash.unwrap();
    let observed = crate::cardano::ledger::observe_anchor(&ledger, &tx_hash)
        .await
        .unwrap()
        .expect("ledger shows the anchor");
    assert_eq!(
        observed.required_signers,
        vec![crate::cardano::anchor_tx::did_key_hash(&wallet).to_string()]
    );
    println!(
        "LIVE_BATCH_CONFIRMED: tx={tx_hash} root={} leaves={} anchored_at={:?}",
        observed.root, observed.leaf_count, observed.anchored_at
    );
}
