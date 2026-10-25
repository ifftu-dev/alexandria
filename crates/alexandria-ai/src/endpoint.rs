//! Where a model request may go.
//!
//! A connection declares one of three locations, and the endpoint must agree
//! with that declaration before anything is sent:
//!
//! * [`Location::Device`]: a loopback address on this machine. Plain HTTP is
//!   allowed because the traffic never leaves the device.
//! * [`Location::PrivateNetwork`]: a server the user controls on a private,
//!   link-local, or carrier-grade NAT range. The request leaves this device.
//! * [`Location::Remote`]: a public internet endpoint. HTTPS is required.
//!
//! Host names are resolved once, every resolved address must belong to the
//! declared location, and the request is then pinned to those addresses. A
//! name that later re-resolves elsewhere (DNS rebinding) cannot redirect a
//! request carrying learner context or a key. Redirects are never followed.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::{Error, Result};

/// Where a connection's endpoint runs, as declared by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Location {
    Device,
    PrivateNetwork,
    Remote,
}

impl Location {
    /// Maps the Studio connection vocabulary (`local` / `cloud`).
    pub fn from_legacy(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Device),
            "cloud" => Some(Self::Remote),
            "device" => Some(Self::Device),
            "private_network" => Some(Self::PrivateNetwork),
            "remote" => Some(Self::Remote),
            _ => None,
        }
    }

    /// The stable name chat uses for this place; equal to its serde form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::PrivateNetwork => "private_network",
            Self::Remote => "remote",
        }
    }

    /// Whether a request to this location leaves the device.
    pub fn leaves_device(self) -> bool {
        !matches!(self, Self::Device)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddrClass {
    Loopback,
    Private,
    Public,
    Forbidden,
}

fn classify(ip: IpAddr) -> AddrClass {
    match ip {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => classify_v4(v4),
            None => classify_v6(v6),
        },
    }
}

fn classify_v4(ip: Ipv4Addr) -> AddrClass {
    let [a, b, ..] = ip.octets();
    if ip.is_loopback() {
        AddrClass::Loopback
    } else if ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast() || a == 0 {
        AddrClass::Forbidden
    } else if ip.is_private() || ip.is_link_local() || (a == 100 && (64..128).contains(&b)) {
        // 100.64.0.0/10 is carrier-grade NAT, used by overlay networks such
        // as Tailscale for machines the user controls.
        AddrClass::Private
    } else {
        AddrClass::Public
    }
}

fn classify_v6(ip: Ipv6Addr) -> AddrClass {
    let first = ip.segments()[0];
    if ip.is_loopback() {
        AddrClass::Loopback
    } else if ip.is_unspecified() || ip.is_multicast() {
        AddrClass::Forbidden
    } else if (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80 {
        // fc00::/7 unique local, fe80::/10 link local.
        AddrClass::Private
    } else {
        AddrClass::Public
    }
}

fn allowed(location: Location, class: AddrClass) -> bool {
    matches!(
        (location, class),
        (Location::Device, AddrClass::Loopback)
            | (Location::PrivateNetwork, AddrClass::Private)
            | (Location::Remote, AddrClass::Public)
    )
}

fn mismatch(location: Location) -> Error {
    Error::Invalid(
        match location {
            Location::Device => "On-device endpoints must use a loopback address such as localhost",
            Location::PrivateNetwork => {
                "Private-network endpoints must resolve only to private or link-local addresses"
            }
            Location::Remote => {
                "Remote endpoints must use HTTPS and a public address; choose Private network for servers on your own network"
            }
        }
        .into(),
    )
}

/// Syntactic validation that needs no network access. Returns the base URL
/// with a trailing slash so relative paths join beneath it.
pub fn parse(raw: &str, location: Location) -> Result<Url> {
    let mut url =
        Url::parse(raw.trim()).map_err(|_| Error::Invalid("Enter a valid API base URL".into()))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Invalid(
            "API base URLs cannot contain credentials, query strings, or fragments".into(),
        ));
    }
    let scheme_ok = match location {
        Location::Device | Location::PrivateNetwork => matches!(url.scheme(), "http" | "https"),
        Location::Remote => url.scheme() == "https",
    };
    if !scheme_ok {
        return Err(mismatch(location));
    }
    match url.host() {
        None => return Err(Error::Invalid("Enter a valid API base URL".into())),
        Some(Host::Ipv4(ip)) => {
            if !allowed(location, classify(IpAddr::V4(ip))) {
                return Err(mismatch(location));
            }
        }
        Some(Host::Ipv6(ip)) => {
            if !allowed(location, classify(IpAddr::V6(ip))) {
                return Err(mismatch(location));
            }
        }
        Some(Host::Domain(domain)) => {
            let domain = domain.trim_end_matches('.').to_ascii_lowercase();
            let is_localhost = domain == "localhost" || domain.ends_with(".localhost");
            match location {
                Location::Device if domain != "localhost" => return Err(mismatch(location)),
                Location::PrivateNetwork | Location::Remote if is_localhost => {
                    return Err(mismatch(location))
                }
                _ => {}
            }
        }
    }
    crate::terms::check_endpoint(&url)?;
    let path = format!("{}/", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url)
}

