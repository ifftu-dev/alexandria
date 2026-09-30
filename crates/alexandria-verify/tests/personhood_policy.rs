use alexandria_verify::personhood::{
    DocumentIssuer, DocumentPolicy, IssuerStatus, PinnedDocumentPolicy, PolicyError, PolicyKind,
    DOCUMENT_CIRCUIT, SYNTHETIC_ISSUER, VERIFICATION_KEY_SHA256,
};
use sha2::{Digest, Sha256};

const NOW: u64 = 1_800_000_000;

fn policy() -> DocumentPolicy {
    DocumentPolicy {
        schema_version: 1,
        policy_version: 1,
        network_id: "policy-tests".into(),
        kind: PolicyKind::LocalDocumentReceipt,
        document_format: "aadhaar_secure_qr_v2".into(),
        circuit_id: DOCUMENT_CIRCUIT.into(),
        verification_key_sha256: VERIFICATION_KEY_SHA256.into(),
        valid_from: NOW - 86400,
        valid_until: NOW + 86400 * 7,
        nullifier_scope: "network_personhood_v1".into(),
        nullifier_seed: "123".into(),
        max_document_age_seconds: 604800,
        max_future_skew_seconds: 300,
        challenge_ttl_seconds: 120,
        receipt_ttl_seconds: 86400,
        // Deliberately fabricated policy-only data; not a production issuer or proof.
        issuers: vec![DocumentIssuer {
            id: "policy-test-issuer".into(),
            circuit_key_hash: "456".into(),
            certificate_sha256: "ab".repeat(32),
            certificate_source: "https://uidai.gov.in/images/policy-test.cer".into(),
            valid_from: NOW - 86400 * 10,
            valid_until: NOW + 86400 * 10,
            status: IssuerStatus::Active,
        }],
    }
}
fn bytes(value: &DocumentPolicy) -> Vec<u8> {
    serde_json_canonicalizer::to_vec(value).unwrap()
}
fn pin(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn load(value: &DocumentPolicy) -> Result<PinnedDocumentPolicy, PolicyError> {
    let bytes = bytes(value);
    PinnedDocumentPolicy::from_pinned(Some(&bytes), Some(&pin(&bytes)), "policy-tests")
}
fn signals(timestamp: u64) -> [String; 9] {
    [
        "456".into(),
        "789".into(),
        timestamp.to_string(),
        "0".into(),
        "0".into(),
        "0".into(),
        "0".into(),
        "123".into(),
        "321".into(),
    ]
}

#[test]
fn absent_policy_is_disabled_and_partial_or_unpinned_configuration_fails() {
    let disabled = PinnedDocumentPolicy::from_pinned(None, None, "policy-tests").unwrap();
    assert!(!disabled.is_configured());
    assert_eq!(
        disabled.check_public_signals(DOCUMENT_CIRCUIT, &signals(NOW), NOW),
        Err(PolicyError::Disabled)
    );
    let bytes = bytes(&policy());
    let hash = pin(&bytes);
    for (document, digest) in [
        (Some(bytes.as_slice()), None),
        (None, Some(hash.as_str())),
        (Some(bytes.as_slice()), Some("00")),
    ] {
        assert!(PinnedDocumentPolicy::from_pinned(document, digest, "policy-tests").is_err());
    }
    assert!(matches!(
        PinnedDocumentPolicy::from_pinned(Some(&bytes), Some(&hash), "other-network"),
        Err(PolicyError::Network)
    ));
    let mut changed = bytes.clone();
    changed.push(b' ');
    assert!(matches!(
        PinnedDocumentPolicy::from_pinned(Some(&changed), Some(&hash), "policy-tests"),
        Err(PolicyError::PinMismatch)
    ));
    assert!(PinnedDocumentPolicy::from_pinned(
        Some(&changed),
        Some(&pin(&changed)),
        "policy-tests"
    )
    .is_err());
}

#[test]
fn test_policy_unknown_fields_duplicate_keys_and_oversize_are_rejected() {
    let mut test = policy();
    test.issuers[0].circuit_key_hash = SYNTHETIC_ISSUER.into();
    assert!(load(&test).is_err());
    let mut value = serde_json::to_value(policy()).unwrap();
    value["kind"] = "synthetic_diagnostic".into();
    let encoded = serde_json_canonicalizer::to_vec(&value).unwrap();
    assert!(PinnedDocumentPolicy::from_pinned(
        Some(&encoded),
        Some(&pin(&encoded)),
        "policy-tests"
    )
    .is_err());
    value["kind"] = "local_document_receipt".into();
    value["ignored_authority"] = true.into();
    let encoded = serde_json_canonicalizer::to_vec(&value).unwrap();
    assert!(PinnedDocumentPolicy::from_pinned(
        Some(&encoded),
        Some(&pin(&encoded)),
        "policy-tests"
    )
    .is_err());
    let bytes = bytes(&policy());
    let duplicated = String::from_utf8(bytes)
        .unwrap()
        .replacen('{', "{\"schema_version\":1,", 1)
        .into_bytes();
    assert!(PinnedDocumentPolicy::from_pinned(
        Some(&duplicated),
        Some(&pin(&duplicated)),
        "policy-tests"
    )
    .is_err());
    let oversized = vec![b' '; 32769];
    assert!(PinnedDocumentPolicy::from_pinned(
        Some(&oversized),
        Some(&pin(&oversized)),
        "policy-tests"
    )
    .is_err());
}

#[test]
fn policy_structure_rejects_duplicate_issuers_aliases_and_unreviewed_circuits() {
    let base = policy();
    let mut backend_source = base.clone();
    backend_source.issuers[0].certificate_source =
        "https://backend.uidai.gov.in/get/files/media/document/2026-07/policy-test.cer".into();
    assert!(load(&backend_source).is_ok());
    let mut duplicate = base.clone();
    duplicate.issuers.push(duplicate.issuers[0].clone());
    assert!(load(&duplicate).is_err());
    for field in [
        "circuit_id",
        "verification_key_sha256",
        "document_format",
        "nullifier_scope",
    ] {
        let mut json = serde_json::to_value(&base).unwrap();
        json[field] = "unsupported".into();
        assert!(
            load(&serde_json::from_value(json).unwrap()).is_err(),
            "{field}"
        );
    }
    for value in [
        "0",
        "01",
        "-1",
        "21888242871839275222246405745257275088548364400416034343698204186575808495617",
    ] {
        let mut changed = base.clone();
        changed.nullifier_seed = value.into();
        assert!(load(&changed).is_err());
    }
    for source in [
        "http://uidai.gov.in/key.cer",
        "https://uidai.gov.in.evil.test/key.cer",
        "https://uidai.gov.in@evil.test/key.cer",
        "https://uidai.gov.in/../key.cer",
        "https://uidai.gov.in/key.cer?redirect=x",
    ] {
        let mut changed = base.clone();
        changed.issuers[0].certificate_source = source.into();
        assert!(load(&changed).is_err());
    }
    for (field, value) in [
        ("max_document_age_seconds", 604801),
        ("max_future_skew_seconds", 301),
        ("challenge_ttl_seconds", 121),
        ("receipt_ttl_seconds", 86401),
    ] {
        let mut json = serde_json::to_value(&base).unwrap();
        json[field] = value.into();
        assert!(
            load(&serde_json::from_value(json).unwrap()).is_err(),
            "{field}"
        );
    }
}

#[test]
fn issuer_status_and_time_are_rechecked_without_grandfathering_old_documents() {
    let base = policy();
    let document = signals(NOW - 60);
    load(&base)
        .unwrap()
        .check_public_signals(DOCUMENT_CIRCUIT, &document, NOW)
        .unwrap();
    for status in [IssuerStatus::Revoked, IssuerStatus::Disabled] {
        let mut changed = base.clone();
        changed.issuers[0].status = status;
        assert_eq!(
            load(&changed)
                .unwrap()
                .check_public_signals(DOCUMENT_CIRCUIT, &document, NOW),
            Err(PolicyError::Issuer)
        );
    }
    let mut unknown = document.clone();
    unknown[0] = "999".into();
    assert_eq!(
        load(&base)
            .unwrap()
            .check_public_signals(DOCUMENT_CIRCUIT, &unknown, NOW),
        Err(PolicyError::Issuer)
    );
    let mut expired = base.clone();
    expired.issuers[0].valid_until = NOW;
    assert_eq!(
        load(&expired)
            .unwrap()
            .check_public_signals(DOCUMENT_CIRCUIT, &document, NOW),
        Err(PolicyError::Issuer)
    );
    let mut future = base.clone();
    future.issuers[0].valid_from = NOW + 1;
    assert_eq!(
        load(&future)
            .unwrap()
            .check_public_signals(DOCUMENT_CIRCUIT, &document, NOW),
        Err(PolicyError::Issuer)
    );
    for now in [base.valid_from - 1, base.valid_until] {
        assert_eq!(
            load(&base)
                .unwrap()
                .check_public_signals(DOCUMENT_CIRCUIT, &document, now),
            Err(PolicyError::PolicyExpired)
        );
    }
}

#[test]
fn freshness_boundaries_and_receipt_expiry_are_bounded_by_every_trust_window() {
    let base = policy();
    let loaded = load(&base).unwrap();
    assert_eq!(
        loaded
            .check_public_signals(DOCUMENT_CIRCUIT, &signals(NOW), NOW)
            .unwrap()
            .receipt_expires_at,
        NOW + 86400
    );
    assert!(loaded
        .check_public_signals(DOCUMENT_CIRCUIT, &signals(NOW + 300), NOW)
        .is_ok());
    for timestamp in [NOW + 301, NOW - 604800] {
        assert_eq!(
            loaded.check_public_signals(DOCUMENT_CIRCUIT, &signals(timestamp), NOW),
            Err(PolicyError::DocumentTime)
        );
    }
    assert_eq!(
        loaded
            .check_public_signals(DOCUMENT_CIRCUIT, &signals(NOW - 604799), NOW)
            .unwrap()
            .receipt_expires_at,
        NOW + 1
    );
    for narrow_policy in [true, false] {
        let mut limited = base.clone();
        if narrow_policy {
            limited.valid_until = NOW + 10;
        } else {
            limited.issuers[0].valid_until = NOW + 10;
        }
        assert_eq!(
            load(&limited)
                .unwrap()
                .check_public_signals(DOCUMENT_CIRCUIT, &signals(NOW), NOW)
                .unwrap()
                .receipt_expires_at,
            NOW + 10
        );
    }
}

#[test]
fn signal_policy_rejects_attributes_seed_circuit_and_noncanonical_fields() {
    let policy = load(&policy()).unwrap();
    for index in [3, 4, 5, 6, 7] {
        let mut changed = signals(NOW);
        changed[index] = "1".into();
        assert_eq!(
            policy.check_public_signals(DOCUMENT_CIRCUIT, &changed, NOW),
            Err(PolicyError::Circuit)
        );
    }
    for value in [
        "01",
        "-1",
        "21888242871839275222246405745257275088548364400416034343698204186575808495617",
    ] {
        let mut changed = signals(NOW);
        changed[1] = value.into();
        assert_eq!(
            policy.check_public_signals(DOCUMENT_CIRCUIT, &changed, NOW),
            Err(PolicyError::Circuit)
        );
    }
    assert_eq!(
        policy.check_public_signals("anon-aadhaar-v2.0.0-synthetic", &signals(NOW), NOW),
        Err(PolicyError::Circuit)
    );
}
