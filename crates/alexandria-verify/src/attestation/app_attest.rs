//! Apple App Attest verification (pure crypto, no network).
//!
//! Follows Apple's "Validating apps that connect to your server" steps for
//! the attestation object and for assertions:
//!
//! 1. the `x5c` chain (leaf, intermediate) verifies up to the App
//!    Attestation Root CA and is within its validity window;
//! 2. `nonce = SHA256(authData || clientDataHash)` equals the value in the
//!    leaf's `1.2.840.113635.100.8.2` extension;
//! 3. `SHA256(leaf public key)` equals the key id the app reported;
//! 4. `authData.rpIdHash == SHA256(appID)`;
//! 5. the sign-in counter is 0;
//! 6. the AAGUID names the production (or, when allowed, development)
//!    environment;
//! 7. the credential id equals the key id.
//!
//! An assertion is `{signature, authenticatorData}` where the signature is
//! ECDSA P-256 over `SHA256(authenticatorData || SHA256(clientData))` with
//! the attested key, `rpIdHash` matches, and the counter advanced.

use p256::ecdsa::signature::Verifier as _;
use p256::ecdsa::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use x509_parser::der_parser::oid::Oid;
use x509_parser::prelude::*;

use super::InvalidReason;

/// OID of the App Attest nonce extension.
const NONCE_EXT_OID: &str = "1.2.840.113635.100.8.2";
const AAGUID_PROD: &[u8; 16] = b"appattest\0\0\0\0\0\0\0";
const AAGUID_DEV: &[u8; 16] = b"appattestdevelop";

/// What a verified attestation object establishes about the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestedKey {
    /// Uncompressed SEC1 point (65 bytes) of the attested P-256 key.
    pub public_key_sec1: Vec<u8>,
    /// `authData` counter at attestation (always 0 on success).
    pub counter: u32,
    /// `appattest` or `appattestdevelop`.
    pub environment: String,
}

/// A failed check: the stable reason code and a human-readable detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub reason: InvalidReason,
    pub detail: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)
    }
}

fn fail(reason: InvalidReason, detail: impl Into<String>) -> Error {
    Error {
        reason,
        detail: detail.into(),
    }
}

fn err<T>(reason: InvalidReason, detail: impl Into<String>) -> Result<T, Error> {
    Err(fail(reason, detail))
}

fn malformed(detail: impl Into<String>) -> Error {
    fail(InvalidReason::MalformedEvidence, detail)
}

/// Strip PEM armour. Accepts exactly one certificate.
pub fn pem_to_der(pem: &str) -> Result<Vec<u8>, Error> {
    use base64::Engine as _;
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .map(str::trim)
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|e| {
            fail(
                InvalidReason::UntrustedChain,
                format!("root PEM is not base64: {e}"),
            )
        })
}

struct AttestationObject<'a> {
    x5c: Vec<&'a [u8]>,
    auth_data: &'a [u8],
}

