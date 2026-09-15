use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::LazyLock;

use alexandria_verify::qualification::QualificationPolicySet;
use ed25519_dalek::VerifyingKey;
use libp2p::PeerId;
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

pub const NETWORK_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const MAX_NETWORK_PROFILE_BYTES: usize = 64 * 1024;

const EMBEDDED_PREPROD_JSON: &[u8] = include_bytes!("../resources/networks/preprod.json");
const EMBEDDED_BOOTSTRAP_REGISTRY_JSON: &[u8] =
    include_bytes!("../resources/bootstrap_registry.json");
/// Exact canonical bytes of every reviewed subject qualification policy this
/// build ships. Each must match a digest in
/// `subject_qualification_policy_digests`, and every pinned digest must have
/// its document here. None are pinned yet, so no opinion privilege applies.
const EMBEDDED_QUALIFICATION_POLICY_DOCUMENTS: &[&[u8]] = &[];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NetworkProfile {
    pub schema_version: u32,
    pub network_id: String,
    pub profile_revision: u32,
    pub cardano_network: String,
    pub cardano_network_magic: u64,
    pub relays: Vec<RelayProfile>,
    pub receipt_issuer_keys: Vec<String>,
    pub stake_registry_founder_keys: Vec<NamedVerifyingKey>,
    pub signed_bootstrap_registry_identity: ResourceIdentity,
    pub subject_qualification_policy_digests: Vec<String>,
    pub cloud_https_origin: Option<String>,
    pub cloud_service_id: Option<String>,
    pub governance_locator: Option<String>,
    pub committee_instance_id: Option<String>,
    pub committee_https_endpoints: Vec<String>,
    pub protocol_namespace: String,
    pub optional_governance_anchor_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RelayProfile {
    pub peer_id: String,
    pub dns_name: String,
    pub port: u16,
    pub fallback_ips: Vec<IpAddr>,
    pub registry_https_origin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NamedVerifyingKey {
    pub id: String,
    pub public_key_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResourceIdentity {
    pub schema_version: u32,
    pub sha256: String,
}

#[derive(Debug, Error)]
pub enum NetworkProfileError {
    #[error("network profile exceeds {MAX_NETWORK_PROFILE_BYTES} bytes")]
    TooLarge,
    #[error("invalid network profile JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("unsupported network profile schema version: {0}")]
    UnsupportedSchema(u32),
    #[error("invalid network profile: {0}")]
    Invalid(String),
    #[error("embedded bootstrap registry digest does not match the network profile")]
    BootstrapRegistryMismatch,
    #[error("subject qualification policies are invalid: {0}")]
    QualificationPolicy(String),
}

static EMBEDDED_PREPROD: LazyLock<Result<NetworkProfile, NetworkProfileError>> =
    LazyLock::new(|| NetworkProfile::parse(EMBEDDED_PREPROD_JSON));

static EMBEDDED_QUALIFICATION_POLICIES: LazyLock<
    Result<QualificationPolicySet, NetworkProfileError>,
> = LazyLock::new(|| {
    embedded_preprod()
        .map_err(|error| NetworkProfileError::Invalid(error.to_string()))?
        .qualification_policies(EMBEDDED_QUALIFICATION_POLICY_DOCUMENTS)
});

/// The pinned subject qualification policies for the embedded network. App
/// setup validates this set before any profile opens, so privilege checks
/// never fall back to an empty or partial set after an error.
pub fn embedded_qualification_policies(
) -> Result<&'static QualificationPolicySet, &'static NetworkProfileError> {
    EMBEDDED_QUALIFICATION_POLICIES.as_ref()
}

pub fn embedded_preprod() -> Result<&'static NetworkProfile, &'static NetworkProfileError> {
    EMBEDDED_PREPROD.as_ref()
}

impl NetworkProfile {
    pub fn parse(bytes: &[u8]) -> Result<Self, NetworkProfileError> {
        if bytes.len() > MAX_NETWORK_PROFILE_BYTES {
            return Err(NetworkProfileError::TooLarge);
        }
        reject_duplicate_json_keys(bytes)?;
        let profile: Self = serde_json::from_slice(bytes)?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), NetworkProfileError> {
        if self.schema_version != NETWORK_PROFILE_SCHEMA_VERSION {
            return Err(NetworkProfileError::UnsupportedSchema(self.schema_version));
        }
        validate_identifier("network_id", &self.network_id)?;
        if self.profile_revision == 0 {
            return invalid("profile_revision must be greater than zero");
        }
        validate_identifier("cardano_network", &self.cardano_network)?;
        if self.relays.is_empty() {
            return invalid("at least one relay is required");
        }
        let expected_namespace = format!("/alexandria/{}", self.network_id);
        if self.protocol_namespace != expected_namespace {
            return invalid(format!("protocol_namespace must be {expected_namespace:?}"));
        }

        let mut relay_ids = HashSet::new();
        let mut relay_hosts = HashSet::new();
        for relay in &self.relays {
            relay
                .peer_id
                .parse::<PeerId>()
                .map_err(|e| NetworkProfileError::Invalid(format!("invalid relay peer_id: {e}")))?;
            if !relay_ids.insert(relay.peer_id.as_str()) {
                return invalid(format!("duplicate relay peer_id: {}", relay.peer_id));
            }
            validate_dns_name(&relay.dns_name)?;
            if !relay_hosts.insert(relay.dns_name.as_str()) {
                return invalid(format!("duplicate relay dns_name: {}", relay.dns_name));
            }
            if relay.port == 0 {
                return invalid(format!("relay {} has port zero", relay.dns_name));
            }
            if relay.fallback_ips.is_empty() {
                return invalid(format!(
                    "relay {} requires at least one public fallback IP",
                    relay.dns_name
                ));
            }
            for ip in &relay.fallback_ips {
                if !is_public_ip(ip) {
                    return invalid(format!(
                        "relay {} has non-public fallback IP {ip}",
                        relay.dns_name
                    ));
                }
            }
            validate_https_origin(&relay.registry_https_origin)?;
        }

        if self.receipt_issuer_keys.is_empty() {
            return invalid("at least one receipt issuer key is required");
        }
        let mut receipt_issuers = HashSet::new();
        for issuer in &self.receipt_issuer_keys {
            issuer.parse::<PeerId>().map_err(|e| {
                NetworkProfileError::Invalid(format!("invalid receipt issuer key: {e}"))
            })?;
            if !receipt_issuers.insert(issuer.as_str()) {
                return invalid(format!("duplicate receipt issuer key: {issuer}"));
            }
            if !relay_ids.contains(issuer.as_str()) {
                return invalid(format!(
                    "receipt issuer key {issuer} is not a configured relay"
                ));
            }
        }

        if self.stake_registry_founder_keys.is_empty() {
            return invalid("at least one stake-registry founder key is required");
        }
        let mut founder_ids = HashSet::new();
        let mut founder_keys = HashSet::new();
        for key in &self.stake_registry_founder_keys {
            validate_identifier("stake registry founder id", &key.id)?;
            if !founder_ids.insert(key.id.as_str()) {
                return invalid(format!("duplicate stake-registry founder id: {}", key.id));
            }
            let bytes = decode_32_byte_hex("stake-registry founder key", &key.public_key_hex)?;
            VerifyingKey::from_bytes(&bytes).map_err(|e| {
                NetworkProfileError::Invalid(format!(
                    "invalid stake-registry founder key {}: {e}",
                    key.id
                ))
            })?;
            if !founder_keys.insert(key.public_key_hex.as_str()) {
                return invalid("duplicate stake-registry founder public key");
            }
        }

        if self.signed_bootstrap_registry_identity.schema_version == 0 {
            return invalid("bootstrap registry schema_version must be greater than zero");
        }
        decode_32_byte_hex(
            "bootstrap registry SHA-256",
            &self.signed_bootstrap_registry_identity.sha256,
        )?;
        validate_unique_digests(
            "subject qualification policy digest",
            &self.subject_qualification_policy_digests,
        )?;

        match (&self.cloud_https_origin, &self.cloud_service_id) {
            (None, None) => {}
            (Some(origin), Some(service_id)) => {
                validate_https_origin(origin)?;
                validate_identifier("cloud_service_id", service_id)?;
            }
            _ => return invalid("cloud_https_origin and cloud_service_id must be set together"),
        }

        match (
            &self.governance_locator,
            &self.committee_instance_id,
            self.committee_https_endpoints.is_empty(),
        ) {
            (None, None, true) => {
                if self.optional_governance_anchor_address.is_some() {
                    return invalid(
                        "optional_governance_anchor_address requires governance to be enabled",
                    );
                }
            }
            (Some(locator), Some(instance_id), false) => {
                reject_blank("governance_locator", locator)?;
                validate_identifier("committee_instance_id", instance_id)?;
                for endpoint in &self.committee_https_endpoints {
                    validate_https_origin(endpoint)?;
                }
                if let Some(address) = &self.optional_governance_anchor_address {
                    reject_blank("optional_governance_anchor_address", address)?;
                }
            }
            _ => {
                return invalid(
                    "governance_locator, committee_instance_id and committee_https_endpoints must be enabled together",
                )
            }
        }

        reject_placeholders(self)?;
        Ok(())
    }

    pub fn verify_embedded_resources(&self) -> Result<(), NetworkProfileError> {
        let actual = hex::encode(Sha256::digest(EMBEDDED_BOOTSTRAP_REGISTRY_JSON));
        if actual != self.signed_bootstrap_registry_identity.sha256 {
            return Err(NetworkProfileError::BootstrapRegistryMismatch);
        }
        self.qualification_policies(EMBEDDED_QUALIFICATION_POLICY_DOCUMENTS)?;
        Ok(())
    }

    /// Build the policy set from exact policy documents. Every pinned digest
    /// must be supplied exactly once and nothing unpinned is accepted.
    pub fn qualification_policies(
        &self,
        documents: &[&[u8]],
    ) -> Result<QualificationPolicySet, NetworkProfileError> {
        QualificationPolicySet::from_pinned(
            documents,
            &self.subject_qualification_policy_digests,
            &self.network_id,
        )
        .map_err(|error| NetworkProfileError::QualificationPolicy(error.to_string()))
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T, NetworkProfileError> {
    Err(NetworkProfileError::Invalid(message.into()))
}

fn reject_blank(field: &str, value: &str) -> Result<(), NetworkProfileError> {
    if value.trim().is_empty() {
        return invalid(format!("{field} must not be blank"));
    }
    Ok(())
}

fn validate_identifier(field: &str, value: &str) -> Result<(), NetworkProfileError> {
    reject_blank(field, value)?;
    if value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return invalid(format!(
            "{field} must contain only lowercase ASCII letters, digits, '-' or '_'"
        ));
    }
    Ok(())
}

fn validate_dns_name(value: &str) -> Result<(), NetworkProfileError> {
    reject_blank("relay dns_name", value)?;
    if value.len() > 253
        || value.starts_with('.')
        || value.ends_with('.')
        || value.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    {
        return invalid(format!("invalid relay dns_name: {value}"));
    }
    Ok(())
}

fn validate_https_origin(value: &str) -> Result<(), NetworkProfileError> {
    let url = Url::parse(value)
        .map_err(|e| NetworkProfileError::Invalid(format!("invalid HTTPS origin: {e}")))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return invalid(format!("invalid public HTTPS origin: {value}"));
    }
    Ok(())
}

fn decode_32_byte_hex(field: &str, value: &str) -> Result<[u8; 32], NetworkProfileError> {
    let decoded = hex::decode(value)
        .map_err(|e| NetworkProfileError::Invalid(format!("invalid {field}: {e}")))?;
    decoded
        .try_into()
        .map_err(|_| NetworkProfileError::Invalid(format!("{field} must be 32 bytes")))
}

fn validate_unique_digests(field: &str, values: &[String]) -> Result<(), NetworkProfileError> {
    let mut unique = HashSet::new();
    for value in values {
        decode_32_byte_hex(field, value)?;
        if !unique.insert(value.as_str()) {
            return invalid(format!("duplicate {field}: {value}"));
        }
    }
    Ok(())
}

fn is_public_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(*ip),
        IpAddr::V6(ip) => is_public_ipv6(*ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || a == 0
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 240)
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return is_public_ipv4(mapped);
    }
    !(ip.is_loopback()
        || ip.is_multicast()
        || ip.is_unspecified()
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || (segments[0] == 0x0100 && segments[1..].iter().all(|segment| *segment == 0)))
}

struct DuplicateKeyGuard;

impl<'de> Deserialize<'de> for DuplicateKeyGuard {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(DuplicateKeyVisitor)
    }
}

