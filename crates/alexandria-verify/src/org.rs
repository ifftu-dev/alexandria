//! Organisation identity: a name an organisation has proven by its domain.
//!
//! An organisation that asks a learner for anything — access to their skills,
//! an assessment, an interview, an offer — names itself. A name on its own is
//! a claim anybody can make, so the claim is anchored where only the real
//! organisation can write: a DNS TXT record on its own domain,
//!
//! ```text
//! _alexandria.<domain>  TXT  "v=alexandria-org1; name=<name>; key=<did:key>"
//! ```
//!
//! naming the organisation and listing the keys allowed to sign for it
//! (`key=` may repeat).
//!
//! Every request the organisation sends carries an [`OrgProof`]. A learner's
//! app looks the record up itself and checks the proof against it, so the
//! name it shows is the one the domain's owner published, not one a service
//! relayed. Nobody between the two has to be trusted, including the service
//! that carried the request.
//!
//! The crate performs no I/O: callers resolve the TXT records and hand them
//! to [`descriptor_from_txt`].

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The descriptor's `format` value.
pub const DESCRIPTOR_FORMAT: &str = "alexandria-org/1";

/// The label the TXT record sits under, in front of the organisation's domain.
pub const TXT_LABEL: &str = "_alexandria";

/// The record's `v=` value.
pub const TXT_VERSION: &str = "alexandria-org1";

/// The most signing keys a descriptor may list: enough to rotate, few enough
/// that a descriptor is not a key dump.
pub const MAX_KEYS: usize = 8;

/// The longest organisation name accepted.
pub const MAX_NAME_CHARS: usize = 120;

const JWS_HEADER_JSON: &str = r#"{"alg":"EdDSA","b64":false,"crit":["b64"]}"#;

/// What an organisation publishes on its domain, as read from its TXT record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrgDescriptor {
    /// Always [`DESCRIPTOR_FORMAT`].
    pub format: String,
    /// The domain the record was found under.
    pub domain: String,
    /// The organisation's name, as learners will see it.
    pub name: String,
    /// `did:key` of every key allowed to sign for the organisation.
    pub keys: Vec<String>,
}

/// An organisation's signature over one payload.
///
/// The signature covers the payload together with the domain and key named
/// here, so a proof cannot be lifted onto another payload or credited to
/// another domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OrgProof {
    pub domain: String,
    /// The `did:key` that signed. Must be listed in the domain's descriptor.
    pub verification_method: String,
    pub created: String,
    /// Detached compact JWS: `header..signature`.
    pub jws: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrgError {
    /// Not a lowercase DNS name with at least one dot.
    BadDomain(String),
    /// The descriptor is malformed or names a different domain.
    BadDescriptor(String),
    /// The proof names a domain other than the descriptor's.
    WrongDomain,
    /// The signing key is not one the domain lists.
    UnlistedKey,
    /// The payload names an organisation other than the domain's.
    WrongName {
        claimed: String,
        published: String,
    },
    /// The payload's audience is not this organisation's.
    WrongAudience,
    MalformedProof(String),
    BadSignature,
}

impl std::fmt::Display for OrgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadDomain(d) => write!(f, "not a domain name: {d}"),
            Self::BadDescriptor(e) => write!(f, "organisation descriptor rejected: {e}"),
            Self::WrongDomain => write!(f, "signed for a different domain"),
            Self::UnlistedKey => write!(f, "signed with a key the domain does not list"),
            Self::WrongName { claimed, published } => write!(
                f,
                "names itself {claimed:?} but its domain publishes {published:?}"
            ),
            Self::WrongAudience => write!(f, "addressed for a different organisation"),
            Self::MalformedProof(e) => write!(f, "malformed proof: {e}"),
            Self::BadSignature => write!(f, "signature does not verify"),
        }
    }
}

impl std::error::Error for OrgError {}

/// Normalise and check a domain name.
///
/// Lowercase ASCII labels of letters, digits and hyphens, at least two of
/// them, no port, no trailing dot. Internationalised names arrive here
/// already in their `xn--` form or not at all.
pub fn normalise_domain(domain: &str) -> Result<String, OrgError> {
    let d = domain.trim().to_ascii_lowercase();
    let bad = || OrgError::BadDomain(domain.to_string());
    if d.len() > 253 || !d.contains('.') {
        return Err(bad());
    }
    for label in d.split('.') {
        let ok = !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !ok {
            return Err(bad());
        }
    }
    Ok(d)
}