/// Decode the CBOR attestation object: `{fmt, attStmt: {x5c, receipt}, authData}`.
fn parse_attestation(bytes: &[u8]) -> Result<AttestationObject<'_>, Error> {
    let mut d = minicbor::Decoder::new(bytes);
    let n = d
        .map()
        .map_err(|e| malformed(format!("attestation is not a CBOR map: {e}")))?
        .ok_or_else(|| malformed("indefinite attestation map"))?;
    let mut fmt = None;
    let mut x5c = Vec::new();
    let mut auth_data = None;
    for _ in 0..n {
        let key = d
            .str()
            .map_err(|e| malformed(format!("attestation key: {e}")))?;
        match key {
            "fmt" => fmt = Some(d.str().map_err(|e| malformed(format!("fmt: {e}")))?),
            "authData" => {
                auth_data = Some(d.bytes().map_err(|e| malformed(format!("authData: {e}")))?)
            }
            "attStmt" => {
                let m = d
                    .map()
                    .map_err(|e| malformed(format!("attStmt: {e}")))?
                    .ok_or_else(|| malformed("indefinite attStmt"))?;
                for _ in 0..m {
                    let k = d
                        .str()
                        .map_err(|e| malformed(format!("attStmt key: {e}")))?;
                    match k {
                        "x5c" => {
                            let c = d
                                .array()
                                .map_err(|e| malformed(format!("x5c: {e}")))?
                                .ok_or_else(|| malformed("indefinite x5c"))?;
                            for _ in 0..c {
                                x5c.push(
                                    d.bytes().map_err(|e| malformed(format!("x5c cert: {e}")))?,
                                );
                            }
                        }
                        _ => d
                            .skip()
                            .map_err(|e| malformed(format!("attStmt skip: {e}")))?,
                    }
                }
            }
            _ => d
                .skip()
                .map_err(|e| malformed(format!("attestation skip: {e}")))?,
        }
    }
    if fmt != Some("apple-appattest") {
        return Err(malformed(format!("unexpected attestation fmt {fmt:?}")));
    }
    let auth_data = auth_data.ok_or_else(|| malformed("attestation lacks authData"))?;
    if x5c.len() < 2 {
        return Err(malformed(
            "attestation x5c needs a leaf and an intermediate",
        ));
    }
    Ok(AttestationObject { x5c, auth_data })
}

/// Pull the 32-byte nonce out of the leaf's App Attest extension:
/// `SEQUENCE { [1] EXPLICIT OCTET STRING nonce }`.
fn extension_nonce(value: &[u8]) -> Result<[u8; 32], Error> {
    // 30 LL  A1 LL  04 20 <32 bytes>
    if value.len() < 38 || value[0] != 0x30 {
        return Err(malformed("nonce extension is not a SEQUENCE"));
    }
    let inner = &value[2..];
    if inner.first() != Some(&0xA1) {
        return Err(malformed("nonce extension lacks the [1] tag"));
    }
    let oct = &inner[2..];
    if oct.len() < 34 || oct[0] != 0x04 || oct[1] != 0x20 {
        return Err(malformed("nonce extension lacks a 32-byte OCTET STRING"));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&oct[2..34]);
    Ok(out)
}

