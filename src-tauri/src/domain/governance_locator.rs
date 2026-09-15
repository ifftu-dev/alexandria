//! Bounded, content-addressed transport locators for founding genesis files.
//!
//! A locator is discovery metadata, never authority. It contains only the
//! genesis-derived DAO id, the BLAKE3 hash of the canonical JSON artifact,
//! and two or more retrieval locations. Importers must retrieve the bytes,
//! verify the content hash and complete genesis envelope, show the material
//! trust facts, and require a separate explicit pin operation.

use std::collections::BTreeSet;

use serde::Serialize;
use url::Url;

use crate::content_store::resolver::reject_unreviewable_https_source;

pub const GOVERNANCE_GENESIS_LOCATOR_VERSION: u16 = 1;
pub const MAX_GOVERNANCE_GENESIS_LOCATOR_BYTES: usize = 2 * 1024;
pub const MAX_GOVERNANCE_GENESIS_LOCATIONS: usize = 8;
const MIN_GOVERNANCE_GENESIS_LOCATIONS: usize = 2;
const MAX_LOCATION_BYTES: usize = 1024;
const OFFICIAL_APP_LINK_HOST: &str = "alexandria.ifftu.dev";
const IROH_ORIGIN: &str = "iroh://";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GovernanceGenesisLocator {
    pub version: u16,
    pub dao_id: String,
    pub content_hash: String,
    /// Canonically sorted source strings, at most one per origin. Iroh
    /// sources are exact `iroh://<blake3>` identifiers. HTTPS sources carry
    /// the same digest in a `#blake3=<digest>` fragment, which is not sent to
    /// the origin.
    pub locations: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GovernanceLocatorError {
    #[error(
        "governance genesis locator exceeds the {MAX_GOVERNANCE_GENESIS_LOCATOR_BYTES}-byte limit"
    )]
    TooLarge,
    #[error("governance genesis locator URL is malformed or unsupported")]
    InvalidUrl,
    #[error("governance genesis locator contains an unsupported or duplicate field")]
    InvalidQuery,
    #[error("governance genesis locator contains an invalid DAO id or content hash")]
    InvalidDigest,
    #[error("governance genesis locator requires between 2 and {MAX_GOVERNANCE_GENESIS_LOCATIONS} content-addressed locations on distinct public origins")]
    InvalidLocations,
}

impl GovernanceGenesisLocator {
    pub fn parse(encoded: &str) -> Result<Self, GovernanceLocatorError> {
        if encoded.len() > MAX_GOVERNANCE_GENESIS_LOCATOR_BYTES {
            return Err(GovernanceLocatorError::TooLarge);
        }
        if encoded.trim() != encoded {
            return Err(GovernanceLocatorError::InvalidUrl);
        }
        let url = Url::parse(encoded).map_err(|_| GovernanceLocatorError::InvalidUrl)?;
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err(GovernanceLocatorError::InvalidUrl);
        }

        let dao_id = locator_dao_id(&url)?;
        if !is_canonical_digest(&dao_id) {
            return Err(GovernanceLocatorError::InvalidDigest);
        }

