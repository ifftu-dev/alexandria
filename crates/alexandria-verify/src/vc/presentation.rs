//! W3C Verifiable Presentations (VC Data Model 2.0 §4.13) secured with a
//! holder `DataIntegrityProof`.
//!
//! A presentation is how a holder hands credentials to one verifier, once:
//! the proof's purpose is `authentication`, its `challenge` is the nonce the
//! verifier issued and its `domain` is the verifier's audience, so the same
//! document cannot be replayed to anyone else or later. The proof carries
//! `created` and `expires` so a presentation is short-lived by construction.
//!
//! The same `eddsa-jcs-2022` cryptosuite secures credentials and
//! presentations, so a verifier that checks one checks the other with the
//! same three primitives.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::sign::{did_key_verification_method, sign_document, verify_document};
use super::{context::W3C_VC_V2, Proof, VcError, DATA_INTEGRITY_PROOF, EDDSA_JCS_2022};
use crate::did::{derive_did_key, resolve_did_key, Did};
use ed25519_dalek::SigningKey;

pub const VERIFIABLE_PRESENTATION: &str = "VerifiablePresentation";
/// How long a presentation proof may remain valid after `created`.
pub const MAX_LIFETIME_SECS: i64 = 300;

/// A presentation. `verifiable_credential` holds credentials as JSON so a
/// selectively disclosed (redacted) credential can travel the same way as a
/// complete one; `properties` carries any extra terms a protocol binds
/// (resolved through the v2 context's issuer-dependent vocabulary).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiablePresentation {
    #[serde(rename = "@context")]
    pub context: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub type_: Vec<String>,
    pub holder: Did,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verifiable_credential: Vec<Value>,
    #[serde(flatten)]
    pub properties: serde_json::Map<String, Value>,
    pub proof: Proof,
}

impl VerifiablePresentation {
    /// An unsigned presentation by `holder` for the verifier identified by
    /// `domain`, answering `challenge`.
    pub fn new(
        id: Option<String>,
        holder: Did,
        verifiable_credential: Vec<Value>,
        created: &str,
        expires: &str,
        challenge: &str,
        domain: &str,
    ) -> Self {
        VerifiablePresentation {
            context: vec![W3C_VC_V2.into()],
            id,
            type_: vec![VERIFIABLE_PRESENTATION.into()],
            holder,
            verifiable_credential,
            properties: serde_json::Map::new(),
            proof: Proof::for_presentation(created, expires, challenge, domain),
        }
    }
}

/// Sign `presentation` as its holder. The key must control the holder DID.
pub fn sign_presentation(
    mut presentation: VerifiablePresentation,
    key: &SigningKey,
) -> Result<VerifiablePresentation, VcError> {
    if derive_did_key(key) != presentation.holder {
        return Err(VcError::InvalidCredential(
            "the signing key does not control the presentation holder".into(),
        ));
    }
    if !presentation
        .type_
        .iter()
        .any(|t| t == VERIFIABLE_PRESENTATION)
    {
        presentation.type_.insert(0, VERIFIABLE_PRESENTATION.into());
    }
    presentation.proof.type_ = DATA_INTEGRITY_PROOF.into();
    presentation.proof.cryptosuite = EDDSA_JCS_2022.into();
    presentation.proof.proof_purpose = "authentication".into();
    presentation.proof.verification_method = did_key_verification_method(&presentation.holder, key);
    let mut document = serde_json::to_value(&presentation)?;
    sign_document(&mut document, key)?;
    Ok(serde_json::from_value(document)?)
}

/// What a verified presentation proof established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationProof {
    pub holder: Did,
    pub challenge: String,
    pub domain: String,
    pub created: i64,
    pub expires: i64,
}

/// Check a presentation's shape, binding, timing and holder signature.
///
/// `now` is unix seconds. The holder's key is `did:key` self-resolved; a
/// presentation by a non-self-resolving holder cannot be checked here.
pub fn verify_presentation_proof(
    presentation: &VerifiablePresentation,
    now: i64,
) -> Result<PresentationProof, String> {
    if !presentation
        .type_
        .iter()
        .any(|t| t == VERIFIABLE_PRESENTATION)
    {
        return Err("not a VerifiablePresentation".into());
    }
    let proof = &presentation.proof;
    if proof.type_ != DATA_INTEGRITY_PROOF
        || proof.cryptosuite != EDDSA_JCS_2022
        || proof.proof_purpose != "authentication"
    {
        return Err("presentation proof is not an eddsa-jcs-2022 authentication proof".into());
    }
    let (challenge, domain) = match (&proof.challenge, &proof.domain) {
        (Some(c), Some(d)) if !c.is_empty() && !d.is_empty() => (c.clone(), d.clone()),
        _ => return Err("presentation proof is not bound to a challenge and domain".into()),
    };
    let created = rfc3339_to_unix(&proof.created).ok_or("proof.created is not a timestamp")?;
    let expires = proof
        .expires
        .as_deref()
        .and_then(rfc3339_to_unix)
        .ok_or("proof.expires is not a timestamp")?;
    if expires <= now {
        return Err("presentation proof has expired".into());
    }
    if created > now.saturating_add(30)
        || expires <= created
        || expires > created.saturating_add(MAX_LIFETIME_SECS)
    {
        return Err("presentation proof has invalid timing".into());
    }
    let holder = presentation.holder.as_str();
    let Some((controller, fragment)) = proof.verification_method.0.split_once('#') else {
        return Err("verificationMethod is not a fragment of the holder".into());
    };
    if controller != holder || fragment.is_empty() {
        return Err("verificationMethod is not controlled by the holder".into());
    }
    let key = resolve_did_key(&presentation.holder).map_err(|e| e.to_string())?;
    let document = serde_json::to_value(presentation).map_err(|e| e.to_string())?;
    if !verify_document(&document, &key).map_err(|e| e.to_string())? {
        return Err("invalid holder signature".into());
    }
    Ok(PresentationProof {
        holder: presentation.holder.clone(),
        challenge,
        domain,
        created,
        expires,
    })
}

