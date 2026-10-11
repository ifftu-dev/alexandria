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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn err<T>(msg: impl Into<String>) -> Result<T, Error> {
    Err(Error(msg.into()))
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
        .map_err(|e| Error(format!("root PEM is not base64: {e}")))
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
        .map_err(|e| Error(format!("attestation is not a CBOR map: {e}")))?
        .ok_or_else(|| Error("indefinite attestation map".into()))?;
    let mut fmt = None;
    let mut x5c = Vec::new();
    let mut auth_data = None;
    for _ in 0..n {
        let key = d
            .str()
            .map_err(|e| Error(format!("attestation key: {e}")))?;
        match key {
            "fmt" => fmt = Some(d.str().map_err(|e| Error(format!("fmt: {e}")))?),
            "authData" => auth_data = Some(d.bytes().map_err(|e| Error(format!("authData: {e}")))?),
            "attStmt" => {
                let m = d
                    .map()
                    .map_err(|e| Error(format!("attStmt: {e}")))?
                    .ok_or_else(|| Error("indefinite attStmt".into()))?;
                for _ in 0..m {
                    let k = d.str().map_err(|e| Error(format!("attStmt key: {e}")))?;
                    match k {
                        "x5c" => {
                            let c = d
                                .array()
                                .map_err(|e| Error(format!("x5c: {e}")))?
                                .ok_or_else(|| Error("indefinite x5c".into()))?;
                            for _ in 0..c {
                                x5c.push(d.bytes().map_err(|e| Error(format!("x5c cert: {e}")))?);
                            }
                        }
                        _ => d.skip().map_err(|e| Error(format!("attStmt skip: {e}")))?,
                    }
                }
            }
            _ => d
                .skip()
                .map_err(|e| Error(format!("attestation skip: {e}")))?,
        }
    }
    if fmt != Some("apple-appattest") {
        return err(format!("unexpected attestation fmt {fmt:?}"));
    }
    let auth_data = auth_data.ok_or_else(|| Error("attestation lacks authData".into()))?;
    if x5c.len() < 2 {
        return err("attestation x5c needs a leaf and an intermediate");
    }
    Ok(AttestationObject { x5c, auth_data })
}

/// Pull the 32-byte nonce out of the leaf's App Attest extension:
/// `SEQUENCE { [1] EXPLICIT OCTET STRING nonce }`.
fn extension_nonce(value: &[u8]) -> Result<[u8; 32], Error> {
    // 30 LL  A1 LL  04 20 <32 bytes>
    if value.len() < 38 || value[0] != 0x30 {
        return err("nonce extension is not a SEQUENCE");
    }
    let inner = &value[2..];
    if inner.first() != Some(&0xA1) {
        return err("nonce extension lacks the [1] tag");
    }
    let oct = &inner[2..];
    if oct.len() < 34 || oct[0] != 0x04 || oct[1] != 0x20 {
        return err("nonce extension lacks a 32-byte OCTET STRING");
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&oct[2..34]);
    Ok(out)
}