/// The DNS name to look the record up at.
pub fn txt_name(domain: &str) -> Result<String, OrgError> {
    Ok(format!("{TXT_LABEL}.{}", normalise_domain(domain)?))
}

/// The TXT value an organisation publishes for this descriptor.
pub fn txt_value(descriptor: &OrgDescriptor) -> String {
    let mut value = format!("v={TXT_VERSION}; name={}", descriptor.name);
    for key in &descriptor.keys {
        value.push_str("; key=");
        value.push_str(key);
    }
    value
}

/// Read the descriptor out of the TXT records found at [`txt_name`].
///
/// Each record is the concatenation of its character-strings. Records that are
/// not ours (no `v=alexandria-org1`) are ignored, since a name may carry other
/// TXT records; more than one of ours is refused rather than guessed between.
/// The result has passed [`check_descriptor`].
pub fn descriptor_from_txt(domain: &str, records: &[String]) -> Result<OrgDescriptor, OrgError> {
    let domain = normalise_domain(domain)?;
    let bad = |e: &str| OrgError::BadDescriptor(e.to_string());
    let ours: Vec<Vec<(String, String)>> = records
        .iter()
        .map(|record| {
            record
                .split(';')
                .filter_map(|part| {
                    let (k, v) = part.split_once('=')?;
                    Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
                })
                .collect::<Vec<_>>()
        })
        .filter(|fields| fields.iter().any(|(k, v)| k == "v" && v == TXT_VERSION))
        .collect();
    let fields = match ours.as_slice() {
        [] => return Err(bad("no Alexandria record on the domain")),
        [one] => one,
        _ => return Err(bad("more than one Alexandria record on the domain")),
    };
    let names: Vec<&String> = fields
        .iter()
        .filter(|(k, _)| k == "name")
        .map(|(_, v)| v)
        .collect();
    let [name] = names.as_slice() else {
        return Err(bad("the record must name the organisation exactly once"));
    };
    let descriptor = OrgDescriptor {
        format: DESCRIPTOR_FORMAT.to_string(),
        domain: domain.clone(),
        name: (*name).clone(),
        keys: fields
            .iter()
            .filter(|(k, _)| k == "key")
            .map(|(_, v)| v.clone())
            .collect(),
    };
    check_descriptor(&descriptor, &domain)?;
    Ok(descriptor)
}

/// The audience an organisation's requests are addressed with, and that a
/// learner's presentation is bound to.
pub fn audience(domain: &str) -> String {
    format!("urn:alexandria:org:{domain}")
}

/// Check a descriptor read for the domain `fetched_from`.
pub fn check_descriptor(descriptor: &OrgDescriptor, fetched_from: &str) -> Result<(), OrgError> {
    let bad = |e: &str| Err(OrgError::BadDescriptor(e.to_string()));
    if descriptor.format != DESCRIPTOR_FORMAT {
        return bad("unknown format");
    }
    if normalise_domain(&descriptor.domain)? != descriptor.domain {
        return bad("domain is not in normal form");
    }
    if descriptor.domain != normalise_domain(fetched_from)? {
        return bad("names a different domain from the one it was read for");
    }
    let name = descriptor.name.trim();
    if name.is_empty() || name != descriptor.name {
        return bad("name is empty or padded");
    }
    if descriptor.name.chars().count() > MAX_NAME_CHARS {
        return bad("name is too long");
    }
    if descriptor.name.chars().any(char::is_control) {
        return bad("name contains control characters");
    }
    if descriptor.keys.is_empty() || descriptor.keys.len() > MAX_KEYS {
        return bad("lists no keys, or too many");
    }
    for key in &descriptor.keys {
        crate::did::resolve_did_key(&crate::did::Did(key.clone()))
            .map_err(|_| OrgError::BadDescriptor(format!("not a did:key: {key}")))?;
    }
    Ok(())
}

fn signing_input(
    payload: &Value,
    domain: &str,
    method: &str,
    created: &str,
    header_b64: &str,
) -> Result<Vec<u8>, String> {
    let bound = serde_json::json!({
        "created": created,
        "domain": domain,
        "payload": payload,
        "verificationMethod": method,
    });
    let canonical = crate::vc::canonicalize::canonicalize(&bound)
        .map_err(|e| format!("canonicalize payload: {e}"))?;
    let mut out = Vec::with_capacity(header_b64.len() + 1 + canonical.len());
    out.extend_from_slice(header_b64.as_bytes());
    out.push(b'.');
    out.extend_from_slice(&canonical);
    Ok(out)
}

