//! OpenID Connect ID-token verification for Sign in with ChatGPT.
//!
//! Only RS256 is accepted. The signature is checked against the provider's
//! published JWKS, then the issuer, audience, expiry, and nonce, as OpenAI's
//! sign-in guide requires before any identity is trusted.

use base64::Engine;
use serde_json::Value;

/// Allowed clock difference, in seconds.
const SKEW: i64 = 120;

/// The verified identity in an ID token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub sub: String,
    pub email: Option<String>,
}

fn b64(part: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(part.trim_end_matches('='))
        .map_err(|_| "the ID token is malformed".to_string())
}

fn json(part: &str) -> Result<Value, String> {
    serde_json::from_slice(&b64(part)?).map_err(|_| "the ID token is malformed".to_string())
}

/// Verifies `token` against `jwks` and returns its identity.
pub fn verify(
    token: &str,
    jwks: &Value,
    issuer: &str,
    audience: &str,
    nonce: &str,
    now: i64,
) -> Result<Identity, String> {
    let parts: Vec<&str> = token.split('.').collect();
    let [header, payload, signature] = parts[..] else {
        return Err("the ID token is malformed".into());
    };
    let header = json(header)?;
    if header.get("alg").and_then(Value::as_str) != Some("RS256") {
        return Err("the ID token uses an unexpected algorithm".into());
    }
    let kid = header.get("kid").and_then(Value::as_str);
    let key = jwks
        .get("keys")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|k| {
            k.get("kty").and_then(Value::as_str) == Some("RSA")
                && (kid.is_none() || k.get("kid").and_then(Value::as_str) == kid)
        })
        .ok_or("the ID token's signing key is unknown")?;
    let n = b64(key.get("n").and_then(Value::as_str).unwrap_or_default())?;
    let e = b64(key.get("e").and_then(Value::as_str).unwrap_or_default())?;
    let message = format!("{}.{}", parts[0], parts[1]);
    ring::signature::RsaPublicKeyComponents { n: &n, e: &e }
        .verify(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            message.as_bytes(),
            &b64(signature)?,
        )
        .map_err(|_| "the ID token's signature is not valid".to_string())?;

    let claims = json(payload)?;
    let text = |name: &str| claims.get(name).and_then(Value::as_str);
    if text("iss") != Some(issuer) {
        return Err("the ID token is from an unexpected issuer".into());
    }
    let audience_ok = match claims.get("aud") {
        Some(Value::String(a)) => a == audience,
        Some(Value::Array(list)) => list.iter().any(|a| a.as_str() == Some(audience)),
        _ => false,
    };
    if !audience_ok {
        return Err("the ID token is for another app".into());
    }
    let exp = claims
        .get("exp")
        .and_then(Value::as_i64)
        .ok_or("the ID token has no expiry")?;
    if exp + SKEW < now {
        return Err("the ID token has expired".into());
    }
    if claims.get("iat").and_then(Value::as_i64).is_none() {
        return Err("the ID token is malformed".into());
    }
    if text("nonce") != Some(nonce) {
        return Err("the ID token does not match this sign-in".into());
    }
    let sub = text("sub")
        .filter(|s| !s.is_empty())
        .ok_or("the ID token has no subject")?;
    Ok(Identity {
        sub: sub.to_owned(),
        email: text("email").map(str::to_owned),
    })
}

#[cfg(test)]
pub(crate) mod test_key {
    //! A throwaway RSA key used only by tests to sign ID tokens.
    use base64::Engine;
    use serde_json::{json, Value};

    pub fn jwks() -> Value {
        let jwk: Value =
            serde_json::from_str(include_str!("testdata/rsa-test-key.jwk.json")).unwrap();
        json!({ "keys": [jwk] })
    }

    pub fn sign(claims: &Value) -> String {
        let der = base64::engine::general_purpose::STANDARD
            .decode(include_str!("testdata/rsa-test-key.pkcs8.b64").trim())
            .unwrap();
        let pair = ring::signature::RsaKeyPair::from_pkcs8(&der).unwrap();
        let enc = |v: &Value| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).unwrap())
        };
        let message = format!(
            "{}.{}",
            enc(&json!({"alg": "RS256", "kid": "test-1", "typ": "JWT"})),
            enc(claims)
        );
        let mut sig = vec![0u8; pair.public().modulus_len()];
        pair.sign(
            &ring::signature::RSA_PKCS1_SHA256,
            &ring::rand::SystemRandom::new(),
            message.as_bytes(),
            &mut sig,
        )
        .unwrap();
        format!(
            "{message}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sig)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_key::{jwks, sign};
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_800_000_000;

    fn claims() -> Value {
        json!({"iss": "https://auth.openai.com", "aud": "oaiapp_1", "exp": NOW + 600,
               "iat": NOW, "nonce": "n1", "sub": "user-1", "email": "a@example.com"})
    }

    #[test]
    fn a_valid_token_yields_its_identity() {
        let id = verify(
            &sign(&claims()),
            &jwks(),
            "https://auth.openai.com",
            "oaiapp_1",
            "n1",
            NOW,
        )
        .unwrap();
        assert_eq!(id.sub, "user-1");
        assert_eq!(id.email.as_deref(), Some("a@example.com"));
        let mut list = claims();
        list["aud"] = json!(["other", "oaiapp_1"]);
        assert!(verify(
            &sign(&list),
            &jwks(),
            "https://auth.openai.com",
            "oaiapp_1",
            "n1",
            NOW
        )
        .is_ok());
    }

    #[test]
    fn every_claim_and_the_signature_are_checked() {
        let check = |claims: Value| {
            verify(
                &sign(&claims),
                &jwks(),
                "https://auth.openai.com",
                "oaiapp_1",
                "n1",
                NOW,
            )
        };
        for (field, value) in [
            ("iss", json!("https://evil.example")),
            ("aud", json!("oaiapp_2")),
            ("exp", json!(NOW - 3600)),
            ("nonce", json!("other")),
            ("sub", json!("")),
        ] {
            let mut c = claims();
            c[field] = value;
            assert!(check(c).is_err(), "{field}");
        }
        let token = sign(&claims());
        let mut forged = token.rsplit_once('.').unwrap().0.to_owned();
        forged.push_str(".AAAA");
        assert!(verify(
            &forged,
            &jwks(),
            "https://auth.openai.com",
            "oaiapp_1",
            "n1",
            NOW
        )
        .is_err());
        let none_alg = format!(
            "{}.{}.",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#),
            token.split('.').nth(1).unwrap()
        );
        assert!(verify(
            &none_alg,
            &jwks(),
            "https://auth.openai.com",
            "oaiapp_1",
            "n1",
            NOW
        )
        .is_err());
    }
}
