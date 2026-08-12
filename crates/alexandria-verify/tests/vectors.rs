//! Interoperability test vectors.
//!
//! The promise is that credential verification is free, offline-capable and
//! permanent, and that anyone can implement it. A permissively-licensed Rust
//! crate only serves people who write Rust. What makes the promise real for
//! everyone else is a set of signed credentials with known-correct outcomes: an
//! implementation in Go, TypeScript or Python can be checked against these and
//! shown to agree, without reading a line of this codebase.
//!
//! `tests/vectors/*.json` is the authority. Each file is self-contained — the
//! credential, any registry or status-list state verification needs, the
//! verification time, the policy, and the expected result. Nothing external is
//! consulted; there is no network access and no database anywhere in this crate.
//!
//! # Regenerating
//!
//! ```sh
//! ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --test vectors
//! ```
//!
//! Signing keys are derived from fixed byte patterns, so regeneration is
//! deterministic: the same inputs produce byte-identical files. A regeneration
//! that changes a file is telling you the wire format moved, and that is a fact
//! worth seeing in a diff rather than discovering downstream.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};

use alexandria_verify::did::VerificationMethodRef;
use alexandria_verify::did::{derive_did_key, Did, KeyRegistryEntry};
use alexandria_verify::vc::sign::{sign_credential, UnsignedCredential};
use alexandria_verify::vc::verify::verify_credential;
use alexandria_verify::vc::{
    AcceptanceDecision, CredentialStatus, CredentialSubject, CredentialType, Proof,
    VerifiableCredential, VerificationPolicy, VerificationResult,
};
use alexandria_verify::VerificationStore;

// ---------------------------------------------------------------------------
// Vector format
// ---------------------------------------------------------------------------

/// One vector: everything a verifier needs, and what it must conclude.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Vector {
    /// What this case is for, in a sentence. Written for an implementer who has
    /// a failing test and needs to know what behaviour it is asserting.
    description: String,
    verification_time: String,
    policy: VerificationPolicy,
    credential: VerifiableCredential,
    /// State the verifier is expected to have. Empty means a verifier with no
    /// local context, which is the common case for a third party.
    #[serde(default)]
    store: VectorStore,
    expect: VerificationResult,
}

/// The four lookups verification performs, as data.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorStore {
    /// Historical issuer keys, hex-encoded, keyed by DID.
    #[serde(default)]
    key_registry: Vec<RegistryRow>,
    /// Status-list bits, hex-encoded, keyed by list id.
    #[serde(default)]
    status_lists: BTreeMap<String, String>,
    #[serde(default)]
    suspended: BTreeMap<String, Option<String>>,
    #[serde(default)]
    superseded: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryRow {
    did: String,
    key_id: String,
    public_key_hex: String,
    valid_from: String,
    valid_until: Option<String>,
}

impl VerificationStore for VectorStore {
    fn key_at(&self, did: &Did, at: &str) -> Option<KeyRegistryEntry> {
        self.key_registry
            .iter()
            .filter(|r| {
                r.did == did.as_str()
                    && r.valid_from.as_str() <= at
                    && r.valid_until.as_deref().is_none_or(|u| u > at)
            })
            .max_by(|a, b| a.valid_from.cmp(&b.valid_from))
            .and_then(|r| {
                Some(KeyRegistryEntry {
                    did: did.clone(),
                    key_id: r.key_id.clone(),
                    public_key_bytes: hex::decode(&r.public_key_hex).ok()?,
                    valid_from: r.valid_from.clone(),
                    valid_until: r.valid_until.clone(),
                    rotated_by: None,
                })
            })
    }

    fn status_list_bits(&self, list_id: &str) -> Option<Vec<u8>> {
        self.status_lists
            .get(list_id)
            .and_then(|h| hex::decode(h).ok())
    }

    fn suspension(&self, credential_id: &str) -> Option<(bool, Option<String>)> {
        self.suspended
            .get(credential_id)
            .map(|until| (true, until.clone()))
    }

    fn is_superseded(&self, credential_id: &str) -> bool {
        self.superseded.iter().any(|s| s == credential_id)
    }
}

// ---------------------------------------------------------------------------
// Deterministic material
// ---------------------------------------------------------------------------