/// An endpoint that passed both syntactic and address checks, pinned to the
/// addresses it resolved to.
#[derive(Debug, Clone)]
pub struct Endpoint {
    base: Url,
    location: Location,
    pin: Option<(String, Vec<SocketAddr>)>,
}

impl Endpoint {
    pub fn base(&self) -> &Url {
        &self.base
    }

    pub fn location(&self) -> Location {
        self.location
    }

    /// The addresses requests are pinned to, if the host was a name.
    pub fn pinned_addrs(&self) -> Option<&[SocketAddr]> {
        self.pin.as_ref().map(|(_, addrs)| addrs.as_slice())
    }

    /// Joins a relative API path beneath the base URL.
    pub fn join(&self, path: &str) -> Result<Url> {
        self.base
            .join(path)
            .map_err(|_| Error::Invalid("Invalid model endpoint path".into()))
    }

    /// An HTTP client bound to this endpoint's policy, for a host that speaks
    /// its own protocol to the same host (the Optional AI checks). Requests
    /// to this endpoint's host go only to the pinned addresses, redirects are
    /// not followed, and local traffic bypasses any system proxy. The pin is
    /// fixed when the endpoint was resolved; rebuild periodically for a
    /// long-lived client.
    pub fn http_client(&self, total_timeout: Option<Duration>) -> Result<reqwest::Client> {
        self.client(total_timeout)
    }

    /// An HTTP client bound to this endpoint's policy.
    pub(crate) fn client(&self, total_timeout: Option<Duration>) -> Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none());
        if let Some(timeout) = total_timeout {
            builder = builder.timeout(timeout);
        }
        if self.location != Location::Remote {
            // A system proxy would carry loopback or LAN traffic somewhere else.
            builder = builder.no_proxy();
        }
        // For remote endpoints behind a user-configured proxy, the proxy
        // resolves the host itself, so the pin below does not apply there.
        // The proxy is the user's own trusted configuration.
        if let Some((host, addrs)) = &self.pin {
            builder = builder.resolve_to_addrs(host, addrs);
        }
        builder
            .build()
            .map_err(|_| Error::Invalid("Could not initialize the model connection".into()))
    }
}

/// Validates `raw` for `location`, resolving and pinning host names.
pub async fn resolve(raw: &str, location: Location) -> Result<Endpoint> {
    let base = parse(raw, location)?;
    let port = base
        .port_or_known_default()
        .ok_or_else(|| Error::Invalid("Enter a valid API base URL".into()))?;
    let pin = match base.host() {
        Some(Host::Domain(domain)) => {
            let domain = domain.to_string();
            let addrs: Vec<SocketAddr> = if location == Location::Device {
                // `localhost` is the only name allowed here. Pin it rather than
                // trusting whatever the resolver says this time.
                vec![
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
                    SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
                ]
            } else {
                let found = tokio::time::timeout(
                    Duration::from_secs(5),
                    tokio::net::lookup_host((domain.as_str(), port)),
                )
                .await
                .map_err(|_| Error::Unreachable)?
                .map_err(|_| Error::Unreachable)?
                .collect::<Vec<_>>();
                check_resolved(location, &found)?;
                found
            };
            Some((domain, addrs))
        }
        _ => None,
    };
    Ok(Endpoint {
        base,
        location,
        pin,
    })
}

