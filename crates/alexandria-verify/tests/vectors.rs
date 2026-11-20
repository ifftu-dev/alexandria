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

use alexandria_verify::attestation::{AttestationOutcome, Platform};
use alexandria_verify::coverage::{self, CoverageStatement};
use alexandria_verify::custody::{CustodyDefect, Grader, HostedAttemptEvidence};
use alexandria_verify::did::{derive_did_key, Did, KeyRegistryEntry};
use alexandria_verify::integrity::AnswerCustody;
use alexandria_verify::org::{organisation_backing, OrgDescriptor, OrganisationBacking};
use alexandria_verify::vc::sign::{sign_credential, UnsignedCredential};
use alexandria_verify::vc::verify::verify_credential;
use alexandria_verify::vc::{
    AcceptanceDecision, CredentialIssuer, CredentialStatus, CredentialSubject, CredentialType,
    IntegrityAssertion, ProbeSummary, Proof, VerifiableCredential, VerificationPolicy,
    VerificationResult,
};
use alexandria_verify::{StoreLookup, VerificationStore};

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
    /// The organisation descriptor the caller resolved for the issuer's
    /// domain (`org::descriptor_from_txt`), when one was supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    organisation_descriptor: Option<OrgDescriptor>,
    /// The organisation backing `org::organisation_backing` must report for
    /// this credential and descriptor. Absent for vectors that do not test it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expect_organisation_backing: Option<OrganisationBacking>,
}

/// The lookups verification performs, as data. Suspension has no lookup of
/// its own: it is a bit in a status list of purpose `suspension`.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorStore {
    /// Registered issuer keys, hex-encoded, keyed by DID. Consulted only for
    /// an issuer whose DID is not self-resolving; a `did:key` ignores it.
    #[serde(default)]
    key_registry: Vec<RegistryRow>,
    /// Status-list bits, hex-encoded, keyed by list id.
    #[serde(default)]
    status_lists: BTreeMap<String, String>,
    #[serde(default)]
    superseded: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryRow {
    did: String,
    key_id: String,
    public_key_hex: String,
}

impl VerificationStore for VectorStore {
    fn registered_key(&self, did: &Did) -> StoreLookup<KeyRegistryEntry> {
        self.key_registry
            .iter()
            .find(|r| r.did == did.as_str())
            .and_then(|r| {
                Some(KeyRegistryEntry {
                    did: did.clone(),
                    key_id: r.key_id.clone(),
                    public_key_bytes: hex::decode(&r.public_key_hex).ok()?,
                })
            })
            .map(StoreLookup::Found)
            .unwrap_or(StoreLookup::Missing)
    }

    fn status_list_bits(&self, list_id: &str) -> StoreLookup<Vec<u8>> {
        self.status_lists
            .get(list_id)
            .and_then(|h| hex::decode(h).ok())
            .map(StoreLookup::Found)
            .unwrap_or(StoreLookup::Missing)
    }