struct DuplicateKeyVisitor;

impl<'de> Visitor<'de> for DuplicateKeyVisitor {
    type Value = DuplicateKeyGuard;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(DuplicateKeyGuard)
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(DuplicateKeyGuard)
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(DuplicateKeyGuard)
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(DuplicateKeyGuard)
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(DuplicateKeyGuard)
    }

    fn visit_string<E>(self, _value: String) -> Result<Self::Value, E> {
        Ok(DuplicateKeyGuard)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(DuplicateKeyGuard)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(DuplicateKeyGuard)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence.next_element::<DuplicateKeyGuard>()?.is_some() {}
        Ok(DuplicateKeyGuard)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate JSON object key: {key}"
                )));
            }
            map.next_value::<DuplicateKeyGuard>()?;
        }
        Ok(DuplicateKeyGuard)
    }
}

fn reject_duplicate_json_keys(bytes: &[u8]) -> Result<(), serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    DuplicateKeyGuard::deserialize(&mut deserializer)?;
    deserializer.end()
}

fn reject_placeholders(profile: &NetworkProfile) -> Result<(), NetworkProfileError> {
    let value = serde_json::to_value(profile)?;
    fn visit(value: &serde_json::Value) -> Option<&str> {
        match value {
            serde_json::Value::String(text) => {
                let upper = text.to_ascii_uppercase();
                ["PLACEHOLDER", "CHANGEME", "REPLACE_ME", "TODO"]
                    .iter()
                    .any(|marker| upper.contains(marker))
                    .then_some(text.as_str())
            }
            serde_json::Value::Array(values) => values.iter().find_map(visit),
            serde_json::Value::Object(values) => values.values().find_map(visit),
            _ => None,
        }
    }
    if let Some(value) = visit(&value) {
        return invalid(format!("unresolved placeholder value: {value}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_profile() -> NetworkProfile {
        NetworkProfile::parse(EMBEDDED_PREPROD_JSON).unwrap()
    }

    #[test]
    fn embedded_preprod_profile_and_resource_identity_are_valid() {
        let profile = embedded_preprod().unwrap();
        profile.verify_embedded_resources().unwrap();
        assert_eq!(profile.network_id, "preprod");
        assert_eq!(profile.protocol_namespace, "/alexandria/preprod");
        assert!(profile.cloud_https_origin.is_none());
        assert!(profile.committee_instance_id.is_none());
    }

    #[test]
    fn embedded_qualification_policies_are_complete_and_pinned_digests_need_documents() {
        let set = embedded_qualification_policies().unwrap();
        assert_eq!(set.network_id(), "preprod");
        assert!(set.policies().is_empty());

        let mut profile = valid_profile();
        profile.subject_qualification_policy_digests = vec!["ab".repeat(32)];
        assert!(matches!(
            profile.qualification_policies(&[]),
            Err(NetworkProfileError::QualificationPolicy(_))
        ));
        assert!(matches!(
            profile.verify_embedded_resources(),
            Err(NetworkProfileError::QualificationPolicy(_))
        ));
    }

    #[test]
    fn malformed_and_oversized_profiles_fail_closed() {
        assert!(matches!(
            NetworkProfile::parse(b"{"),
            Err(NetworkProfileError::InvalidJson(_))
        ));
        assert!(matches!(
            NetworkProfile::parse(&vec![b' '; MAX_NETWORK_PROFILE_BYTES + 1]),
            Err(NetworkProfileError::TooLarge)
        ));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let mut value: serde_json::Value = serde_json::from_slice(EMBEDDED_PREPROD_JSON).unwrap();
        value["api_secret"] = serde_json::json!("must never be accepted");
        assert!(matches!(
            NetworkProfile::parse(&serde_json::to_vec(&value).unwrap()),
            Err(NetworkProfileError::InvalidJson(_))
        ));
    }

    #[test]
    fn duplicate_fields_are_rejected() {
        let bytes = EMBEDDED_PREPROD_JSON;
        let end = bytes.iter().rposition(|byte| *byte == b'}').unwrap();
        let mut duplicated = bytes[..end].to_vec();
        duplicated.extend_from_slice(b",\n  \"network_id\": \"other\"\n}");
        assert!(matches!(
            NetworkProfile::parse(&duplicated),
            Err(NetworkProfileError::InvalidJson(_))
        ));
    }

    #[test]
    fn plaintext_registry_origin_is_rejected() {
        let mut profile = valid_profile();
        profile.relays[0].registry_https_origin = "http://relay.example".into();
        assert!(profile.validate().is_err());
    }

    #[test]
    fn private_fallback_ip_is_rejected() {
        let mut profile = valid_profile();
        profile.relays[0].fallback_ips = vec!["127.0.0.1".parse().unwrap()];
        assert!(profile.validate().is_err());
    }

    #[test]
    fn receipt_issuer_must_be_a_configured_relay() {
        let mut profile = valid_profile();
        profile.receipt_issuer_keys[0] =
            "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN".into();
        assert!(profile.validate().is_err());
    }

    #[test]
    fn optional_service_fields_must_be_enabled_together() {
        let mut profile = valid_profile();
        profile.cloud_https_origin = Some("https://cloud.example".into());
        assert!(profile.validate().is_err());

        let mut profile = valid_profile();
        profile.committee_instance_id = Some("committee-a".into());
        assert!(profile.validate().is_err());
    }

    #[test]
    fn protocol_namespace_is_bound_to_network_identity() {
        let mut profile = valid_profile();
        profile.network_id = "other-network".into();
        assert!(profile.validate().is_err());
        profile.protocol_namespace = "/alexandria/other-network".into();
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn unresolved_placeholder_is_rejected() {
        let mut profile = valid_profile();
        profile.cloud_https_origin = Some("https://placeholder.example".into());
        profile.cloud_service_id = Some("cloud-a".into());
        assert!(profile.validate().is_err());
    }

    #[test]
    fn changed_bootstrap_bytes_fail_identity_check() {
        let mut profile = valid_profile();
        profile.signed_bootstrap_registry_identity.sha256 = "00".repeat(32);
        assert!(matches!(
            profile.verify_embedded_resources(),
            Err(NetworkProfileError::BootstrapRegistryMismatch)
        ));
    }
}