/// Every resolved address must belong to the declared location. One stray
/// address is enough to refuse: the client could connect to any of them.
fn check_resolved(location: Location, addrs: &[SocketAddr]) -> Result<()> {
    if addrs.is_empty() {
        return Err(Error::Unreachable);
    }
    if addrs.iter().all(|a| allowed(location, classify(a.ip()))) {
        Ok(())
    } else {
        Err(mismatch(location))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn device_accepts_only_loopback() {
        for ok in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:1234",
            "https://[::1]:8080/v1",
            "http://LOCALHOST/v1",
        ] {
            assert!(parse(ok, Location::Device).is_ok(), "{ok}");
        }
        for bad in [
            "http://127.0.0.1.evil.test/v1",
            "http://192.168.1.4:1234/v1",
            "http://example.com/v1",
            "http://api.localhost/v1",
            "ftp://localhost/v1",
            "http://0.0.0.0:11434",
        ] {
            assert!(parse(bad, Location::Device).is_err(), "{bad}");
        }
    }

    #[test]
    fn remote_requires_https_and_public_literals() {
        assert!(parse("https://api.openai.com/v1", Location::Remote).is_ok());
        assert!(parse("https://8.8.8.8/v1", Location::Remote).is_ok());
        for bad in [
            "http://api.openai.com/v1",
            "https://localhost/v1",
            "https://10.0.0.5/v1",
            "https://[fd00::1]/v1",
            "https://169.254.169.254/latest",
            "https://[::ffff:127.0.0.1]/v1",
        ] {
            assert!(parse(bad, Location::Remote).is_err(), "{bad}");
        }
    }

    #[test]
    fn private_network_accepts_lan_and_overlay_ranges() {
        for ok in [
            "http://192.168.1.40:1234/v1",
            "http://10.1.2.3/v1",
            "http://172.20.0.2/v1",
            "http://100.101.102.103:11434",
            "http://[fe80::1]/v1",
            "http://studio.local:1234/v1",
        ] {
            assert!(parse(ok, Location::PrivateNetwork).is_ok(), "{ok}");
        }
        for bad in [
            "http://127.0.0.1/v1",
            "http://localhost/v1",
            "http://8.8.8.8/v1",
            "http://100.128.0.1/v1",
        ] {
            assert!(parse(bad, Location::PrivateNetwork).is_err(), "{bad}");
        }
    }

    #[test]
    fn urls_cannot_smuggle_credentials_or_parameters() {
        for bad in [
            "http://user:pass@localhost/v1",
            "http://localhost/v1?key=secret",
            "http://localhost/v1#frag",
        ] {
            assert!(parse(bad, Location::Device).is_err(), "{bad}");
        }
    }

    #[test]
    fn base_path_gains_a_trailing_slash() {
        let url = parse("http://localhost:11434/v1", Location::Device).unwrap();
        assert_eq!(url.as_str(), "http://localhost:11434/v1/");
        assert_eq!(
            url.join("chat/completions").unwrap().as_str(),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn one_stray_resolved_address_refuses_the_endpoint() {
        let lan = [addr("192.168.1.40:1234"), addr("[fd00::40]:1234")];
        assert!(check_resolved(Location::PrivateNetwork, &lan).is_ok());
        let rebound = [addr("192.168.1.40:1234"), addr("93.184.216.34:1234")];
        assert!(check_resolved(Location::PrivateNetwork, &rebound).is_err());
        let to_loopback = [addr("127.0.0.1:443")];
        assert!(check_resolved(Location::Remote, &to_loopback).is_err());
        let to_lan = [addr("10.0.0.8:443")];
        assert!(check_resolved(Location::Remote, &to_lan).is_err());
        assert!(check_resolved(Location::Remote, &[addr("93.184.216.34:443")]).is_ok());
        assert!(check_resolved(Location::Remote, &[]).is_err());
    }

    #[tokio::test]
    async fn localhost_is_pinned_without_consulting_the_resolver() {
        let endpoint = resolve("http://localhost:11434/v1", Location::Device)
            .await
            .unwrap();
        let pinned = endpoint.pinned_addrs().unwrap();
        assert!(pinned
            .iter()
            .all(|a| a.ip().is_loopback() && a.port() == 11434));
    }

    #[test]
    fn legacy_locations_map() {
        assert_eq!(Location::from_legacy("local"), Some(Location::Device));
        assert_eq!(Location::from_legacy("cloud"), Some(Location::Remote));
        assert_eq!(Location::from_legacy("lan"), None);
        assert!(!Location::Device.leaves_device());
        assert!(Location::PrivateNetwork.leaves_device());
    }
}