        let mut content_hash = None;
        let mut locations = Vec::new();
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "content" if content_hash.is_none() => content_hash = Some(value.into_owned()),
                "source" => locations.push(value.into_owned()),
                _ => return Err(GovernanceLocatorError::InvalidQuery),
            }
        }
        let content_hash = content_hash.ok_or(GovernanceLocatorError::InvalidQuery)?;
        if !is_canonical_digest(&content_hash) {
            return Err(GovernanceLocatorError::InvalidDigest);
        }
        canonicalize_locations(&content_hash, &mut locations)?;

        Ok(Self {
            version: GOVERNANCE_GENESIS_LOCATOR_VERSION,
            dao_id,
            content_hash,
            locations,
        })
    }

    /// Build the compact custom-scheme form used for QR codes and sharing.
    pub fn encode(&self) -> Result<String, GovernanceLocatorError> {
        if self.version != GOVERNANCE_GENESIS_LOCATOR_VERSION
            || !is_canonical_digest(&self.dao_id)
            || !is_canonical_digest(&self.content_hash)
        {
            return Err(GovernanceLocatorError::InvalidDigest);
        }
        let mut locations = self.locations.clone();
        canonicalize_locations(&self.content_hash, &mut locations)?;
        let base = format!("alexandria://governance/genesis/{}", self.dao_id);
        let mut url = Url::parse(&base).map_err(|_| GovernanceLocatorError::InvalidUrl)?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("content", &self.content_hash);
            for location in &locations {
                query.append_pair("source", location);
            }
        }
        let encoded: String = url.into();
        if encoded.len() > MAX_GOVERNANCE_GENESIS_LOCATOR_BYTES {
            return Err(GovernanceLocatorError::TooLarge);
        }
        Ok(encoded)
    }

    /// Return fetchable identifiers without weakening their content binding.
    /// The caller still verifies `content_hash` and the genesis-derived
    /// `dao_id` after retrieval.
    pub fn retrieval_identifiers(&self) -> Result<Vec<String>, GovernanceLocatorError> {
        let mut locations = self.locations.clone();
        canonicalize_locations(&self.content_hash, &mut locations)?;
        locations.sort_by_key(|location| !location.starts_with(IROH_ORIGIN));
        locations
            .into_iter()
            .map(|location| {
                if location.starts_with(IROH_ORIGIN) {
                    return Ok(self.content_hash.clone());
                }
                let mut url =
                    Url::parse(&location).map_err(|_| GovernanceLocatorError::InvalidLocations)?;
                url.set_fragment(None);
                Ok(url.into())
            })
            .collect()
    }
}

fn locator_dao_id(url: &Url) -> Result<String, GovernanceLocatorError> {
    if url.port().is_some() {
        return Err(GovernanceLocatorError::InvalidUrl);
    }
    let segments = url
        .path_segments()
        .ok_or(GovernanceLocatorError::InvalidUrl)?
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    match (url.scheme(), url.host_str(), segments.as_slice()) {
        ("alexandria", Some("governance"), ["genesis", dao_id])
            if url.path() == format!("/genesis/{dao_id}") =>
        {
            Ok((*dao_id).to_owned())
        }
        ("https", Some(OFFICIAL_APP_LINK_HOST), ["governance", "genesis", dao_id])
            if url.path() == format!("/governance/genesis/{dao_id}") =>
        {
            Ok((*dao_id).to_owned())
        }
        _ => Err(GovernanceLocatorError::InvalidUrl),
    }
}

/// Normalize every location and require each to come from a distinct origin.
///
/// The redundancy rule exists so that one party cannot satisfy the two-source
/// minimum alone. Different paths on one host are still one operator, so
/// sources are counted by origin: the iroh network, or an HTTPS host. The
/// resource path is normalized for a stable canonical form, but it does not
/// make a second source.
fn canonicalize_locations(
    content_hash: &str,
    locations: &mut [String],
) -> Result<(), GovernanceLocatorError> {
    if locations.len() < MIN_GOVERNANCE_GENESIS_LOCATIONS
        || locations.len() > MAX_GOVERNANCE_GENESIS_LOCATIONS
    {
        return Err(GovernanceLocatorError::InvalidLocations);
    }
    let expected_iroh = format!("{IROH_ORIGIN}{content_hash}");
    let mut origins = BTreeSet::new();
    for location in locations.iter_mut() {
        if location.is_empty() || location.len() > MAX_LOCATION_BYTES {
            return Err(GovernanceLocatorError::InvalidLocations);
        }
        let origin = if location == &expected_iroh {
            IROH_ORIGIN.to_owned()
        } else {
            let (canonical, host) = canonical_https_location(content_hash, location)?;
            *location = canonical;
            host
        };
        if !origins.insert(origin) {
            return Err(GovernanceLocatorError::InvalidLocations);
        }
    }
    locations.sort_unstable();
    Ok(())
}

