//! Google Play Integrity token decryption and verdict checks.
//!
//! A classic-request token is a compact JWE (`A256KW` key wrap, `A256GCM`
//! content encryption) whose plaintext is a compact JWS (`ES256`) carrying
//! the verdict JSON. Google offers two ways to open it: a server call that
//! needs a Google Cloud project, or the developer's own response keys from
//! the Play Console. Alexandria uses the keys, supplied by the caller as a
//! trust input, so any verifier holding them can check a token without a
//! service in the middle; a verifier without them reports the token as
//! unverifiable.
//!
//! Verdict rules: the nonce is the expected one, the package is the trusted
//! one, Play recognises the app, and the device meets at least
//! `MEETS_DEVICE_INTEGRITY`.

use aes::cipher::{BlockDecrypt, KeyInit};
use aes::Aes256;
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use p256::ecdsa::signature::Verifier as _;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey as _;
use serde_json::Value;

use super::{InvalidReason, PlayIntegrityKeys};

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

fn malformed(detail: impl Into<String>) -> Error {
    fail(InvalidReason::MalformedEvidence, detail)
}

/// Decryption or signature failure: the token was not made for these keys.
fn unauthentic(detail: impl Into<String>) -> Error {
    fail(InvalidReason::InvalidSignature, detail)
}

fn b64url(s: &str) -> Result<Vec<u8>, Error> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s)
        .map_err(|e| malformed(format!("base64url: {e}")))
}

fn b64std(s: &str) -> Result<Vec<u8>, Error> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s.trim()))
        .map_err(|e| unauthentic(format!("response key is not base64: {e}")))
}

/// RFC 3394 AES Key Unwrap with the default IV. `wrapped` is 8 bytes longer
/// than the key it protects.
pub fn aes_key_unwrap(kek: &[u8; 32], wrapped: &[u8]) -> Result<Vec<u8>, Error> {
    if wrapped.len() < 24 || wrapped.len() % 8 != 0 {
        return Err(malformed("wrapped key has an invalid length"));
    }
    let n = wrapped.len() / 8 - 1;
    let cipher = Aes256::new(kek.into());
    let mut a = [0u8; 8];
    a.copy_from_slice(&wrapped[..8]);
    let mut r: Vec<[u8; 8]> = wrapped[8..]
        .chunks(8)
        .map(|c| {
            let mut b = [0u8; 8];
            b.copy_from_slice(c);
            b
        })
        .collect();
    for j in (0..6).rev() {
        for i in (1..=n).rev() {
            let t = (n * j + i) as u64;
            let mut block = [0u8; 16];
            let a_xor = u64::from_be_bytes(a) ^ t;
            block[..8].copy_from_slice(&a_xor.to_be_bytes());
            block[8..].copy_from_slice(&r[i - 1]);
            let mut ga = aes::Block::clone_from_slice(&block);
            cipher.decrypt_block(&mut ga);
            a.copy_from_slice(&ga[..8]);
            r[i - 1].copy_from_slice(&ga[8..]);
        }
    }
    if a != [0xA6u8; 8] {
        return Err(unauthentic("wrapped key integrity check failed"));
    }
    Ok(r.concat())
}

/// Decrypt the JWE and verify the inner JWS; returns the verdict payload.
pub fn decrypt_and_verify(token: &str, keys: &PlayIntegrityKeys) -> Result<Value, Error> {
    let parts: Vec<&str> = token.trim().split('.').collect();
    if parts.len() != 5 {
        return Err(malformed("token is not a compact JWE"));
    }
    let header: Value = serde_json::from_slice(&b64url(parts[0])?)
        .map_err(|e| malformed(format!("JWE header: {e}")))?;
    if header.get("alg").and_then(Value::as_str) != Some("A256KW")
        || header.get("enc").and_then(Value::as_str) != Some("A256GCM")
    {
        return Err(malformed("JWE is not A256KW / A256GCM"));
    }
    let kek_bytes = b64std(&keys.decryption_key_b64)?;
    let kek: [u8; 32] = kek_bytes
        .as_slice()
        .try_into()
        .map_err(|_| unauthentic("decryption key is not 32 bytes"))?;
    let cek = aes_key_unwrap(&kek, &b64url(parts[1])?)?;
    let cek: [u8; 32] = cek
        .as_slice()
        .try_into()
        .map_err(|_| malformed("content key is not 32 bytes"))?;
    let iv = b64url(parts[2])?;
    if iv.len() != 12 {
        return Err(malformed("JWE IV is not 96 bits"));
    }
    let mut ciphertext = b64url(parts[3])?;
    ciphertext.extend_from_slice(&b64url(parts[4])?);
    let plaintext = Aes256Gcm::new(&cek.into())
        .decrypt(
            Nonce::from_slice(&iv),
            Payload {
                msg: &ciphertext,
                aad: parts[0].as_bytes(),
            },
        )
        .map_err(|_| unauthentic("JWE authentication failed"))?;
    let jws = std::str::from_utf8(&plaintext).map_err(|_| malformed("JWS is not UTF-8"))?;
    verify_jws(jws, &keys.verification_key_b64)
}