/// A signing key derived from a label, so vectors regenerate byte-identically.
/// Not secret and not intended to be — these keys exist to be published.
fn key(label: &str) -> SigningKey {
    let mut bytes = [0u8; 32];
    let b = label.as_bytes();
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = b[i % b.len().max(1)];
    }
    SigningKey::from_bytes(&bytes)
}

const V2_CONTEXT: &str = "https://www.w3.org/ns/credentials/v2";
const ALEX_CONTEXT: &str = "https://alexandria.protocol/context/v1";
const NOW: &str = "2026-06-01T00:00:00Z";

fn skeleton(issuer: Did, subject: Did, valid_until: Option<&str>) -> VerifiableCredential {
    VerifiableCredential {
        context: vec![V2_CONTEXT.into(), ALEX_CONTEXT.into()],
        id: Some("urn:uuid:vector-credential".into()),
        type_: vec!["VerifiableCredential".into(), "FormalCredential".into()],
        issuer,
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_until: valid_until.map(str::to_string),
        credential_subject: CredentialSubject {
            id: subject,
            properties: serde_json::from_value(serde_json::json!({
                "skillId": "skill_vector",
                "level": 3,
                "score": 0.9,
                "evidenceRefs": []
            }))
            .expect("subject properties"),
        },
        credential_status: None,
        terms_of_use: None,
        witness: None,
        integrity: None,
        proof: Proof {
            type_: "Ed25519Signature2020".into(),
            created: "2026-01-01T00:00:00Z".into(),
            verification_method: VerificationMethodRef(String::new()),
            proof_purpose: "assertionMethod".into(),
            jws: String::new(),
        },
    }
}

fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors")
}

// ---------------------------------------------------------------------------
// The vectors
// ---------------------------------------------------------------------------