/// Verify an attestation object. `client_data_hash` is the challenge the
/// app passed to `attestKey` (our session nonce, already 32 bytes).
/// `now_unix` is the instant certificate validity is judged at.
pub fn verify_attestation(
    attestation: &[u8],
    key_id: &[u8],
    client_data_hash: &[u8; 32],
    app_id_hash: &[u8; 32],
    root_pem: &str,
    allow_development: bool,
    now_unix: i64,
) -> Result<AttestedKey, Error> {
    use InvalidReason as R;
    let obj = parse_attestation(attestation)?;
    let root_der = pem_to_der(root_pem)?;
    let (_, root) = parse_x509_certificate(&root_der)
        .map_err(|e| fail(R::UntrustedChain, format!("root certificate: {e}")))?;
    let (_, leaf) = parse_x509_certificate(obj.x5c[0])
        .map_err(|e| malformed(format!("leaf certificate: {e}")))?;
    let (_, inter) = parse_x509_certificate(obj.x5c[1])
        .map_err(|e| malformed(format!("intermediate certificate: {e}")))?;

    // 1. Chain and validity.
    leaf.verify_signature(Some(inter.public_key()))
        .map_err(|e| {
            fail(
                R::UntrustedChain,
                format!("leaf is not signed by the intermediate: {e}"),
            )
        })?;
    inter
        .verify_signature(Some(root.public_key()))
        .map_err(|e| {
            fail(
                R::UntrustedChain,
                format!("intermediate is not signed by the root: {e}"),
            )
        })?;
    for (name, cert) in [("leaf", &leaf), ("intermediate", &inter)] {
        let v = cert.validity();
        if now_unix < v.not_before.timestamp() || now_unix > v.not_after.timestamp() {
            return err(
                R::UntrustedChain,
                format!("{name} certificate is outside its validity window"),
            );
        }
    }

    // 2. Nonce extension.
    let expected_nonce: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(obj.auth_data);
        h.update(client_data_hash);
        h.finalize().into()
    };
    let oid =
        Oid::from(&[1u64, 2, 840, 113635, 100, 8, 2]).map_err(|_| malformed("bad nonce OID"))?;
    debug_assert_eq!(oid.to_id_string(), NONCE_EXT_OID);
    let ext = leaf
        .get_extension_unique(&oid)
        .map_err(|e| malformed(format!("nonce extension lookup: {e}")))?
        .ok_or_else(|| malformed("leaf lacks the App Attest nonce extension"))?;
    if extension_nonce(ext.value)? != expected_nonce {
        return err(
            R::WrongNonce,
            "attestation nonce does not match authData and the challenge",
        );
    }

    // 3. Key id.
    let public_key_sec1 = leaf.public_key().subject_public_key.data.to_vec();
    if public_key_sec1.len() != 65 || public_key_sec1[0] != 0x04 {
        return Err(malformed(
            "leaf public key is not an uncompressed P-256 point",
        ));
    }
    let computed_key_id: [u8; 32] = Sha256::digest(&public_key_sec1).into();
    if computed_key_id[..] != key_id[..] {
        return err(
            R::KeyMismatch,
            "key id does not match the attested public key",
        );
    }

    // 4–7. authData.
    let ad = obj.auth_data;
    if ad.len() < 55 {
        return Err(malformed("authData too short"));
    }
    if ad[..32] != app_id_hash[..] {
        return err(R::WrongAppId, "authData rpIdHash is not our app id");
    }
    let counter = u32::from_be_bytes([ad[33], ad[34], ad[35], ad[36]]);
    if counter != 0 {
        return err(
            R::Counter,
            format!("attestation counter is {counter}, expected 0"),
        );
    }
    let aaguid = &ad[37..53];
    let environment = if aaguid == AAGUID_PROD {
        "appattest"
    } else if aaguid == AAGUID_DEV {
        if !allow_development {
            return err(
                R::EnvironmentNotAccepted,
                "development App Attest environment is not accepted here",
            );
        }
        "appattestdevelop"
    } else {
        return err(
            R::EnvironmentNotAccepted,
            "authData AAGUID is not an App Attest environment",
        );
    };
    let cred_len = u16::from_be_bytes([ad[53], ad[54]]) as usize;
    if ad.len() < 55 + cred_len {
        return Err(malformed("authData credential id is truncated"));
    }
    if &ad[55..55 + cred_len] != key_id {
        return err(R::KeyMismatch, "authData credential id is not the key id");
    }
    Ok(AttestedKey {
        public_key_sec1,
        counter,
        environment: environment.to_string(),
    })
}