/// Verify an attestation object. `client_data_hash` is the challenge the
/// app passed to `attestKey` (our session nonce, already 32 bytes).
pub fn verify_attestation(
    attestation: &[u8],
    key_id: &[u8],
    client_data_hash: &[u8; 32],
    app_id_hash: &[u8; 32],
    root_pem: &str,
    allow_development: bool,
    now_unix: i64,
) -> Result<AttestedKey, Error> {
    let obj = parse_attestation(attestation)?;
    let root_der = pem_to_der(root_pem)?;
    let (_, root) =
        parse_x509_certificate(&root_der).map_err(|e| Error(format!("root certificate: {e}")))?;
    let (_, leaf) =
        parse_x509_certificate(obj.x5c[0]).map_err(|e| Error(format!("leaf certificate: {e}")))?;
    let (_, inter) = parse_x509_certificate(obj.x5c[1])
        .map_err(|e| Error(format!("intermediate certificate: {e}")))?;

    // 1. Chain and validity.
    leaf.verify_signature(Some(inter.public_key()))
        .map_err(|e| Error(format!("leaf is not signed by the intermediate: {e}")))?;
    inter
        .verify_signature(Some(root.public_key()))
        .map_err(|e| Error(format!("intermediate is not signed by the root: {e}")))?;
    for (name, cert) in [("leaf", &leaf), ("intermediate", &inter)] {
        let v = cert.validity();
        if now_unix < v.not_before.timestamp() || now_unix > v.not_after.timestamp() {
            return err(format!("{name} certificate is outside its validity window"));
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
        Oid::from(&[1u64, 2, 840, 113635, 100, 8, 2]).map_err(|_| Error("bad nonce OID".into()))?;
    debug_assert_eq!(oid.to_id_string(), NONCE_EXT_OID);
    let ext = leaf
        .get_extension_unique(&oid)
        .map_err(|e| Error(format!("nonce extension lookup: {e}")))?
        .ok_or_else(|| Error("leaf lacks the App Attest nonce extension".into()))?;
    if extension_nonce(ext.value)? != expected_nonce {
        return err("attestation nonce does not match authData and the challenge");
    }

    // 3. Key id.
    let public_key_sec1 = leaf.public_key().subject_public_key.data.to_vec();
    if public_key_sec1.len() != 65 || public_key_sec1[0] != 0x04 {
        return err("leaf public key is not an uncompressed P-256 point");
    }
    let computed_key_id: [u8; 32] = Sha256::digest(&public_key_sec1).into();
    if computed_key_id[..] != key_id[..] {
        return err("key id does not match the attested public key");
    }

    // 4–7. authData.
    let ad = obj.auth_data;
    if ad.len() < 55 {
        return err("authData too short");
    }
    if ad[..32] != app_id_hash[..] {
        return err("authData rpIdHash is not our app id");
    }
    let counter = u32::from_be_bytes([ad[33], ad[34], ad[35], ad[36]]);
    if counter != 0 {
        return err(format!("attestation counter is {counter}, expected 0"));
    }
    let aaguid = &ad[37..53];
    let environment = if aaguid == AAGUID_PROD {
        "appattest"
    } else if aaguid == AAGUID_DEV {
        if !allow_development {
            return err("development App Attest environment is not accepted here");
        }
        "appattestdevelop"
    } else {
        return err("authData AAGUID is not an App Attest environment");
    };
    let cred_len = u16::from_be_bytes([ad[53], ad[54]]) as usize;
    if ad.len() < 55 + cred_len {
        return err("authData credential id is truncated");
    }
    if &ad[55..55 + cred_len] != key_id {
        return err("authData credential id is not the key id");
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
        .map_err(|e| Error(format!("assertion is not a CBOR map: {e}")))?
        .ok_or_else(|| Error("indefinite assertion map".into()))?;
    let mut signature = None;
    let mut auth_data = None;
    for _ in 0..n {
        let key = d.str().map_err(|e| Error(format!("assertion key: {e}")))?;
        match key {
            "signature" => {
                signature = Some(d.bytes().map_err(|e| Error(format!("signature: {e}")))?)
            }
            "authenticatorData" => {
                auth_data = Some(
                    d.bytes()
                        .map_err(|e| Error(format!("authenticatorData: {e}")))?,
                )
            }
            _ => d
                .skip()
                .map_err(|e| Error(format!("assertion skip: {e}")))?,
        }
    }
    let signature = signature.ok_or_else(|| Error("assertion lacks signature".into()))?;
    let auth_data = auth_data.ok_or_else(|| Error("assertion lacks authenticatorData".into()))?;
    if auth_data.len() < 37 {
        return err("assertion authenticatorData too short");
    }
    if auth_data[..32] != app_id_hash[..] {
        return err("assertion rpIdHash is not our app id");
    }
    let client_data_hash: [u8; 32] = Sha256::digest(client_data).into();
    let nonce: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(auth_data);
        h.update(client_data_hash);
        h.finalize().into()
    };
    let key = VerifyingKey::from_sec1_bytes(public_key_sec1)
        .map_err(|e| Error(format!("attested public key: {e}")))?;
    let sig = Signature::from_der(signature)
        .map_err(|e| Error(format!("assertion signature is not DER ECDSA: {e}")))?;
    // ECDSA over the nonce (the verifier hashes it once more, as Apple's
    // reference verifiers do).
    key.verify(&nonce, &sig)
        .map_err(|_| Error("assertion signature does not verify".into()))?;
    let counter = u32::from_be_bytes([auth_data[33], auth_data[34], auth_data[35], auth_data[36]]);
    if counter <= previous_counter {
        return err(format!(
            "assertion counter {counter} did not advance past {previous_counter}"
        ));
    }
    Ok(counter)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use p256::ecdsa::signature::Signer as _;
    use p256::ecdsa::SigningKey;
    use p256::pkcs8::DecodePrivateKey as _;
    use rcgen::{
        BasicConstraints, CertificateParams, CustomExtension, IsCa, KeyPair,
        PKCS_ECDSA_P256_SHA256, PKCS_ECDSA_P384_SHA384,
    };

    /// A synthetic App Attest world: root → intermediate → leaf with the
    /// nonce extension, plus authData built the way Apple does. Mirrors the
    /// real structure so the verifier's checks are exercised, not mocked.
    pub(crate) struct World {
        pub root_pem: String,
        pub key_id: [u8; 32],
        pub attestation: Vec<u8>,
        pub app_id_hash: [u8; 32],
        pub client_data_hash: [u8; 32],
        pub leaf_signing_key: SigningKey,
        pub now: i64,
    }

    fn der_seq(inner: &[u8]) -> Vec<u8> {
        let mut v = vec![0x30, inner.len() as u8];
        v.extend_from_slice(inner);
        v
    }

    fn nonce_extension(nonce: &[u8; 32]) -> Vec<u8> {
        let mut oct = vec![0x04, 0x20];
        oct.extend_from_slice(nonce);
        let mut tagged = vec![0xA1, oct.len() as u8];
        tagged.extend_from_slice(&oct);
        der_seq(&tagged)
    }

    fn cbor_bytes(out: &mut Vec<u8>, b: &[u8]) {
        let n = b.len();
        if n < 24 {
            out.push(0x40 | n as u8);
        } else if n < 256 {
            out.extend_from_slice(&[0x58, n as u8]);
        } else {
            out.extend_from_slice(&[0x59, (n >> 8) as u8, n as u8]);
        }
        out.extend_from_slice(b);
    }

    fn cbor_text(out: &mut Vec<u8>, s: &str) {
        out.push(0x60 | s.len() as u8);
        out.extend_from_slice(s.as_bytes());
    }

    pub(crate) fn build_world(
        app_id: &str,
        client_data_hash: [u8; 32],
        aaguid: &[u8; 16],
        counter: u32,
    ) -> World {
        let now = 1_760_000_000i64; // 2025-10-09
        let valid_from = rcgen::date_time_ymd(2025, 1, 1);
        let valid_to = rcgen::date_time_ymd(2030, 1, 1);

        let root_key = KeyPair::generate_for(&PKCS_ECDSA_P384_SHA384).unwrap();
        let mut root_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        root_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        root_params.not_before = valid_from;
        root_params.not_after = valid_to;
        let root = root_params.self_signed(&root_key).unwrap();

        let inter_key = KeyPair::generate_for(&PKCS_ECDSA_P384_SHA384).unwrap();
        let mut inter_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        inter_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        inter_params.not_before = valid_from;
        inter_params.not_after = valid_to;
        let inter = inter_params
            .signed_by(&inter_key, &root, &root_key)
            .unwrap();

        let leaf_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let leaf_signing_key = SigningKey::from_pkcs8_der(&leaf_key.serialize_der()).unwrap();
        let public_key_sec1 = leaf_signing_key
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();
        let key_id: [u8; 32] = Sha256::digest(&public_key_sec1).into();
        let app_id_hash: [u8; 32] = Sha256::digest(app_id.as_bytes()).into();

        let mut auth_data = Vec::new();
        auth_data.extend_from_slice(&app_id_hash);
        auth_data.push(0x41); // flags: UP + AT
        auth_data.extend_from_slice(&counter.to_be_bytes());
        auth_data.extend_from_slice(aaguid);
        auth_data.extend_from_slice(&(key_id.len() as u16).to_be_bytes());
        auth_data.extend_from_slice(&key_id);
        // Apple appends a COSE key; the verifier does not read past the
        // credential id, so a stub suffices.
        auth_data.extend_from_slice(&[0xA0]);

        let nonce: [u8; 32] = {
            let mut h = Sha256::new();
            h.update(&auth_data);
            h.update(client_data_hash);
            h.finalize().into()
        };
        let mut leaf_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        leaf_params.not_before = valid_from;
        leaf_params.not_after = valid_to;
        leaf_params
            .custom_extensions
            .push(CustomExtension::from_oid_content(
                &[1, 2, 840, 113635, 100, 8, 2],
                nonce_extension(&nonce),
            ));
        let leaf = leaf_params
            .signed_by(&leaf_key, &inter, &inter_key)
            .unwrap();

        // CBOR: {"fmt": "apple-appattest", "attStmt": {"x5c": [leaf, inter], "receipt": b""}, "authData": bytes}
        let mut att = vec![0xA3];
        cbor_text(&mut att, "fmt");
        cbor_text(&mut att, "apple-appattest");
        cbor_text(&mut att, "attStmt");
        att.push(0xA2);
        cbor_text(&mut att, "x5c");
        att.push(0x82);
        cbor_bytes(&mut att, leaf.der());
        cbor_bytes(&mut att, inter.der());
        cbor_text(&mut att, "receipt");
        cbor_bytes(&mut att, b"");
        cbor_text(&mut att, "authData");
        cbor_bytes(&mut att, &auth_data);

        World {
            root_pem: root.pem(),
            key_id,
            attestation: att,
            app_id_hash,
            client_data_hash,
            leaf_signing_key,
            now,
        }
    }

    /// Build an assertion the way `generateAssertion` does for `client_data`.
    pub(crate) fn build_assertion(w: &World, client_data: &[u8], counter: u32) -> Vec<u8> {
        let mut auth_data = Vec::new();
        auth_data.extend_from_slice(&w.app_id_hash);
        auth_data.push(0x40);
        auth_data.extend_from_slice(&counter.to_be_bytes());
        let client_data_hash: [u8; 32] = Sha256::digest(client_data).into();
        let nonce: [u8; 32] = {
            let mut h = Sha256::new();
            h.update(&auth_data);
            h.update(client_data_hash);
            h.finalize().into()
        };
        let sig: Signature = w.leaf_signing_key.sign(&nonce);
        let der = sig.to_der();
        let mut out = vec![0xA2];
        cbor_text(&mut out, "signature");
        cbor_bytes(&mut out, der.as_bytes());
        cbor_text(&mut out, "authenticatorData");
        cbor_bytes(&mut out, &auth_data);
        out
    }

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
        assert!(e.0.contains("nonce"), "{e}");
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
        assert!(
            e.0.contains("intermediate is not signed by the root"),
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
        assert!(e.0.contains("not signed by the root"), "{e}");
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
        assert!(e.0.contains("rpIdHash"), "{e}");
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
        assert!(refused.is_err());
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
        assert!(verify_attestation(
            &w.attestation,
            &w.key_id,
            &w.client_data_hash,
            &w.app_id_hash,
            &w.root_pem,
            false,
            w.now,
        )
        .unwrap_err()
        .0
        .contains("counter"));
        let w = build_world(APP, [9u8; 32], AAGUID_PROD, 0);
        assert!(verify_attestation(
            &w.attestation,
            &[0u8; 32],
            &w.client_data_hash,
            &w.app_id_hash,
            &w.root_pem,
            false,
            w.now,
        )
        .unwrap_err()
        .0
        .contains("key id"));
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
        .0
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
        assert!(verify_assertion(
            &assertion,
            root,
            &attested.public_key_sec1,
            &w.app_id_hash,
            1
        )
        .is_err());
        // Different data under the same signature.
        assert!(verify_assertion(
            &assertion,
            b"other-root",
            &attested.public_key_sec1,
            &w.app_id_hash,
            0
        )
        .is_err());
        // Another key.
        let other = build_world(APP, [9u8; 32], AAGUID_PROD, 0);
        let other_pk = other
            .leaf_signing_key
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec();
        assert!(verify_assertion(&assertion, root, &other_pk, &w.app_id_hash, 0).is_err());
    }

    #[test]
    fn garbage_is_refused_without_panicking() {
        assert!(parse_attestation(b"").is_err());
        assert!(parse_attestation(&[0xA1, 0x63, b'f', b'm', b't', 0x60]).is_err());
        assert!(verify_assertion(b"\xff", b"x", &[4u8; 65], &[0u8; 32], 0).is_err());
        assert!(extension_nonce(&[0x30, 0x00]).is_err());
    }
}