fn build_all() -> Vec<(&'static str, Vector)> {
    let issuer_key = key("vector-issuer");
    let issuer = derive_did_key(&issuer_key);
    let subject = derive_did_key(&key("vector-subject"));
    let default_policy = VerificationPolicy::default();

    let signed = |vc: VerifiableCredential, k: &SigningKey, did: &Did| {
        sign_credential(UnsignedCredential { credential: vc }, k, did).expect("sign")
    };

    let mut out: Vec<(&'static str, Vector)> = Vec::new();

    // --- accepts ---------------------------------------------------------
    {
        let vc = signed(
            skeleton(issuer.clone(), subject.clone(), None),
            &issuer_key,
            &issuer,
        );
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "01-valid",
            Vector {
                description:
                    "A correctly signed credential with no expiry, verified with no local \
                              context. did:key self-resolution supplies the issuer key."
                        .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store: VectorStore::default(),
                expect,
            },
        ));
    }

    // --- signature failures ----------------------------------------------
    {
        let mut vc = signed(
            skeleton(issuer.clone(), subject.clone(), None),
            &issuer_key,
            &issuer,
        );
        // Change a claim after signing. The JWS still parses; it just no longer
        // covers this payload.
        vc.credential_subject
            .properties
            .insert("level".into(), serde_json::json!(5));
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "02-tampered-payload",
            Vector {
                description: "The claim was altered after signing. A verifier that canonicalizes \
                              correctly rejects this; one that canonicalizes differently may not, \
                              which makes this the sharpest test of a JCS implementation."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store: VectorStore::default(),
                expect,
            },
        ));
    }
    {
        // Signed by a different key than the issuer DID names.
        let impostor = key("vector-impostor");
        let mut vc = signed(
            skeleton(issuer.clone(), subject.clone(), None),
            &impostor,
            &issuer,
        );
        vc.issuer = issuer.clone();
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "03-wrong-signing-key",
            Vector {
                description: "Signed by a key other than the one the issuer DID embeds. The \
                              signature is well-formed and verifies against nothing that matters."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store: VectorStore::default(),
                expect,
            },
        ));
    }
    {
        let mut vc = signed(
            skeleton(issuer.clone(), subject.clone(), None),
            &issuer_key,
            &issuer,
        );
        vc.proof.jws = "not-a-jws".into();
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "04-malformed-jws",
            Vector {
                description: "proof.jws is not a detached JWS. A verifier must reject rather than \
                              error out — malformed input is a verification failure, not a crash."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store: VectorStore::default(),
                expect,
            },
        ));
    }

    // --- expiry -----------------------------------------------------------
    {
        let vc = signed(
            skeleton(
                issuer.clone(),
                subject.clone(),
                Some("2026-03-01T00:00:00Z"),
            ),
            &issuer_key,
            &issuer,
        );
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "05-expired",
            Vector {
                description: "validUntil precedes the verification time. This is the case the v1 \
                              context bug hid: a processor that drops an undefined validUntil \
                              reads this credential as one that never expires."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store: VectorStore::default(),
                expect,
            },
        ));
    }

    // --- subject binding --------------------------------------------------
    {
        let mut vc = skeleton(issuer.clone(), subject.clone(), None);
        vc.credential_subject.id = Did("not-a-did".into());
        let vc = signed(vc, &issuer_key, &issuer);
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "06-non-did-subject",
            Vector {
                description: "The subject id is not a DID, so the credential is not bound to a \
                              holder and cannot be non-transferable."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store: VectorStore::default(),
                expect,
            },
        ));
    }

    // --- revocation -------------------------------------------------------
    {
        let mut vc = skeleton(issuer.clone(), subject.clone(), None);
        vc.credential_status = Some(CredentialStatus {
            id: "urn:uuid:status-entry".into(),
            type_: "RevocationList2020Status".into(),
            status_purpose: "revocation".into(),
            status_list_credential: "urn:uuid:status-list-1".into(),
            status_list_index: "9".into(),
        });
        let vc = signed(vc, &issuer_key, &issuer);
        // Bit 9 set: byte 1, bit 1.
        let mut bits = vec![0u8; 8];
        bits[1] |= 1 << 1;
        let store = VectorStore {
            status_lists: BTreeMap::from([(
                "urn:uuid:status-list-1".to_string(),
                hex::encode(&bits),
            )]),
            ..Default::default()
        };
        let expect = verify_credential(&store, &vc, NOW, &default_policy);
        out.push((
            "07-revoked",
            Vector {
                description: "The status list has the credential's index bit set. Index 9 is byte \
                              1, bit 1 — little-endian within the byte, which is the detail an \
                              independent implementation most often gets backwards."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store,
                expect,
            },
        ));
    }

    // --- key rotation -----------------------------------------------------
    {
        // Signed with a rotated key while keeping the original DID. did:key
        // self-resolution yields the OLD key and fails; only the registry entry
        // for the post-rotation window produces the right one.
        let rotated = key("vector-issuer-v2");
        let vc = signed(
            skeleton(issuer.clone(), subject.clone(), None),
            &rotated,
            &issuer,
        );
        let store = VectorStore {
            key_registry: vec![
                RegistryRow {
                    did: issuer.as_str().to_string(),
                    key_id: "key-1".into(),
                    public_key_hex: hex::encode(issuer_key.verifying_key().as_bytes()),
                    valid_from: "1970-01-01T00:00:00Z".into(),
                    valid_until: Some("2026-02-01T00:00:00Z".into()),
                },
                RegistryRow {
                    did: issuer.as_str().to_string(),
                    key_id: "key-2".into(),
                    public_key_hex: hex::encode(rotated.verifying_key().as_bytes()),
                    valid_from: "2026-02-01T00:00:00Z".into(),
                    valid_until: None,
                },
            ],
            ..Default::default()
        };
        let expect = verify_credential(&store, &vc, NOW, &default_policy);
        out.push((
            "08-rotated-issuer-key",
            Vector {
                description: "The issuer rotated keys and signed with the new one while keeping \
                              their DID. did:key self-resolution returns the pre-rotation key and \
                              fails; the registry entry covering the verification time is what \
                              makes this verify."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store,
                expect,
            },
        ));
    }

    // --- policy -----------------------------------------------------------
    {
        let vc = signed(
            skeleton(
                issuer.clone(),
                subject.clone(),
                Some("2026-03-01T00:00:00Z"),
            ),
            &issuer_key,
            &issuer,
        );
        let permissive = VerificationPolicy {
            reject_expired: false,
            ..VerificationPolicy::default()
        };
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &permissive);
        out.push((
            "09-expired-permissive-policy",
            Vector {
                description: "The same expired credential under a policy that does not reject on \
                              expiry. `expired` stays true — policy changes the decision, never \
                              the facts."
                    .into(),
                verification_time: NOW.into(),
                policy: permissive,
                credential: vc,
                store: VectorStore::default(),
                expect,
            },
        ));
    }
    {
        let vc = signed(
            skeleton(issuer.clone(), subject.clone(), None),
            &issuer_key,
            &issuer,
        );
        let narrow = VerificationPolicy {
            allowed_types: vec![CredentialType::RoleCredential],
            ..VerificationPolicy::default()
        };
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &narrow);
        out.push((
            "10-type-not-allowed",
            Vector {
                description: "A perfectly valid credential of a type the policy does not accept. \
                              Every cryptographic check passes and the decision is still reject."
                    .into(),
                verification_time: NOW.into(),
                policy: narrow,
                credential: vc,
                store: VectorStore::default(),
                expect,
            },
        ));
    }

    out
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