/// Parse one HTTPS source into its canonical spelling and origin host.
fn canonical_https_location(
    content_hash: &str,
    location: &str,
) -> Result<(String, String), GovernanceLocatorError> {
    let mut parsed = Url::parse(location).map_err(|_| GovernanceLocatorError::InvalidLocations)?;
    let expected_fragment = format!("blake3={content_hash}");
    if parsed.fragment() != Some(expected_fragment.as_str()) {
        return Err(GovernanceLocatorError::InvalidLocations);
    }
    // Scheme, userinfo, port, IP-literal and special-use host rules are the
    // resolver's, so what review accepts is exactly what preview will fetch.
    reject_unreviewable_https_source(&parsed)
        .map_err(|_| GovernanceLocatorError::InvalidLocations)?;
    let host = parsed
        .host_str()
        .ok_or(GovernanceLocatorError::InvalidLocations)?
        .to_owned();

    let path = normalize_percent_encoding(parsed.path());
    parsed.set_path(&path);
    match parsed.query().map(normalize_percent_encoding) {
        Some(query) if query.is_empty() => parsed.set_query(None),
        Some(query) => parsed.set_query(Some(&query)),
        None => {}
    }
    let canonical: String = parsed.into();
    let reparsed = Url::parse(&canonical).map_err(|_| GovernanceLocatorError::InvalidLocations)?;
    if reparsed.as_str() != canonical || canonical.len() > MAX_LOCATION_BYTES {
        return Err(GovernanceLocatorError::InvalidLocations);
    }
    Ok((canonical, host))
}

/// RFC 3986 section 6.2.2: decode percent-encoded unreserved characters and
/// uppercase the hex digits of every remaining escape.
fn normalize_percent_encoding(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut normalized = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() + 1 {
            let decoded = bytes
                .get(index + 1..index + 3)
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok());
            if let Some(byte) = decoded {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                    normalized.push(char::from(byte));
                } else {
                    normalized.push_str(&format!("%{byte:02X}"));
                }
                index += 3;
                continue;
            }
        }
        normalized.push(char::from(bytes[index]));
        index += 1;
    }
    normalized
}