/// Sign a payload as the organisation at `domain`.
pub fn sign(
    payload: &Value,
    domain: &str,
    signing_key: &ed25519_dalek::SigningKey,
    now: &str,
) -> Result<OrgProof, String> {
    use ed25519_dalek::Signer;

    let domain = normalise_domain(domain).map_err(|e| e.to_string())?;
    let method = crate::did::derive_did_key(signing_key).as_str().to_string();
    let header_b64 = crate::vc::sign::b64url(JWS_HEADER_JSON.as_bytes());
    let input = signing_input(payload, &domain, &method, now, &header_b64)?;
    let sig = signing_key.sign(&input);
    Ok(OrgProof {
        domain,
        verification_method: method,
        created: now.to_string(),
        jws: format!("{header_b64}..{}", crate::vc::sign::b64url(&sig.to_bytes())),
    })
}

/// Verify an organisation's proof over a payload against its descriptor.
///
/// The descriptor must already have passed [`check_descriptor`] for the domain
/// it came from. Beyond the signature, two fields of the payload are held to
/// the descriptor when present: `organization` must be the published name, and
/// `audience` must be this organisation's. Those are what a learner reads and
/// what their answer gets bound to, so neither may be the signer's own story.
pub fn verify(
    payload: &Value,
    proof: &OrgProof,
    descriptor: &OrgDescriptor,
) -> Result<(), OrgError> {
    use ed25519_dalek::Verifier;

    if proof.domain != descriptor.domain {
        return Err(OrgError::WrongDomain);
    }
    if !descriptor
        .keys
        .iter()
        .any(|k| k == &proof.verification_method)
    {
        return Err(OrgError::UnlistedKey);
    }
    if let Some(claimed) = payload.get("organization") {
        let claimed = claimed.as_str().unwrap_or_default();
        if claimed != descriptor.name {
            return Err(OrgError::WrongName {
                claimed: claimed.to_string(),
                published: descriptor.name.clone(),
            });
        }
    }
    if let Some(aud) = payload.get("audience") {
        if aud.as_str() != Some(audience(&descriptor.domain).as_str()) {
            return Err(OrgError::WrongAudience);
        }
    }

    let key = crate::did::resolve_did_key(&crate::did::Did(proof.verification_method.clone()))
        .map_err(|_| OrgError::MalformedProof("verification method is not a did:key".into()))?;
    let (header_b64, sig_b64) = proof
        .jws
        .split_once("..")
        .ok_or_else(|| OrgError::MalformedProof("expected detached compact JWS".into()))?;
    if crate::vc::sign::b64url_decode(header_b64).as_deref() != Some(JWS_HEADER_JSON.as_bytes()) {
        return Err(OrgError::MalformedProof("unexpected JWS header".into()));
    }
    let sig: [u8; 64] = crate::vc::sign::b64url_decode(sig_b64)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| OrgError::MalformedProof("signature is not 64 base64url bytes".into()))?;
    let input = signing_input(
        payload,
        &proof.domain,
        &proof.verification_method,
        &proof.created,
        header_b64,
    )
    .map_err(OrgError::MalformedProof)?;
    key.verify(&input, &ed25519_dalek::Signature::from_bytes(&sig))
        .map_err(|_| OrgError::BadSignature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use serde_json::json;

    const NOW: &str = "2026-10-12T00:00:00Z";

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn descriptor(k: &SigningKey) -> OrgDescriptor {
        OrgDescriptor {
            format: DESCRIPTOR_FORMAT.into(),
            domain: "alexandria.ifftu.dev".into(),
            name: "IFFTU".into(),
            keys: vec![crate::did::derive_did_key(k).as_str().to_string()],
        }
    }

    fn payload() -> Value {
        json!({
            "id": "req-1",
            "organization": "IFFTU",
            "audience": audience("alexandria.ifftu.dev"),
            "skills": ["skill_big_o"],
        })
    }

    #[test]
    fn domains_are_normalised_and_checked() {
        assert_eq!(
            normalise_domain(" Alexandria.IFFTU.dev ").unwrap(),
            "alexandria.ifftu.dev"
        );
        for bad in [
            "localhost",
            "a..b",
            "-a.b",
            "a.b:443",
            "a.b/",
            "a b.c",
            "é.fr",
            "",
        ] {
            assert!(normalise_domain(bad).is_err(), "{bad}");
        }
        assert_eq!(
            txt_name("alexandria.ifftu.dev").unwrap(),
            "_alexandria.alexandria.ifftu.dev"
        );
    }

    #[test]
    fn the_txt_record_round_trips_and_ignores_other_records() {
        let d = descriptor(&key(1));
        let value = txt_value(&d);
        assert!(value.starts_with("v=alexandria-org1; name=IFFTU; key=did:key:"));
        let records = vec![
            "v=spf1 include:_spf.google.com ~all".to_string(),
            value.clone(),
        ];
        assert_eq!(
            descriptor_from_txt("alexandria.ifftu.dev", &records).unwrap(),
            d
        );
        // Spacing and case of the field names do not matter.
        let loose = value.replace("; ", ";").replace("name=", "NAME = ");
        assert_eq!(
            descriptor_from_txt("alexandria.ifftu.dev", &[loose]).unwrap(),
            d
        );
    }

    #[test]
    fn a_missing_doubled_or_unkeyed_record_is_refused() {
        let d = descriptor(&key(1));
        assert!(descriptor_from_txt("alexandria.ifftu.dev", &[]).is_err());
        assert!(
            descriptor_from_txt("alexandria.ifftu.dev", &[txt_value(&d), txt_value(&d)]).is_err()
        );
        assert!(descriptor_from_txt(
            "alexandria.ifftu.dev",
            &["v=alexandria-org1; name=IFFTU".to_string()]
        )
        .is_err());
        assert!(descriptor_from_txt(
            "alexandria.ifftu.dev",
            &[format!("{}; name=Other", txt_value(&d))]
        )
        .is_err());
    }

    #[test]
    fn a_descriptor_must_name_the_domain_it_came_from() {
        let d = descriptor(&key(1));
        check_descriptor(&d, "alexandria.ifftu.dev").unwrap();
        assert!(check_descriptor(&d, "example.com").is_err());
        let mut unnamed = d.clone();
        unnamed.name = "  ".into();
        assert!(check_descriptor(&unnamed, "alexandria.ifftu.dev").is_err());
        let mut keyless = d.clone();
        keyless.keys.clear();
        assert!(check_descriptor(&keyless, "alexandria.ifftu.dev").is_err());
        let mut junk = d;
        junk.keys = vec!["did:web:example.com".into()];
        assert!(check_descriptor(&junk, "alexandria.ifftu.dev").is_err());
    }

    #[test]
    fn a_signed_payload_verifies_against_the_published_descriptor() {
        let k = key(1);
        let proof = sign(&payload(), "alexandria.ifftu.dev", &k, NOW).unwrap();
        verify(&payload(), &proof, &descriptor(&k)).unwrap();
    }

    #[test]
    fn a_key_the_domain_does_not_list_is_refused() {
        let proof = sign(&payload(), "alexandria.ifftu.dev", &key(2), NOW).unwrap();
        assert_eq!(
            verify(&payload(), &proof, &descriptor(&key(1))),
            Err(OrgError::UnlistedKey)
        );
    }

    #[test]
    fn the_name_and_audience_are_the_domains_not_the_signers() {
        let k = key(1);
        let mut renamed = payload();
        renamed["organization"] = json!("Some Bank");
        let proof = sign(&renamed, "alexandria.ifftu.dev", &k, NOW).unwrap();
        assert!(matches!(
            verify(&renamed, &proof, &descriptor(&k)),
            Err(OrgError::WrongName { .. })
        ));

        let mut readdressed = payload();
        readdressed["audience"] = json!(audience("example.com"));
        let proof = sign(&readdressed, "alexandria.ifftu.dev", &k, NOW).unwrap();
        assert_eq!(
            verify(&readdressed, &proof, &descriptor(&k)),
            Err(OrgError::WrongAudience)
        );
    }

    #[test]
    fn tampering_with_the_payload_or_the_proof_breaks_the_signature() {
        let k = key(1);
        let proof = sign(&payload(), "alexandria.ifftu.dev", &k, NOW).unwrap();

        let mut altered = payload();
        altered["skills"] = json!(["skill_big_o", "skill_javascript"]);
        assert_eq!(
            verify(&altered, &proof, &descriptor(&k)),
            Err(OrgError::BadSignature)
        );

        let mut backdated = proof.clone();
        backdated.created = "2020-01-01T00:00:00Z".into();
        assert_eq!(
            verify(&payload(), &backdated, &descriptor(&k)),
            Err(OrgError::BadSignature)
        );

        let mut elsewhere = proof;
        elsewhere.domain = "example.com".into();
        assert_eq!(
            verify(&payload(), &elsewhere, &descriptor(&k)),
            Err(OrgError::WrongDomain)
        );
    }
}