    fn is_superseded(&self, credential_id: &str) -> StoreLookup<bool> {
        StoreLookup::Found(self.superseded.iter().any(|s| s == credential_id))
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
const NOW: &str = "2026-06-01T00:00:00Z";
const ORG_NAME: &str = "Vector Organisation";
const ORG_DOMAIN: &str = "org.example";
/// The name the domain's descriptor publishes; differs from the issuer
/// object's own `name` so a vector can show which one is reported.
const ORG_PUBLISHED_NAME: &str = "Vector Organisation Ltd";

fn skeleton(issuer: Did, subject: Did, valid_until: Option<&str>) -> VerifiableCredential {
    VerifiableCredential {
        context: vec![V2_CONTEXT.into()],
        id: Some("urn:uuid:vector-credential".into()),
        type_: vec!["VerifiableCredential".into(), "FormalCredential".into()],
        issuer: issuer.into(),
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
        evidence: Vec::new(),
        related_resource: Vec::new(),
        proof: Proof::unsigned("2026-01-01T00:00:00Z"),
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
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }

    // --- incomplete verification evidence -------------------------------
    {
        let mut vc = skeleton(issuer.clone(), subject.clone(), None);
        vc.credential_status = Some(
            CredentialStatus {
                id: "urn:uuid:missing-status-entry".into(),
                type_: "BitstringStatusListEntry".into(),
                status_purpose: "revocation".into(),
                status_list_credential: "urn:uuid:missing-status-list".into(),
                status_list_index: "0".into(),
            }
            .into(),
        );
        let vc = signed(vc, &issuer_key, &issuer);
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "11-missing-status-list",
            Vector {
                description: "The signature is authentic, but the referenced status list is not supplied. The decision is pending rather than accepting absence as proof of non-revocation."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store: VectorStore::default(),
                expect,
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }
    {
        let external_issuer = Did("did:web:issuer.example".into());
        let vc = signed(
            skeleton(external_issuer.clone(), subject.clone(), None),
            &issuer_key,
            &external_issuer,
        );
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "12-missing-issuer-key",
            Vector {
                description: "The issuer uses a DID method that is not self-resolving in this verifier and no key binding is supplied. The decision remains pending until that binding is acquired."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store: VectorStore::default(),
                expect,
                organisation_descriptor: None,
                expect_organisation_backing: None,
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
                organisation_descriptor: None,
                expect_organisation_backing: None,
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
        vc.issuer = issuer.clone().into();
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
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }
    {
        let mut vc = signed(
            skeleton(issuer.clone(), subject.clone(), None),
            &issuer_key,
            &issuer,
        );
        vc.proof.proof_value = "not-multibase".into();
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "04-malformed-proof-value",
            Vector {
                description: "proof.proofValue is not a multibase Ed25519 signature. A verifier must reject rather than \
                              error out — malformed input is a verification failure, not a crash."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store: VectorStore::default(),
                expect,
                organisation_descriptor: None,
                expect_organisation_backing: None,
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
                organisation_descriptor: None,
                expect_organisation_backing: None,
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
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }

    // --- revocation -------------------------------------------------------
    {
        let mut vc = skeleton(issuer.clone(), subject.clone(), None);
        vc.credential_status = Some(
            CredentialStatus {
                id: "urn:uuid:status-entry".into(),
                type_: "BitstringStatusListEntry".into(),
                status_purpose: "revocation".into(),
                status_list_credential: "urn:uuid:status-list-1".into(),
                status_list_index: "9".into(),
            }
            .into(),
        );
        let vc = signed(vc, &issuer_key, &issuer);
        // Bit 9 set: byte 1, second-most-significant bit (0x40). Bitstring
        // Status List counts from the left, which is what trips verifiers
        // that assume the little-endian order of older status-list drafts.
        let mut bits = vec![0u8; 8];
        alexandria_verify::vc::status::set_bit(&mut bits, 9, true).unwrap();
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
                              1, second from the left (mask 0x40): Bitstring Status List counts \
                              from the most significant bit, which is the detail an independent \
                              implementation most often gets backwards."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store,
                expect,
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }

    // --- suspension -------------------------------------------------------
    // Two entries, as credentials are issued today: revocation and
    // suspension, each into a list of that purpose, at the same index.
    let both_entries = |index: &str| -> VerifiableCredential {
        let mut vc = skeleton(issuer.clone(), subject.clone(), None);
        vc.credential_status = Some(
            vec![
                CredentialStatus {
                    id: format!("urn:uuid:revocation-list#{index}"),
                    type_: "BitstringStatusListEntry".into(),
                    status_purpose: "revocation".into(),
                    status_list_credential: "urn:uuid:revocation-list".into(),
                    status_list_index: index.into(),
                },
                CredentialStatus {
                    id: format!("urn:uuid:suspension-list#{index}"),
                    type_: "BitstringStatusListEntry".into(),
                    status_purpose: "suspension".into(),
                    status_list_credential: "urn:uuid:suspension-list".into(),
                    status_list_index: index.into(),
                },
            ]
            .into(),
        );
        signed(vc, &issuer_key, &issuer)
    };
    let lists = |suspension: Option<&[u8]>| VectorStore {
        status_lists: [
            ("urn:uuid:revocation-list", Some(&[0u8; 8][..])),
            ("urn:uuid:suspension-list", suspension),
        ]
        .into_iter()
        .filter_map(|(id, bits)| Some((id.to_string(), hex::encode(bits?))))
        .collect(),
        ..Default::default()
    };
    {
        let vc = both_entries("4");
        // Bit 4: byte 0, mask 0x08.
        let store = lists(Some(&[0x08, 0, 0, 0, 0, 0, 0, 0]));
        let expect = verify_credential(&store, &vc, NOW, &default_policy);
        out.push((
            "16-suspended",
            Vector {
                description: "The credential carries a revocation entry and a suspension entry. The \
                              suspension list has its index bit set (index 4: byte 0, mask 0x08), so \
                              the credential is suspended and rejected while the revocation list, \
                              clear at that index, leaves it unrevoked. Suspension is read from the \
                              published list alone."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store,
                expect,
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }
    {
        let vc = both_entries("4");
        let store = lists(Some(&[0u8; 8]));
        let expect = verify_credential(&store, &vc, NOW, &default_policy);
        out.push((
            "17-reinstated",
            Vector {
                description: "The same credential after its issuer reinstated it: the suspension \
                              list's bit is clear again, and the credential is accepted. Suspension \
                              is reversible; revocation is not."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store,
                expect,
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }
    {
        let vc = both_entries("4");
        let store = lists(None);
        let expect = verify_credential(&store, &vc, NOW, &default_policy);
        out.push((
            "18-suspension-list-missing",
            Vector {
                description: "The revocation list is supplied but the suspension list is not. The \
                              decision is pending with reason suspension_list_missing: an \
                              unreachable list is never read as \"not suspended\"."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store,
                expect,
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }

    // --- issuer resolution ----------------------------------------------
    {
        // A did:key is its key. The verifier is handed registry state for the
        // issuer's DID naming some other key (stale, forged, or left over from
        // the retired rotation feature): it must not change which key the
        // signature is checked against.
        let vc = signed(
            skeleton(issuer.clone(), subject.clone(), None),
            &issuer_key,
            &issuer,
        );
        let store = VectorStore {
            key_registry: vec![RegistryRow {
                did: issuer.as_str().to_string(),
                key_id: "key-2".into(),
                public_key_hex: hex::encode(key("vector-issuer-v2").verifying_key().as_bytes()),
            }],
            ..Default::default()
        };
        let expect = verify_credential(&store, &vc, NOW, &default_policy);
        out.push((
            "08-did-key-ignores-registry",
            Vector {
                description: "A did:key issuer, verified with registry state supplied for that \
                              DID that names a different key. A did:key resolves to the key in \
                              its identifier, always: the registry entry is ignored and the \
                              credential verifies under the embedded key."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store,
                expect,
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }

    // --- organisation backing --------------------------------------------
    {
        // VC 2.0 allows `issuer` as an object. The proof is checked against
        // its `id`; `name` and `domain` are the issuer's own statement until
        // a descriptor from that domain backs them.
        let mut vc = skeleton(issuer.clone(), subject.clone(), None);
        vc.issuer = CredentialIssuer::organisation(issuer.clone(), ORG_NAME, ORG_DOMAIN);
        let vc = signed(vc, &issuer_key, &issuer);
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "19-object-issuer",
            Vector {
                description: "The issuer is an object carrying the signing key's DID as `id` \
                              with the organisation's `name` and `domain`, as VC 2.0 allows. \
                              The key is resolved from `id` and the proof's verification \
                              method sits under `id`; the credential is accepted. No descriptor \
                              for the domain is supplied, so organisation backing is \
                              descriptor_not_supplied."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                expect_organisation_backing: Some(organisation_backing(&vc, None)),
                credential: vc,
                store: VectorStore::default(),
                expect,
                organisation_descriptor: None,
            },
        ));
    }
    // The same object-issuer credential, judged against descriptors the
    // caller resolved. Backing never changes the verification result.
    let org_vector = |description: &str, descriptor: OrgDescriptor| {
        let mut vc = skeleton(issuer.clone(), subject.clone(), None);
        vc.issuer = CredentialIssuer::organisation(issuer.clone(), ORG_NAME, ORG_DOMAIN);
        let vc = signed(vc, &issuer_key, &issuer);
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        Vector {
            description: description.into(),
            verification_time: NOW.into(),
            policy: default_policy.clone(),
            expect_organisation_backing: Some(organisation_backing(&vc, Some(&descriptor))),
            credential: vc,
            store: VectorStore::default(),
            expect,
            organisation_descriptor: Some(descriptor),
        }
    };
    let successor = derive_did_key(&key("vector-org-successor"));
    let descriptor = |domain: &str, name: &str, keys: &[&Did]| OrgDescriptor {
        format: alexandria_verify::org::DESCRIPTOR_FORMAT.into(),
        domain: domain.into(),
        name: name.into(),
        keys: keys.iter().map(|k| k.as_str().to_string()).collect(),
    };
    out.push((
        "20-organisation-backing-verified",
        org_vector(
            "The descriptor for the issuer's domain lists the issuer key among its keys. \
             Organisation backing is verified, with the name and domain the descriptor \
             publishes: the issuer object's own name is not the one reported.",
            descriptor(ORG_DOMAIN, ORG_PUBLISHED_NAME, &[&successor, &issuer]),
        ),
    ));
    out.push((
        "21-organisation-key-not-listed",
        org_vector(
            "The organisation removed the issuer key from its domain's descriptor, which \
             now lists only a successor key. Organisation backing is key_not_listed, while \
             the signature still verifies and the credential is accepted as a plain key's.",
            descriptor(ORG_DOMAIN, ORG_PUBLISHED_NAME, &[&successor]),
        ),
    ));
    out.push((
        "22-organisation-wrong-domain",
        org_vector(
            "The caller supplied the descriptor of a domain other than the one the issuer \
             object names. It lists the issuer key, but backs nothing for this credential: \
             organisation backing is wrong_domain.",
            descriptor("other.example", "Other Organisation", &[&issuer]),
        ),
    ));
    out.push((
        "23-organisation-invalid-descriptor",
        org_vector(
            "The supplied descriptor for the issuer's domain lists a key that is not a \
             did:key, so it fails the descriptor checks and backs nothing: organisation \
             backing is invalid_descriptor, even though it also lists the issuer key.",
            OrgDescriptor {
                keys: vec![issuer.as_str().into(), "did:web:org.example".into()],
                ..descriptor(ORG_DOMAIN, ORG_PUBLISHED_NAME, &[])
            },
        ),
    ));

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
                organisation_descriptor: None,
                expect_organisation_backing: None,
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
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }

    // --- coverage statements ----------------------------------------------
    {
        // Issued before credentials named their statement by relatedResource:
        // only the integrity assertion's digest names it.
        let v1 = coverage::statement("1", "core", "process", None).expect("v1 statement");
        let mut vc = skeleton(issuer.clone(), subject.clone(), None);
        vc.integrity = Some(integrity_naming(&v1));
        let vc = signed(vc, &issuer_key, &issuer);
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "13-coverage-statement-v1",
            Vector {
                description: "An integrity assertion names a version-1 coverage statement by \
                              coverageStatementDigest alone, as credentials issued before \
                              relatedResource did. The digest resolves from the statement \
                              registry; there is nothing to cross-check, so it is accepted."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store: VectorStore::default(),
                expect,
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }
    {
        let v2 = coverage::statement("2", "full", "live", None).expect("v2 statement");
        let mut vc = skeleton(issuer.clone(), subject.clone(), None);
        vc.integrity = Some(integrity_naming(&v2));
        vc.related_resource = vec![coverage::related_resource(
            &v2,
            Some("https://status.example"),
        )];
        let vc = signed(vc, &issuer_key, &issuer);
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "14-coverage-statement-v2",
            Vector {
                description: "A version-2 coverage statement named twice: by the integrity \
                              assertion's coverageStatementDigest and by a relatedResource whose \
                              id is {host}/coverage-statements/{digest}, with digestSRI over the \
                              statement's bytes. The digest in the id matches, so it is accepted."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store: VectorStore::default(),
                expect,
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }
    {
        let named = coverage::statement("2", "core", "process", None).expect("v2 statement");
        let other = coverage::statement("2", "camera", "live", None).expect("v2 statement");
        let mut vc = skeleton(issuer.clone(), subject.clone(), None);
        vc.integrity = Some(integrity_naming(&named));
        vc.related_resource = vec![coverage::related_resource(&other, None)];
        let vc = signed(vc, &issuer_key, &issuer);
        let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
        out.push((
            "15-coverage-statement-mismatch",
            Vector {
                description: "The relatedResource names a coverage statement \
                              (urn:alexandria:coverage-statement:{digest}) whose digest differs \
                              from the integrity assertion's coverageStatementDigest. The \
                              signature is valid, but the credential names two statements, so \
                              it is rejected with coverageStatementMismatch."
                    .into(),
                verification_time: NOW.into(),
                policy: default_policy.clone(),
                credential: vc,
                store: VectorStore::default(),
                expect,
                organisation_descriptor: None,
                expect_organisation_backing: None,
            },
        ));
    }

    // --- answer custody and hosted attempts -------------------------------
    {
        let learner_key = key("vector-learner");
        let learner = derive_did_key(&learner_key);
        let core_probed = coverage::statement("3", "core", "probed", None).expect("v3 statement");
        let attested_probed =
            coverage::statement("3", "attested", "probed", None).expect("v3 statement");
        let stating = |statement: &CoverageStatement, custody: AnswerCustody| IntegrityAssertion {
            answer_custody: Some(custody),
            probe: Some(ProbeSummary {
                outcome: "satisfied".into(),
                policies: vec!["authored".into()],
                satisfied: 1,
                failed: 0,
                skipped: 0,
            }),
            ..integrity_naming(statement)
        };
        let hosted = || {
            HostedAttemptEvidence::new(
                "hatt_vector",
                Some(ProbeSummary {
                    outcome: "satisfied".into(),
                    policies: vec!["authored".into()],
                    satisfied: 1,
                    failed: 0,
                    skipped: 0,
                }),
                AttestationOutcome::VerifiedCredentialBound {
                    platform: Platform::Ios,
                    companion: false,
                },
            )
        };
        // A self-issued credential: the learner signs about themselves.
        let self_issued = |custody: AnswerCustody, evidence: Vec<HostedAttemptEvidence>| {
            let mut vc = skeleton(learner.clone(), learner.clone(), None);
            vc.integrity = Some(stating(&core_probed, custody));
            vc.related_resource = vec![coverage::related_resource(&core_probed, None)];
            vc.evidence = evidence.into_iter().map(|e| e.into_entry()).collect();
            signed(vc, &learner_key, &learner)
        };
        // An organisation's role credential from a hosted attempt.
        let org_issued = |custody: AnswerCustody, entry: HostedAttemptEvidence| {
            let mut vc = skeleton(issuer.clone(), subject.clone(), None);
            vc.issuer = CredentialIssuer::organisation(issuer.clone(), ORG_NAME, ORG_DOMAIN);
            vc.type_ = vec!["VerifiableCredential".into(), "RoleCredential".into()];
            vc.integrity = Some(stating(&attested_probed, custody));
            vc.related_resource = vec![coverage::related_resource(
                &attested_probed,
                Some("https://status.example"),
            )];
            vc.evidence = vec![entry.into_entry()];
            signed(vc, &issuer_key, &issuer)
        };
        let mut push = |name: &'static str, description: &str, vc: VerifiableCredential| {
            let expect = verify_credential(&VectorStore::default(), &vc, NOW, &default_policy);
            out.push((
                name,
                Vector {
                    description: description.into(),
                    verification_time: NOW.into(),
                    policy: default_policy.clone(),
                    credential: vc,
                    store: VectorStore::default(),
                    expect,
                    organisation_descriptor: None,
                    expect_organisation_backing: None,
                },
            ));
        };
        push(
            "24-device-custody-self-issued",
            "A self-issued credential whose integrity assertion states answerCustody \
             `device`, naming a version-3 coverage statement: the learner's device held the \
             answers, which is all a self-issued credential can say. Accepted.",
            self_issued(AnswerCustody::Device, vec![]),
        );
        push(
            "25-issuer-custody-self-issued",
            "A self-issued credential (issuer is the subject) whose integrity assertion \
             states answerCustody `issuer`. Nobody holds answers away from themselves: \
             rejected with custodyDefects [issuer_custody_self_issued].",
            self_issued(AnswerCustody::Issuer, vec![]),
        );
        push(
            "26-hosted-attempt-issuer-signed",
            "An organisation's role credential from a hosted attempt: issuer-signed, the \
             assertion states answerCustody `issuer`, and one HostedAttemptEvidence entry \
             names the attempt, says the issuer held the answers and graded, and records \
             the probe outcome and the attestation outcome and verified assurance level \
             the issuer found. Accepted; the entry is read back as written.",
            org_issued(AnswerCustody::Issuer, hosted()),
        );
        push(
            "27-hosted-attempt-self-issued",
            "A self-issued credential carrying a HostedAttemptEvidence entry and stating \
             `issuer` custody. The entry is only valid on an issuer-signed credential: \
             rejected with custodyDefects [issuer_custody_self_issued, \
             hosted_attempt_self_issued].",
            self_issued(AnswerCustody::Issuer, vec![hosted()]),
        );
        push(
            "28-hosted-attempt-custody-mismatch",
            "An issuer-signed credential with a HostedAttemptEvidence entry whose \
             integrity assertion states answerCustody `device`: the credential says two \
             things about who held the answers. Rejected with custodyDefects \
             [hosted_attempt_custody_mismatch].",
            org_issued(AnswerCustody::Device, hosted()),
        );
        push(
            "29-hosted-attempt-graded-by-device",
            "An issuer-signed credential whose HostedAttemptEvidence entry says the device \
             graded. A hosted attempt is graded by the issuer: rejected with \
             custodyDefects [hosted_attempt_invalid].",
            org_issued(
                AnswerCustody::Issuer,
                HostedAttemptEvidence {
                    graded_by: Grader::Device,
                    ..hosted()
                },
            ),
        );
    }

    out
}

/// A terminal integrity assertion on `statement`'s profile and rung that
/// names it by digest.
fn integrity_naming(statement: &CoverageStatement) -> IntegrityAssertion {
    IntegrityAssertion {
        session_id: "vector-session".into(),
        status: "completed".into(),
        integrity_score: None,
        critical_count: 0,
        warning_count: 0,
        assurance_level: "local".into(),
        commitment_root: None,
        anchor_ref: None,
        generated_at: "2026-01-01T00:00:00Z".into(),
        trust_profile: Some(statement.profile.clone()),
        verification_rung: Some(statement.rung.clone()),
        coverage_statement_digest: Some(statement.digest()),
        signals_run: Vec::new(),
        signals_degraded: Vec::new(),
        signals_not_run: Vec::new(),
        camera_coverage: None,
        probe: None,
        answer_custody: None,
    }
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
        assert_eq!(
            got.status_valid,
            v.expect.status_valid,
            "{}: statusValid",
            path.display()
        );
        assert_eq!(got.expired, v.expect.expired, "{}: expired", path.display());
        assert_eq!(
            got.suspended,
            v.expect.suspended,
            "{}: suspended",
            path.display()
        );
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
        assert_eq!(
            got.pending_reasons,
            v.expect.pending_reasons,
            "{}: pendingReasons",
            path.display()
        );
        assert_eq!(
            got.coverage_statement_mismatch,
            v.expect.coverage_statement_mismatch,
            "{}: coverageStatementMismatch",
            path.display()
        );
        assert_eq!(
            got.custody_defects,
            v.expect.custody_defects,
            "{}: custodyDefects",
            path.display()
        );
        if let Some(expected) = &v.expect_organisation_backing {
            assert_eq!(
                &organisation_backing(&v.credential, v.organisation_descriptor.as_ref()),
                expected,
                "{}: organisationBacking",
                path.display()
            );
        }
        // A coverage statement the credential names consistently resolves
        // from the registry, whatever its version.
        if let Some(digest) = v
            .credential
            .integrity
            .as_ref()
            .and_then(|a| a.coverage_statement_digest.as_deref())
            .filter(|_| !got.coverage_statement_mismatch)
        {
            assert!(
                coverage::resolve(digest, None).is_some(),
                "{}: coverage statement {digest} does not resolve",
                path.display()
            );
        }
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
    assert!(
        all.iter()
            .any(|(_, v)| v.expect.coverage_statement_mismatch),
        "no vector names a mismatched coverage statement"
    );
    for version in coverage::VERSIONS {
        assert!(
            all.iter().any(|(_, v)| v
                .credential
                .integrity
                .as_ref()
                .and_then(|a| a.coverage_statement_digest.as_deref())
                .and_then(|d| coverage::resolve(d, None))
                .is_some_and(|s| s.version == version)),
            "no vector names a version-{version} coverage statement"
        );
    }
    assert!(
        all.iter().any(|(_, v)| v.expect.suspended),
        "no vector is suspended"
    );
    for defect in CustodyDefect::ALL {
        assert!(
            all.iter()
                .any(|(_, v)| v.expect.custody_defects.contains(&defect)),
            "no vector for custody defect {defect:?}"
        );
    }
}

/// The custody vectors state the spec's rules: a self-issued credential may
/// state `device` custody only, and a hosted attempt entry is valid only on
/// an issuer-signed credential that states `issuer` custody and says the
/// issuer graded. Written by hand, not read from the verifier's output.
#[test]
fn custody_vectors_assert_the_custody_rules() {
    use AcceptanceDecision::{Accept, Reject};
    use CustodyDefect as D;
    let all = build_all();
    let get = |name: &str| &all.iter().find(|(n, _)| *n == name).expect(name).1;
    for (name, decision, defects) in [
        ("24-device-custody-self-issued", Accept, vec![]),
        (
            "25-issuer-custody-self-issued",
            Reject,
            vec![D::IssuerCustodySelfIssued],
        ),
        ("26-hosted-attempt-issuer-signed", Accept, vec![]),
        (
            "27-hosted-attempt-self-issued",
            Reject,
            vec![D::IssuerCustodySelfIssued, D::HostedAttemptSelfIssued],
        ),
        (
            "28-hosted-attempt-custody-mismatch",
            Reject,
            vec![D::HostedAttemptCustodyMismatch],
        ),
        (
            "29-hosted-attempt-graded-by-device",
            Reject,
            vec![D::HostedAttemptInvalid],
        ),
    ] {
        let v = get(name);
        assert!(v.expect.valid_signature, "{name}: the signature verifies");
        assert_eq!(v.expect.acceptance_decision, decision, "{name}");
        assert_eq!(v.expect.custody_defects, defects, "{name}");
    }

    // The accepted hosted attempt exposes its entry as the issuer wrote it.
    let v = get("26-hosted-attempt-issuer-signed");
    let entry = HostedAttemptEvidence::find(&v.credential)
        .expect("one well-formed entry")
        .expect("an entry");
    assert_eq!(entry.attempt_id, "hatt_vector");
    assert_eq!(entry.answer_custody, AnswerCustody::Issuer);
    assert_eq!(entry.graded_by, Grader::Issuer);
    assert_eq!(
        entry.verified_assurance_level,
        alexandria_verify::attestation::VerifiedAssuranceLevel::DeviceAttested
    );
    let wire = serde_json::to_value(&v.credential.evidence[0]).expect("serialize");
    assert_eq!(wire["type"], "HostedAttemptEvidence");
    assert_eq!(wire["attemptId"], "hatt_vector");
    assert_eq!(wire["answerCustody"], "issuer");
    assert_eq!(wire["gradedBy"], "issuer");
    assert_eq!(wire["probe"]["outcome"], "satisfied");
    assert_eq!(wire["attestation"]["outcome"], "verified_credential_bound");
    assert_eq!(wire["verifiedAssuranceLevel"], "device_attested");
    let custody = v.credential.integrity.as_ref().expect("assertion");
    assert_eq!(custody.answer_custody(), AnswerCustody::Issuer);
}

/// A `did:key` resolves to the key in its identifier whatever registry state
/// the verifier holds for it. Asserted against the spec's rule rather than the
/// implementation's own output.
#[test]
fn a_did_key_issuer_resolves_its_embedded_key_despite_registry_state() {
    let (_, v) = build_all()
        .into_iter()
        .find(|(name, _)| *name == "08-did-key-ignores-registry")
        .expect("the stale-registry vector exists");
    assert!(
        !v.store.key_registry.is_empty(),
        "the vector must supply registry state for the issuer"
    );
    assert!(v.expect.issuer_resolved);
    assert!(v.expect.valid_signature);
    assert_eq!(v.expect.acceptance_decision, AcceptanceDecision::Accept);
}

/// An object issuer verifies against its `id` and keeps its object form on
/// the wire, so the signature covers the name and domain the issuer stated.
#[test]
fn an_object_issuer_verifies_against_its_id() {
    let (_, v) = build_all()
        .into_iter()
        .find(|(name, _)| *name == "19-object-issuer")
        .expect("the object-issuer vector exists");
    let wire = serde_json::to_value(&v.credential).expect("serialize");
    assert_eq!(wire["issuer"]["id"], v.credential.issuer.id().as_str());
    assert_eq!(wire["issuer"]["name"], ORG_NAME);
    assert_eq!(wire["issuer"]["domain"], ORG_DOMAIN);
    assert!(v.expect.issuer_resolved);
    assert!(v.expect.valid_signature);
    assert_eq!(v.expect.acceptance_decision, AcceptanceDecision::Accept);

    // Renaming the organisation after signing breaks the signature.
    let mut tampered = v.credential.clone();
    tampered.issuer = CredentialIssuer::organisation(
        tampered.issuer.id().clone(),
        "Another Organisation",
        ORG_DOMAIN,
    );
    let got = verify_credential(&VectorStore::default(), &tampered, NOW, &v.policy);
    assert!(!got.valid_signature);
}

/// The organisation backing vectors state what the spec promises: backing
/// verified with the published name and domain, withdrawn when the key leaves
/// the descriptor, refused for another domain's or a malformed descriptor,
/// and never a change to the verification result itself.
#[test]
fn organisation_backing_vectors_report_what_the_descriptor_says() {
    let all = build_all();
    let get = |name: &str| &all.iter().find(|(n, _)| *n == name).expect(name).1;
    let backing = |name: &str| {
        serde_json::to_value(get(name).expect_organisation_backing.as_ref().expect(name))
            .expect("serialize backing")
    };

    assert_eq!(
        backing("19-object-issuer"),
        serde_json::json!({"outcome": "descriptor_not_supplied"})
    );
    assert_eq!(
        backing("20-organisation-backing-verified"),
        serde_json::json!({
            "outcome": "verified",
            "name": "Vector Organisation Ltd",
            "domain": "org.example",
        })
    );
    assert_eq!(
        backing("21-organisation-key-not-listed"),
        serde_json::json!({"outcome": "key_not_listed"})
    );
    assert_eq!(
        backing("22-organisation-wrong-domain"),
        serde_json::json!({"outcome": "wrong_domain"})
    );
    assert_eq!(
        backing("23-organisation-invalid-descriptor"),
        serde_json::json!({"outcome": "invalid_descriptor"})
    );
    for name in [
        "19-object-issuer",
        "20-organisation-backing-verified",
        "21-organisation-key-not-listed",
        "22-organisation-wrong-domain",
        "23-organisation-invalid-descriptor",
    ] {
        let v = get(name);
        assert!(v.expect.valid_signature, "{name}: signature still verifies");
        assert_eq!(
            v.expect.acceptance_decision,
            AcceptanceDecision::Accept,
            "{name}: backing never changes the decision"
        );
    }
}

/// The suspension vectors state facts an implementer can check by hand:
/// the bit, not any issuer record, decides suspension, and a list that is not
/// supplied leaves the decision pending.
#[test]
fn suspension_vectors_assert_what_the_list_says() {
    use alexandria_verify::vc::VerificationPendingReason;
    let all = build_all();
    let get = |name: &str| &all.iter().find(|(n, _)| *n == name).expect(name).1;

    let suspended = get("16-suspended");
    assert!(suspended.expect.suspended && !suspended.expect.revoked);
    assert_eq!(
        suspended.expect.acceptance_decision,
        AcceptanceDecision::Reject
    );

    let reinstated = get("17-reinstated");
    assert!(!reinstated.expect.suspended);
    assert_eq!(
        reinstated.expect.acceptance_decision,
        AcceptanceDecision::Accept
    );

    let missing = get("18-suspension-list-missing");
    assert!(!missing.expect.suspended);
    assert_eq!(
        missing.expect.acceptance_decision,
        AcceptanceDecision::Pending
    );
    assert_eq!(
        missing.expect.pending_reasons,
        vec![VerificationPendingReason::SuspensionListMissing]
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