#[test]
fn vectors_match_this_implementation() {
    if std::env::var("ALEXANDRIA_REGENERATE_VECTORS").is_ok() {
        regenerate();
        return;
    }

    let dir = vectors_dir();
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| {
            panic!(
                "read {}: {e}. Regenerate with ALEXANDRIA_REGENERATE_VECTORS=1",
                dir.display()
            )
        })
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();

    assert!(!files.is_empty(), "no vectors found in {}", dir.display());

    for path in files {
        let raw = fs::read_to_string(&path).expect("read vector");
        let v: Vector =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        let got = verify_credential(&v.store, &v.credential, &v.verification_time, &v.policy);

        assert_eq!(
            got.valid_signature,
            v.expect.valid_signature,
            "{}: validSignature",
            path.display()
        );
        assert_eq!(
            got.issuer_resolved,
            v.expect.issuer_resolved,
            "{}: issuerResolved",
            path.display()
        );
        assert_eq!(got.revoked, v.expect.revoked, "{}: revoked", path.display());
        assert_eq!(got.expired, v.expect.expired, "{}: expired", path.display());
        assert_eq!(
            got.subject_bound,
            v.expect.subject_bound,
            "{}: subjectBound",
            path.display()
        );
        assert_eq!(
            got.acceptance_decision,
            v.expect.acceptance_decision,
            "{}: acceptanceDecision",
            path.display()
        );
    }
}

/// Every vector must exercise something the others do not.
///
/// A suite where several cases fail for the same reason gives false confidence:
/// an implementation can pass most of it while getting a whole check wrong.
#[test]
fn the_suite_covers_distinct_failure_modes() {
    let all = build_all();
    assert!(all.len() >= 10, "suite has shrunk: {} vectors", all.len());

    let accepts = all
        .iter()
        .filter(|(_, v)| v.expect.acceptance_decision == AcceptanceDecision::Accept)
        .count();
    assert!(
        accepts >= 2,
        "a suite that never accepts cannot detect a verifier that rejects everything"
    );
    assert!(
        accepts < all.len(),
        "a suite that always accepts cannot detect a verifier that accepts everything"
    );

    // Each of the four independent facts must be false in at least one vector,
    // or an implementation could hardcode it true and still pass.
    assert!(
        all.iter().any(|(_, v)| !v.expect.valid_signature),
        "no vector fails the signature"
    );
    assert!(
        all.iter().any(|(_, v)| v.expect.expired),
        "no vector is expired"
    );
    assert!(
        all.iter().any(|(_, v)| v.expect.revoked),
        "no vector is revoked"
    );
    assert!(
        all.iter().any(|(_, v)| !v.expect.subject_bound),
        "no vector fails subject binding"
    );
}

fn regenerate() {
    let dir = vectors_dir();
    fs::create_dir_all(&dir).expect("create vectors dir");
    for entry in fs::read_dir(&dir).expect("read dir").flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|x| x == "json") {
            fs::remove_file(p).expect("remove stale vector");
        }
    }
    for (name, v) in build_all() {
        let path = dir.join(format!("{name}.json"));
        let json = serde_json::to_string_pretty(&v).expect("serialize vector");
        fs::write(&path, json + "\n").expect("write vector");
        println!("wrote {}", path.display());
    }
}