fn is_canonical_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: u8) -> String {
        hex::encode([byte; 32])
    }

    fn locator() -> GovernanceGenesisLocator {
        let content_hash = digest(0xab);
        GovernanceGenesisLocator {
            version: GOVERNANCE_GENESIS_LOCATOR_VERSION,
            dao_id: digest(0xcd),
            locations: vec![
                format!("https://mirror.example.org/genesis.json#blake3={content_hash}"),
                format!("iroh://{content_hash}"),
            ],
            content_hash,
        }
    }

    fn with_sources(sources: &[String]) -> GovernanceGenesisLocator {
        let mut locator = locator();
        locator.locations = sources.to_vec();
        locator
    }

    #[test]
    fn compact_and_official_links_parse_to_one_canonical_locator() {
        let expected = locator();
        let encoded = expected.encode().unwrap();
        let parsed = GovernanceGenesisLocator::parse(&encoded).unwrap();
        let mut canonical = expected.clone();
        canonical.locations.sort_unstable();
        assert_eq!(parsed, canonical);

        let official = encoded.replacen(
            "alexandria://governance/genesis/",
            "https://alexandria.ifftu.dev/governance/genesis/",
            1,
        );
        assert_eq!(GovernanceGenesisLocator::parse(&official).unwrap(), parsed);
        assert_eq!(
            parsed.retrieval_identifiers().unwrap(),
            vec![
                parsed.content_hash,
                "https://mirror.example.org/genesis.json".to_string(),
            ]
        );
    }

    #[test]
    fn locator_never_embeds_genesis_and_requires_redundant_content_binding() {
        let encoded = locator().encode().unwrap();
        assert!(!encoded.contains("acceptances"));

        let one_source = encoded.replace(&format!("&source=iroh%3A%2F%2F{}", digest(0xab)), "");
        assert_eq!(
            GovernanceGenesisLocator::parse(&one_source),
            Err(GovernanceLocatorError::InvalidLocations)
        );

        let mismatched = encoded.replace(&format!("blake3%3D{}", digest(0xab)), "blake3%3D00");
        assert_eq!(
            GovernanceGenesisLocator::parse(&mismatched),
            Err(GovernanceLocatorError::InvalidLocations)
        );
    }

    #[test]
    fn malformed_or_ambiguous_locators_fail_closed() {
        let encoded = locator().encode().unwrap();
        assert_eq!(
            GovernanceGenesisLocator::parse(&encoded.replace("content=", "extra=x&content=")),
            Err(GovernanceLocatorError::InvalidQuery)
        );
        assert_eq!(
            GovernanceGenesisLocator::parse(&encoded.replace("alexandria:", "https:")),
            Err(GovernanceLocatorError::InvalidUrl)
        );
        assert_eq!(
            GovernanceGenesisLocator::parse(&encoded.replace("?content", "/?content")),
            Err(GovernanceLocatorError::InvalidUrl)
        );
        assert_eq!(
            GovernanceGenesisLocator::parse(&format!(" {encoded}")),
            Err(GovernanceLocatorError::InvalidUrl)
        );
        assert_eq!(
            GovernanceGenesisLocator::parse(&"x".repeat(MAX_GOVERNANCE_GENESIS_LOCATOR_BYTES + 1)),
            Err(GovernanceLocatorError::TooLarge)
        );

        let hash = digest(0xab);
        let equivalent_mirrors = with_sources(&[
            format!("https://MIRROR.EXAMPLE.ORG:443/genesis.json#blake3={hash}"),
            format!("https://mirror.example.org/genesis.json#blake3={hash}"),
        ]);
        assert_eq!(
            equivalent_mirrors.encode(),
            Err(GovernanceLocatorError::InvalidLocations)
        );
    }

    #[test]
    fn trivially_different_spellings_of_one_origin_are_one_source() {
        let hash = digest(0xab);
        let base = format!("https://mirror.example.org/genesis.json#blake3={hash}");
        for (label, twin) in [
            (
                "percent-encoded unreserved path",
                format!("https://mirror.example.org/%67enesis.json#blake3={hash}"),
            ),
            (
                "dot segment",
                format!("https://mirror.example.org/x/../genesis.json#blake3={hash}"),
            ),
            (
                "empty query",
                format!("https://mirror.example.org/genesis.json?#blake3={hash}"),
            ),
            (
                "trailing-dot host",
                format!("https://mirror.example.org./genesis.json#blake3={hash}"),
            ),
            (
                "default port",
                format!("https://mirror.example.org:443/genesis.json#blake3={hash}"),
            ),
            (
                "host case",
                format!("https://Mirror.Example.ORG/genesis.json#blake3={hash}"),
            ),
            (
                "different path on the same host",
                format!("https://mirror.example.org/copy/genesis.json#blake3={hash}"),
            ),
        ] {
            assert_eq!(
                with_sources(&[base.clone(), twin]).encode(),
                Err(GovernanceLocatorError::InvalidLocations),
                "{label}"
            );
        }

        assert_eq!(
            with_sources(&[format!("iroh://{hash}"), format!("iroh://{hash}")]).encode(),
            Err(GovernanceLocatorError::InvalidLocations)
        );
    }

    #[test]
    fn equivalent_spellings_normalize_to_one_canonical_source() {
        let hash = digest(0xab);
        let parsed = GovernanceGenesisLocator::parse(
            &with_sources(&[
                format!("https://Mirror.Example.ORG:443/x/../%67enesis%2ejson?#blake3={hash}"),
                format!("https://second.example.net/a%2fb?v=%7e1#blake3={hash}"),
            ])
            .encode()
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            parsed.locations,
            vec![
                format!("https://mirror.example.org/genesis.json#blake3={hash}"),
                format!("https://second.example.net/a%2Fb?v=~1#blake3={hash}"),
            ]
        );
        assert_eq!(
            GovernanceGenesisLocator::parse(&parsed.encode().unwrap()).unwrap(),
            parsed
        );
    }

    #[test]
    fn ip_literal_private_special_use_and_nonstandard_sources_are_rejected() {
        let hash = digest(0xab);
        for source in [
            "https://2130706433/g",
            "https://10.0.0.1/g",
            "https://10.0.0.1:8443/g",
            "https://8.8.8.8/g",
            "https://[::1]/g",
            "https://[2001:4860:4860::8888]/g",
            "https://localhost/g",
            "https://mirror.localhost/g",
            "https://intranet/g",
            "https://printer.local/g",
            "https://metadata.google.internal/g",
            "https://router.home.arpa/g",
            "https://hidden.onion/g",
            "https://mirror.example/g",
            "https://mirror.test/g",
            "https://mirror.example.org:8443/g",
            "http://mirror.example.org/g",
            "https://good.host@evil.example.org/g",
        ] {
            assert_eq!(
                with_sources(&[format!("{source}#blake3={hash}"), format!("iroh://{hash}")])
                    .encode(),
                Err(GovernanceLocatorError::InvalidLocations),
                "{source}"
            );
        }
        assert_eq!(
            with_sources(&[
                format!("https://mirror.example.org/g#blake3={hash}"),
                format!("IROH://{hash}"),
            ])
            .encode(),
            Err(GovernanceLocatorError::InvalidLocations)
        );
    }
}