/// Verify an assertion `{signature, authenticatorData}` over `client_data`
/// with the attested key. Returns the new counter, which must exceed
/// `previous_counter`.
pub fn verify_assertion(
    assertion: &[u8],
    client_data: &[u8],
    public_key_sec1: &[u8],
    app_id_hash: &[u8; 32],
    previous_counter: u32,
) -> Result<u32, Error> {
    let mut d = minicbor::Decoder::new(assertion);
    let n = d
        .map()
        .map_err(|e| malformed(format!("assertion is not a CBOR map: {e}")))?
        .ok_or_else(|| malformed("indefinite assertion map"))?;
    let mut signature = None;
    let mut auth_data = None;
    for _ in 0..n {
        let key = d
            .str()
            .map_err(|e| malformed(format!("assertion key: {e}")))?;
        match key {
            "signature" => {
                signature = Some(
                    d.bytes()
                        .map_err(|e| malformed(format!("signature: {e}")))?,
                )
            }
            "authenticatorData" => {
                auth_data = Some(
                    d.bytes()
                        .map_err(|e| malformed(format!("authenticatorData: {e}")))?,
                )
            }
            _ => d
                .skip()
                .map_err(|e| malformed(format!("assertion skip: {e}")))?,
        }
    }
    let signature = signature.ok_or_else(|| malformed("assertion lacks signature"))?;
    let auth_data = auth_data.ok_or_else(|| malformed("assertion lacks authenticatorData"))?;
    if auth_data.len() < 37 {
        return Err(malformed("assertion authenticatorData too short"));
    }
    if auth_data[..32] != app_id_hash[..] {
        return err(
            InvalidReason::WrongAppId,
            "assertion rpIdHash is not our app id",
        );
    }
    let client_data_hash: [u8; 32] = Sha256::digest(client_data).into();
    let nonce: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(auth_data);
        h.update(client_data_hash);
        h.finalize().into()
    };
    let key = VerifyingKey::from_sec1_bytes(public_key_sec1)
        .map_err(|e| malformed(format!("attested public key: {e}")))?;
    let sig = Signature::from_der(signature)
        .map_err(|e| malformed(format!("assertion signature is not DER ECDSA: {e}")))?;
    // ECDSA over the nonce (the verifier hashes it once more, as Apple's
    // reference verifiers do).
    key.verify(&nonce, &sig).map_err(|_| {
        fail(
            InvalidReason::InvalidSignature,
            "assertion signature does not verify",
        )
    })?;
    let counter = u32::from_be_bytes([auth_data[33], auth_data[34], auth_data[35], auth_data[36]]);
    if counter <= previous_counter {
        return err(
            InvalidReason::Counter,
            format!("assertion counter {counter} did not advance past {previous_counter}"),
        );
    }
    Ok(counter)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{build_assertion, build_world};
    use super::*;

    const APP: &str = "VLMNL3V44U.org.alexandria.node";

    #[test]
    fn a_well_formed_attestation_verifies() {
        let w = build_world(APP, [9u8; 32], AAGUID_PROD, 0);
        let k = verify_attestation(
            &w.attestation,
            &w.key_id,
            &w.client_data_hash,
            &w.app_id_hash,
            &w.root_pem,
            false,
            w.now,
        )
        .unwrap();
        assert_eq!(k.environment, "appattest");
        assert_eq!(k.counter, 0);
        assert_eq!(k.public_key_sec1.len(), 65);
    }

    #[test]
    fn the_wrong_challenge_fails_the_nonce_check() {
        let w = build_world(APP, [9u8; 32], AAGUID_PROD, 0);
        let e = verify_attestation(
            &w.attestation,
            &w.key_id,
            &[8u8; 32],
            &w.app_id_hash,
            &w.root_pem,
            false,
            w.now,
        )
        .unwrap_err();
        assert_eq!(e.reason, InvalidReason::WrongNonce, "{e}");
    }

    #[test]
    fn a_chain_to_a_different_root_is_refused() {
        let w = build_world(APP, [9u8; 32], AAGUID_PROD, 0);
        let other = build_world(APP, [9u8; 32], AAGUID_PROD, 0);
        let e = verify_attestation(
            &w.attestation,
            &w.key_id,
            &w.client_data_hash,
            &w.app_id_hash,
            &other.root_pem,
            false,
            w.now,
        )
        .unwrap_err();
        assert_eq!(e.reason, InvalidReason::UntrustedChain);
        assert!(
            e.detail.contains("intermediate is not signed by the root"),
            "{e}"
        );
    }

    #[test]
    fn the_real_apple_root_refuses_a_synthetic_chain() {
        let w = build_world(APP, [9u8; 32], AAGUID_PROD, 0);
        let e = verify_attestation(
            &w.attestation,
            &w.key_id,
            &w.client_data_hash,
            &w.app_id_hash,
            super::super::APPLE_APP_ATTEST_ROOT_PEM,
            false,
            w.now,
        )
        .unwrap_err();
        assert!(e.detail.contains("not signed by the root"), "{e}");
    }

    #[test]
    fn another_apps_attestation_is_refused() {
        let w = build_world("OTHERTEAM.com.example.other", [9u8; 32], AAGUID_PROD, 0);
        let ours: [u8; 32] = Sha256::digest(APP.as_bytes()).into();
        let e = verify_attestation(
            &w.attestation,
            &w.key_id,
            &w.client_data_hash,
            &ours,
            &w.root_pem,
            false,
            w.now,
        )
        .unwrap_err();
        assert_eq!(e.reason, InvalidReason::WrongAppId, "{e}");
    }

    #[test]
    fn development_environment_needs_opt_in() {
        let w = build_world(APP, [9u8; 32], AAGUID_DEV, 0);
        let refused = verify_attestation(
            &w.attestation,
            &w.key_id,
            &w.client_data_hash,
            &w.app_id_hash,
            &w.root_pem,
            false,
            w.now,
        );
        assert_eq!(
            refused.unwrap_err().reason,
            InvalidReason::EnvironmentNotAccepted
        );
        let ok = verify_attestation(
            &w.attestation,
            &w.key_id,
            &w.client_data_hash,
            &w.app_id_hash,
            &w.root_pem,
            true,
            w.now,
        )
        .unwrap();
        assert_eq!(ok.environment, "appattestdevelop");
    }

    #[test]
    fn a_nonzero_counter_a_wrong_key_id_and_an_expired_cert_fail() {
        let w = build_world(APP, [9u8; 32], AAGUID_PROD, 3);
        assert_eq!(
            verify_attestation(
                &w.attestation,
                &w.key_id,
                &w.client_data_hash,
                &w.app_id_hash,
                &w.root_pem,
                false,
                w.now,
            )
            .unwrap_err()
            .reason,
            InvalidReason::Counter
        );
        let w = build_world(APP, [9u8; 32], AAGUID_PROD, 0);
        assert_eq!(
            verify_attestation(
                &w.attestation,
                &[0u8; 32],
                &w.client_data_hash,
                &w.app_id_hash,
                &w.root_pem,
                false,
                w.now,
            )
            .unwrap_err()
            .reason,
            InvalidReason::KeyMismatch
        );
        assert!(verify_attestation(
            &w.attestation,
            &w.key_id,
            &w.client_data_hash,
            &w.app_id_hash,
            &w.root_pem,
            false,
            2_000_000_000, // 2033
        )
        .unwrap_err()
        .detail
        .contains("validity"));
    }

    #[test]
    fn assertions_verify_and_the_counter_must_advance() {
        let w = build_world(APP, [9u8; 32], AAGUID_PROD, 0);
        let attested = verify_attestation(
            &w.attestation,
            &w.key_id,
            &w.client_data_hash,
            &w.app_id_hash,
            &w.root_pem,
            false,
            w.now,
        )
        .unwrap();
        let root = b"commitment-root-hex";
        let assertion = build_assertion(&w, root, 1);
        assert_eq!(
            verify_assertion(
                &assertion,
                root,
                &attested.public_key_sec1,
                &w.app_id_hash,
                0
            )
            .unwrap(),
            1
        );
        // Replay with a stale counter.
        assert_eq!(
            verify_assertion(
                &assertion,
                root,
                &attested.public_key_sec1,
                &w.app_id_hash,
                1
            )
            .unwrap_err()
            .reason,
            InvalidReason::Counter
        );
        // Different data under the same signature.
        assert_eq!(
            verify_assertion(
                &assertion,
                b"other-root",
                &attested.public_key_sec1,
                &w.app_id_hash,
                0
            )
            .unwrap_err()
            .reason,
            InvalidReason::InvalidSignature
        );
        // Another key.
        let other = build_world(APP, [9u8; 32], AAGUID_PROD, 0);
        assert!(verify_assertion(
            &assertion,
            root,
            &other.public_key_sec1(),
            &w.app_id_hash,
            0
        )
        .is_err());
    }

    #[test]
    fn garbage_is_refused_without_panicking() {
        assert!(parse_attestation(b"").is_err());
        assert!(parse_attestation(&[0xA1, 0x63, b'f', b'm', b't', 0x60]).is_err());
        assert_eq!(
            verify_assertion(b"\xff", b"x", &[4u8; 65], &[0u8; 32], 0)
                .unwrap_err()
                .reason,
            InvalidReason::MalformedEvidence
        );
        assert!(extension_nonce(&[0x30, 0x00]).is_err());
    }
}
