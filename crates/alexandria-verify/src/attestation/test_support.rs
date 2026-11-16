//! Synthetic attestation fixtures for tests: an App Attest certificate chain
//! with the nonce extension and `authData` built the way Apple builds them,
//! assertions from the attested key, and Play Integrity tokens minted the way
//! Google mints them under caller-chosen response keys. The verifier's real
//! checks run against them; nothing is mocked.
//!
//! Everything is deterministic for a given seed (ECDSA signatures are RFC 6979,
//! serial numbers derive from the public key, the JWE IV is supplied), so
//! vectors built from these regenerate byte for byte.
//!
//! This file names no `crate::` path: the crate's own vector tests include it
//! with `#[path]`, building it from dev-dependencies whether or not the
//! `attestation` feature is on. Available to other crates under the
//! `test-support` feature. Never ship it.

use aes::cipher::{BlockEncrypt as _, KeyInit as _};
use aes::Aes256;
use aes_gcm::aead::{Aead as _, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{Signature, SigningKey};
use p256::pkcs8::EncodePublicKey as _;
use rcgen::{
    BasicConstraints, CertificateParams, CustomExtension, IsCa, KeyPair, RemoteKeyPair,
    PKCS_ECDSA_P256_SHA256,
};
use sha2::{Digest, Sha256};

/// AAGUID of the production App Attest environment.
pub const AAGUID_PROD: &[u8; 16] = b"appattest\0\0\0\0\0\0\0";
/// AAGUID of the development App Attest environment.
pub const AAGUID_DEV: &[u8; 16] = b"appattestdevelop";
/// Inside every fixture certificate's validity (2025-01-01 to 2030-01-01):
/// 2025-10-09.
pub const FIXTURE_NOW: i64 = 1_760_000_000;

/// A synthetic App Attest world: root → intermediate → leaf with the nonce
/// extension, and the attestation object over `client_data_hash`.
pub struct World {
    /// PEM of the synthetic root, supplied as the Apple root trust input.
    pub root_pem: String,
    pub key_id: [u8; 32],
    /// CBOR attestation object.
    pub attestation: Vec<u8>,
    pub app_id_hash: [u8; 32],
    pub client_data_hash: [u8; 32],
    pub leaf_signing_key: SigningKey,
    /// A Unix time inside every certificate's validity.
    pub now: i64,
}

impl World {
    /// Standard base64 of the key id, as stored.
    pub fn key_id_b64(&self) -> String {
        b64(&self.key_id)
    }

    /// Standard base64 of the attestation object, as stored.
    pub fn attestation_b64(&self) -> String {
        b64(&self.attestation)
    }

    /// Uncompressed SEC1 point of the attested key.
    pub fn public_key_sec1(&self) -> Vec<u8> {
        self.leaf_signing_key
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec()
    }
}

/// Standard base64.
pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn b64u(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// A P-256 signing key derived from `label`. Published test material.
pub fn p256_key(label: &[u8]) -> SigningKey {
    let mut counter = 0u8;
    loop {
        let mut h = Sha256::new();
        h.update(b"alexandria-test-p256\0");
        h.update(label);
        h.update([counter]);
        if let Ok(key) = SigningKey::from_slice(&h.finalize()) {
            return key;
        }
        counter += 1;
    }
}

/// rcgen signs through this so certificates are deterministic.
struct DeterministicKey {
    key: SigningKey,
    public: Vec<u8>,
}

impl RemoteKeyPair for DeterministicKey {
    fn public_key(&self) -> &[u8] {
        &self.public
    }

    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, rcgen::Error> {
        let sig: Signature = self.key.sign(msg);
        Ok(sig.to_der().as_bytes().to_vec())
    }

    fn algorithm(&self) -> &'static rcgen::SignatureAlgorithm {
        &PKCS_ECDSA_P256_SHA256
    }
}

fn key_pair(key: &SigningKey) -> KeyPair {
    let public = key
        .verifying_key()
        .to_encoded_point(false)
        .as_bytes()
        .to_vec();
    KeyPair::from_remote(Box::new(DeterministicKey {
        key: key.clone(),
        public,
    }))
    .expect("remote key pair")
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

/// A world with fresh keys on every call (distinct roots for two calls with
/// the same arguments). Use [`build_world_seeded`] where output must repeat.
pub fn build_world(
    app_id: &str,
    client_data_hash: [u8; 32],
    aaguid: &[u8; 16],
    counter: u32,
) -> World {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let seed = NEXT.fetch_add(1, Ordering::Relaxed);
    build_world_seeded(
        &seed.to_be_bytes(),
        app_id,
        client_data_hash,
        aaguid,
        counter,
    )
}

/// A world whose keys, certificates and attestation derive from `seed`.
pub fn build_world_seeded(
    seed: &[u8],
    app_id: &str,
    client_data_hash: [u8; 32],
    aaguid: &[u8; 16],
    counter: u32,
) -> World {
    let labelled = |role: &str| {
        let mut l = seed.to_vec();
        l.extend_from_slice(role.as_bytes());
        p256_key(&l)
    };
    let valid_from = rcgen::date_time_ymd(2025, 1, 1);
    let valid_to = rcgen::date_time_ymd(2030, 1, 1);

    let root_key = key_pair(&labelled("root"));
    let mut root_params = CertificateParams::new(Vec::<String>::new()).expect("params");
    root_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    root_params.not_before = valid_from;
    root_params.not_after = valid_to;
    let root = root_params.self_signed(&root_key).expect("root");

    let inter_key = key_pair(&labelled("intermediate"));
    let mut inter_params = CertificateParams::new(Vec::<String>::new()).expect("params");
    inter_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    inter_params.not_before = valid_from;
    inter_params.not_after = valid_to;
    let inter = inter_params
        .signed_by(&inter_key, &root, &root_key)
        .expect("intermediate");

    let leaf_signing_key = labelled("leaf");
    let leaf_key = key_pair(&leaf_signing_key);
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
    let mut leaf_params = CertificateParams::new(Vec::<String>::new()).expect("params");
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
        .expect("leaf");

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
        now: FIXTURE_NOW,
    }
}

/// An assertion over `client_data` the way `generateAssertion` builds one.
pub fn build_assertion(w: &World, client_data: &[u8], counter: u32) -> Vec<u8> {
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

/// RFC 3394 AES key wrap with the default IV.
pub fn aes_key_wrap(kek: &[u8; 32], key: &[u8]) -> Vec<u8> {
    let n = key.len() / 8;
    let cipher = Aes256::new(kek.into());
    let mut a = [0xA6u8; 8];
    let mut r: Vec<[u8; 8]> = key
        .chunks(8)
        .map(|c| {
            let mut b = [0u8; 8];
            b.copy_from_slice(c);
            b
        })
        .collect();
    for j in 0..6 {
        for i in 1..=n {
            let mut block = [0u8; 16];
            block[..8].copy_from_slice(&a);
            block[8..].copy_from_slice(&r[i - 1]);
            let mut ga = aes::Block::clone_from_slice(&block);
            cipher.encrypt_block(&mut ga);
            let t = (n * j + i) as u64;
            let a_val = u64::from_be_bytes(ga[..8].try_into().expect("8 bytes")) ^ t;
            a.copy_from_slice(&a_val.to_be_bytes());
            r[i - 1].copy_from_slice(&ga[8..]);
        }
    }
    let mut out = a.to_vec();
    out.extend(r.concat());
    out
}

/// Play Integrity response keys as the Play Console hands them out:
/// `(decryption_key_b64, verification_key_b64)`.
pub fn play_keys_b64(kek: &[u8; 32], signer: &SigningKey) -> (String, String) {
    (
        b64(kek),
        b64(signer
            .verifying_key()
            .to_public_key_der()
            .expect("spki")
            .as_bytes()),
    )
}

/// A verdict that passes every rule for `nonce` and `package`.
pub fn play_verdict(nonce: &[u8; 32], package: &str) -> serde_json::Value {
    serde_json::json!({
        "requestDetails": {
            "requestPackageName": package,
            "nonce": b64u(nonce),
            "timestampMillis": "1760000000000"
        },
        "appIntegrity": {
            "appRecognitionVerdict": "PLAY_RECOGNIZED",
            "packageName": package,
            "certificateSha256Digest": ["abc"],
            "versionCode": "61"
        },
        "deviceIntegrity": {
            "deviceRecognitionVerdict": ["MEETS_DEVICE_INTEGRITY", "MEETS_BASIC_INTEGRITY"]
        },
        "accountDetails": { "appLicensingVerdict": "LICENSED" }
    })
}

/// Mint a token the way Google does: an ES256 JWS of `verdict`, encrypted
/// as an A256KW / A256GCM JWE under `kek`. `iv` makes the output repeatable.
pub fn mint_play_token(
    verdict: &serde_json::Value,
    kek: &[u8; 32],
    signer: &SigningKey,
    iv: [u8; 12],
) -> String {
    let jws_header = b64u(br#"{"alg":"ES256"}"#);
    let payload = b64u(serde_json::to_vec(verdict).expect("verdict").as_slice());
    let signing_input = format!("{jws_header}.{payload}");
    let sig: Signature = signer.sign(signing_input.as_bytes());
    let jws = format!("{signing_input}.{}", b64u(&sig.to_bytes()));

    let jwe_header = b64u(br#"{"alg":"A256KW","enc":"A256GCM"}"#);
    let cek = [0x42u8; 32];
    let wrapped = aes_key_wrap(kek, &cek);
    let ct = Aes256Gcm::new(&cek.into())
        .encrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: jws.as_bytes(),
                aad: jwe_header.as_bytes(),
            },
        )
        .expect("encrypt");
    let (body, tag) = ct.split_at(ct.len() - 16);
    format!(
        "{jwe_header}.{}.{}.{}.{}",
        b64u(&wrapped),
        b64u(&iv),
        b64u(body),
        b64u(tag)
    )
}