/// Verify a compact ES256 JWS with the given SPKI (DER, base64) key.
fn verify_jws(jws: &str, verification_key_b64: &str) -> Result<Value, Error> {
    let parts: Vec<&str> = jws.split('.').collect();
    if parts.len() != 3 {
        return Err(malformed("inner token is not a compact JWS"));
    }
    let header: Value = serde_json::from_slice(&b64url(parts[0])?)
        .map_err(|e| malformed(format!("JWS header: {e}")))?;
    if header.get("alg").and_then(Value::as_str) != Some("ES256") {
        return Err(malformed("JWS is not ES256"));
    }
    let key_der = b64std(verification_key_b64)?;
    let key = VerifyingKey::from_public_key_der(&key_der)
        .map_err(|e| unauthentic(format!("verification key: {e}")))?;
    let sig = Signature::from_slice(&b64url(parts[2])?)
        .map_err(|e| malformed(format!("JWS signature: {e}")))?;
    let signing_input = format!("{}.{}", parts[0], parts[1]);
    key.verify(signing_input.as_bytes(), &sig)
        .map_err(|_| unauthentic("JWS signature does not verify"))?;
    serde_json::from_slice(&b64url(parts[1])?).map_err(|e| malformed(format!("verdict JSON: {e}")))
}

/// Device verdict labels (`MEETS_DEVICE_INTEGRITY`, …).
pub fn device_labels(verdict: &Value) -> Vec<String> {
    verdict
        .pointer("/deviceIntegrity/deviceRecognitionVerdict")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Apply the verdict rules against the expected nonce and package.
pub fn check_verdict(
    verdict: &Value,
    expected_nonce: &[u8; 32],
    package: &str,
) -> Result<(), Error> {
    use InvalidReason as R;
    let nonce = verdict
        .pointer("/requestDetails/nonce")
        .and_then(Value::as_str)
        .ok_or_else(|| malformed("verdict lacks requestDetails.nonce"))?;
    let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(expected_nonce);
    if nonce.trim_end_matches('=') != expected {
        return Err(fail(R::WrongNonce, "verdict nonce is not the expected one"));
    }
    let req_pkg = verdict
        .pointer("/requestDetails/requestPackageName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if req_pkg != package {
        return Err(fail(
            R::WrongAppId,
            format!("verdict is for package {req_pkg:?}"),
        ));
    }
    let app_verdict = verdict
        .pointer("/appIntegrity/appRecognitionVerdict")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if app_verdict != "PLAY_RECOGNIZED" {
        return Err(fail(
            R::DeviceIntegrity,
            format!("app is not Play-recognised ({app_verdict:?})"),
        ));
    }
    let app_pkg = verdict
        .pointer("/appIntegrity/packageName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if app_pkg != package {
        return Err(fail(
            R::WrongAppId,
            format!("app verdict names package {app_pkg:?}"),
        ));
    }
    let labels = device_labels(verdict);
    if !labels.iter().any(|l| l == "MEETS_DEVICE_INTEGRITY") {
        return Err(fail(
            R::DeviceIntegrity,
            format!("device integrity not met ({labels:?})"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{
        aes_key_wrap, mint_play_token, p256_key, play_keys_b64, play_verdict,
    };
    use super::*;
    use p256::ecdsa::SigningKey;
    use serde_json::json;

    const PKG: &str = "org.alexandria.node";

    fn b64u(b: &[u8]) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
    }

    fn mint(verdict: &Value, kek: &[u8; 32], signer: &SigningKey) -> String {
        mint_play_token(verdict, kek, signer, [0x11; 12])
    }

    fn keys(kek: &[u8; 32], signer: &SigningKey) -> PlayIntegrityKeys {
        let (decryption_key_b64, verification_key_b64) = play_keys_b64(kek, signer);
        PlayIntegrityKeys {
            decryption_key_b64,
            verification_key_b64,
        }
    }

    fn good_verdict(nonce: &[u8; 32]) -> Value {
        play_verdict(nonce, PKG)
    }

    #[test]
    fn key_wrap_round_trips_and_detects_tampering() {
        let kek = [3u8; 32];
        let key = [9u8; 32];
        let wrapped = aes_key_wrap(&kek, &key);
        assert_eq!(wrapped.len(), 40);
        assert_eq!(aes_key_unwrap(&kek, &wrapped).unwrap(), key);
        let mut bad = wrapped.clone();
        bad[20] ^= 1;
        assert!(aes_key_unwrap(&kek, &bad).is_err());
        assert!(aes_key_unwrap(&[4u8; 32], &wrapped).is_err());
    }

    #[test]
    fn a_minted_token_decrypts_verifies_and_passes_the_verdict_rules() {
        let kek = [5u8; 32];
        let signer = p256_key(b"signer");
        let nonce = [1u8; 32];
        let token = mint(&good_verdict(&nonce), &kek, &signer);
        let verdict = decrypt_and_verify(&token, &keys(&kek, &signer)).unwrap();
        check_verdict(&verdict, &nonce, PKG).unwrap();
        assert_eq!(device_labels(&verdict)[0], "MEETS_DEVICE_INTEGRITY");
    }

    #[test]
    fn the_wrong_decryption_or_verification_key_fails() {
        let kek = [5u8; 32];
        let signer = p256_key(b"signer");
        let token = mint(&good_verdict(&[1u8; 32]), &kek, &signer);
        let other_signer = p256_key(b"other signer");
        assert_eq!(
            decrypt_and_verify(&token, &keys(&[6u8; 32], &signer))
                .unwrap_err()
                .reason,
            InvalidReason::InvalidSignature
        );
        let e = decrypt_and_verify(&token, &keys(&kek, &other_signer)).unwrap_err();
        assert_eq!(e.reason, InvalidReason::InvalidSignature);
        assert!(e.detail.contains("signature"));
    }

    #[test]
    fn verdict_rules_reject_nonce_package_app_and_device_failures() {
        let nonce = [1u8; 32];
        let mut v = good_verdict(&nonce);
        assert_eq!(
            check_verdict(&v, &[2u8; 32], PKG).unwrap_err().reason,
            InvalidReason::WrongNonce
        );
        assert_eq!(
            check_verdict(&v, &nonce, "com.example.other")
                .unwrap_err()
                .reason,
            InvalidReason::WrongAppId
        );
        v["appIntegrity"]["appRecognitionVerdict"] = json!("UNRECOGNIZED_VERSION");
        assert!(check_verdict(&v, &nonce, PKG)
            .unwrap_err()
            .detail
            .contains("Play-recognised"));
        let mut v = good_verdict(&nonce);
        v["deviceIntegrity"]["deviceRecognitionVerdict"] = json!(["MEETS_BASIC_INTEGRITY"]);
        let e = check_verdict(&v, &nonce, PKG).unwrap_err();
        assert_eq!(e.reason, InvalidReason::DeviceIntegrity);
        assert!(e.detail.contains("device integrity"));
        let mut v = good_verdict(&nonce);
        v["appIntegrity"]["packageName"] = json!("com.example.spoof");
        assert!(check_verdict(&v, &nonce, PKG).is_err());
    }

    #[test]
    fn malformed_tokens_are_refused() {
        let k = keys(&[5u8; 32], &p256_key(b"signer"));
        assert!(decrypt_and_verify("a.b.c", &k).is_err());
        assert!(decrypt_and_verify("!!.b.c.d.e", &k).is_err());
        let header = b64u(br#"{"alg":"RSA-OAEP","enc":"A256GCM"}"#);
        let e = decrypt_and_verify(&format!("{header}.a.b.c.d"), &k).unwrap_err();
        assert_eq!(e.reason, InvalidReason::MalformedEvidence);
        assert!(e.detail.contains("A256KW"));
    }
}