/// `YYYY-MM-DDTHH:MM:SSZ` (optionally with fractional seconds) → unix seconds.
pub fn rfc3339_to_unix(s: &str) -> Option<i64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day) = (
        d.next()?.parse::<i64>().ok()?,
        d.next()?.parse::<u32>().ok()?,
        d.next()?.parse::<u32>().ok()?,
    );
    if d.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    let time = time.split('.').next()?;
    let mut t = time.split(':');
    let (h, min, sec) = (
        t.next()?.parse::<i64>().ok()?,
        t.next()?.parse::<i64>().ok()?,
        t.next()?.parse::<i64>().ok()?,
    );
    if t.next().is_some() || h > 23 || min > 59 || sec > 60 {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * i64::from(m) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3_600 + min * 60 + sec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn holder() -> (SigningKey, Did) {
        let key = SigningKey::from_bytes(&[42; 32]);
        let did = derive_did_key(&key);
        (key, did)
    }

    #[test]
    fn rfc3339_parses_like_the_clock() {
        assert_eq!(rfc3339_to_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_to_unix("2026-06-01T00:00:00Z"), Some(1_780_272_000));
        assert_eq!(
            rfc3339_to_unix("2026-06-01T00:00:00.123Z"),
            Some(1_780_272_000)
        );
        assert_eq!(rfc3339_to_unix("2026-06-01T00:00:00+01:00"), None);
        assert_eq!(rfc3339_to_unix("not a date"), None);
    }

    #[test]
    fn a_presentation_is_bound_to_its_challenge_domain_holder_and_window() {
        let (key, did) = holder();
        let now = rfc3339_to_unix("2026-06-01T00:00:00Z").unwrap();
        let vp = VerifiablePresentation::new(
            Some("urn:uuid:vp-1".into()),
            did.clone(),
            vec![json!({"id": "urn:uuid:c1", "type": ["VerifiableCredential"]})],
            "2026-06-01T00:00:00Z",
            "2026-06-01T00:05:00Z",
            "nonce-1",
            "urn:alexandria:organization:org",
        );
        let signed = sign_presentation(vp.clone(), &key).unwrap();
        let json = serde_json::to_value(&signed).unwrap();
        assert_eq!(json["type"], json!(["VerifiablePresentation"]));
        assert_eq!(json["proof"]["proofPurpose"], "authentication");
        assert_eq!(json["proof"]["challenge"], "nonce-1");
        assert_eq!(json["proof"]["domain"], "urn:alexandria:organization:org");
        assert!(json["proof"]["proofValue"]
            .as_str()
            .unwrap()
            .starts_with('z'));
        let checked = verify_presentation_proof(&signed, now + 10).unwrap();
        assert_eq!(checked.challenge, "nonce-1");
        assert_eq!(checked.holder, did);

        let mut moved = signed.clone();
        moved.proof.domain = Some("urn:alexandria:organization:other".into());
        assert_eq!(
            verify_presentation_proof(&moved, now).unwrap_err(),
            "invalid holder signature"
        );
        let mut tampered = signed.clone();
        tampered.verifiable_credential[0]["id"] = json!("urn:uuid:c2");
        assert_eq!(
            verify_presentation_proof(&tampered, now).unwrap_err(),
            "invalid holder signature"
        );
        assert!(
            verify_presentation_proof(&signed, now + 301).is_err(),
            "expired"
        );
        assert!(
            verify_presentation_proof(&signed, now - 60).is_err(),
            "not yet created"
        );
        let other = SigningKey::from_bytes(&[7; 32]);
        assert!(
            sign_presentation(vp.clone(), &other).is_err(),
            "key must control the holder"
        );
        let mut long = vp.clone();
        long.proof.expires = Some("2026-06-01T00:06:00Z".into());
        let long = sign_presentation(long, &key).unwrap();
        assert!(
            verify_presentation_proof(&long, now).is_err(),
            "lifetime capped"
        );
        let mut unbound = vp;
        unbound.proof.challenge = None;
        let unbound = sign_presentation(unbound, &key).unwrap();
        assert!(verify_presentation_proof(&unbound, now).is_err());
    }
}
