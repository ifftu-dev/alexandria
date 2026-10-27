//! One preamble to a model: from a saved connection and its credential to a
//! ready adapter.
//!
//! Every adapter is built here. The chat, the lesson tutor, Studio runs, and
//! provider sign-in all go through [`Connection::resolve`] and
//! [`Resolved::connect`] (or [`connect`], which does both): the endpoint is
//! checked against the policy for the connection's declared location and
//! the vendor terms, resolved, and pinned; then a credential is required for
//! a remote endpoint, admitted under the vendor terms, and the caller's
//! limits are applied. Adapter constructors are private to this crate, so
//! no adapter skips these checks. Nothing here reads storage: the host looks
//! the credential up and passes it in, and resolving first lets it refuse a
//! bad endpoint before it renews a sign-in.
//!
//! Out of scope on purpose: the Optional AI checks client
//! (`alexandria-decisions`, built by the app's `pinned_http`). That crate
//! must stay MIT/Apache because `alexandria-cloud` vendors it, so it cannot
//! depend on this AGPL crate or its adapters, and it speaks its own non-chat
//! protocol. It shares only endpoint resolution, through
//! [`Endpoint::http_client`](crate::Endpoint::http_client) (AI chat spec,
//! section 26).

use url::Url;

use crate::endpoint::{self, Endpoint, Location};
use crate::model::Limits;
use crate::{terms, Adapter, Error, Protocol, Result};

/// What this crate needs to know about a saved connection: its base URL,
/// where the user says it runs, and the protocol it speaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub endpoint: String,
    pub location: Location,
    pub protocol: Protocol,
}

impl Connection {
    /// Checks the endpoint against its declared location and the vendor
    /// terms, then resolves and pins it. Needs no credential and sends
    /// nothing.
    pub async fn resolve(&self) -> Result<Resolved> {
        let base = endpoint::parse(&self.endpoint, self.location)?;
        Ok(Resolved {
            endpoint: endpoint::resolve_parsed(base, self.location).await?,
            protocol: self.protocol,
        })
    }
}

/// A connection whose endpoint passed the policy and is pinned to the
/// addresses it resolved to. Nothing has been sent.
#[derive(Debug)]
pub struct Resolved {
    endpoint: Endpoint,
    protocol: Protocol,
}

impl Resolved {
    /// Admits the credential and returns an adapter bound to `limits`. A
    /// blank credential counts as none, and a remote endpoint needs one.
    pub fn connect(self, credential: Option<&str>, limits: Limits) -> Result<Adapter> {
        let credential = credential.filter(|c| !c.trim().is_empty());
        match credential {
            Some(credential) => admit(self.endpoint.base(), self.protocol, credential)?,
            None if self.endpoint.location() == Location::Remote => {
                return Err(Error::Invalid(
                    "Add an API key for this cloud connection".into(),
                ))
            }
            None => {}
        }
        let adapter = Adapter::new(self.protocol, self.endpoint, credential.map(str::to_owned));
        Ok(adapter.with_limits(limits))
    }
}

/// Admits a credential for a connection, before it is stored or sent. The
/// host's connection store applies this when saving; [`Resolved::connect`]
/// applies it before every adapter. A token from OpenAI's own "Sign in with
/// ChatGPT" flow is the one sanctioned sign-in, and it is only ever spoken
/// over `responses`.
pub fn admit(base: &Url, protocol: Protocol, credential: &str) -> Result<()> {
    terms::check_credential(base, credential, protocol == Protocol::Responses)
}

/// [`Connection::resolve`] then [`Resolved::connect`], for a caller that
/// already holds its credential.
pub async fn connect(
    connection: &Connection,
    credential: Option<&str>,
    limits: Limits,
) -> Result<Adapter> {
    connection.resolve().await?.connect(credential, limits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChatRequest, Message};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Serves one HTTP response and reports the raw request it received.
    async fn server(response: &'static str) -> (u16, tokio::sync::oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1 << 16];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let _ = seen_tx.send(String::from_utf8_lossy(&buf[..n]).into_owned());
            let _ = socket.write_all(response.as_bytes()).await;
        });
        (port, seen_rx)
    }

    fn request() -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            messages: vec![Message::user("hello")],
            max_output_tokens: 16,
            temperature: None,
        }
    }

    fn device(port: u16, host: &str) -> Connection {
        Connection {
            endpoint: format!("http://{host}:{port}/v1"),
            location: Location::Device,
            protocol: Protocol::Compatible,
        }
    }

    #[tokio::test]
    async fn a_device_connection_is_pinned_to_loopback_and_sends_its_key() {
        let body = r#"{"choices":[{"message":{"content":"hi"},"finish_reason":"stop"}]}"#;
        let (port, seen) = server(Box::leak(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_boxed_str(),
        ))
        .await;
        let adapter = connect(
            &device(port, "localhost"),
            Some("sk-test"),
            Limits::default(),
        )
        .await
        .unwrap();
        let pinned = adapter.endpoint().pinned_addrs().unwrap();
        assert!(
            pinned
                .iter()
                .all(|a| a.ip().is_loopback() && a.port() == port),
            "{pinned:?}"
        );
        let completion = adapter.complete(&request()).await.unwrap();
        assert_eq!(completion.text, "hi");
        let seen = seen.await.unwrap();
        assert!(seen.starts_with("POST /v1/chat/completions"), "{seen}");
        assert!(
            seen.to_ascii_lowercase()
                .contains("authorization: bearer sk-test"),
            "{seen}"
        );
    }

    fn remote(endpoint: &str, protocol: Protocol) -> Connection {
        Connection {
            endpoint: endpoint.into(),
            location: Location::Remote,
            protocol,
        }
    }

    fn invalid(error: crate::Error) -> String {
        match error {
            crate::Error::Invalid(message) => message,
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn consumer_tokens_and_consumer_backends_are_refused_before_anything_is_sent() {
        // None of these reach the resolver: the refusal is immediate.
        let anthropic = remote("https://api.anthropic.com/v1", Protocol::Anthropic);
        let message = invalid(
            connect(&anthropic, Some("sk-ant-oat01-abc"), Limits::default())
                .await
                .unwrap_err(),
        );
        assert!(message.contains("subscription sign-in token"), "{message}");

        let openai = remote("https://api.openai.com/v1", Protocol::Compatible);
        let jwt = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ1In0.c2lnbmF0dXJl";
        let message = invalid(
            connect(&openai, Some(jwt), Limits::default())
                .await
                .unwrap_err(),
        );
        assert!(message.contains("ChatGPT sign-in token"), "{message}");

        let backend = remote("https://chatgpt.com/backend-api", Protocol::Compatible);
        let message = invalid(
            connect(&backend, Some("sk-x"), Limits::default())
                .await
                .unwrap_err(),
        );
        assert!(message.contains("consumer app"), "{message}");
    }

    #[test]
    fn the_sanctioned_sign_in_is_admitted_only_over_responses() {
        let openai = Url::parse("https://api.openai.com/v1/").unwrap();
        let jwt = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ1In0.c2lnbmF0dXJl";
        assert!(admit(&openai, Protocol::Responses, jwt).is_ok());
        assert!(admit(&openai, Protocol::Compatible, jwt).is_err());
        assert!(admit(&openai, Protocol::Compatible, "sk-proj-abc").is_ok());
        let anthropic = Url::parse("https://api.anthropic.com/v1/").unwrap();
        assert!(admit(&anthropic, Protocol::Responses, "sk-ant-oat01-abc").is_err());
    }

    #[tokio::test]
    async fn a_remote_connection_refuses_loopback_and_private_addresses() {
        for endpoint in [
            "https://127.0.0.1:1/v1",
            "https://localhost/v1",
            "https://10.0.0.5/v1",
            "https://[fd00::1]/v1",
            "http://api.openai.com/v1",
        ] {
            let message = invalid(
                connect(
                    &remote(endpoint, Protocol::Compatible),
                    Some("sk-x"),
                    Limits::default(),
                )
                .await
                .unwrap_err(),
            );
            assert!(message.contains("public address"), "{endpoint}: {message}");
        }
    }

    #[tokio::test]
    async fn limits_bound_what_the_adapter_reads() {
        let body = format!(
            r#"{{"choices":[{{"message":{{"content":"{}"}}}}]}}"#,
            "x".repeat(600)
        );
        let (port, _seen) = server(Box::leak(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_boxed_str(),
        ))
        .await;
        let limits = Limits {
            max_response_bytes: 200,
            ..Limits::default()
        };
        let adapter = connect(&device(port, "127.0.0.1"), None, limits)
            .await
            .unwrap();
        assert_eq!(
            adapter.complete(&request()).await.unwrap_err(),
            crate::Error::TooLarge
        );
    }

    #[tokio::test]
    async fn transport_failures_arrive_typed_without_provider_text() {
        // A port nothing listens on.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed = listener.local_addr().unwrap().port();
        drop(listener);
        let adapter = connect(&device(closed, "127.0.0.1"), None, Limits::default())
            .await
            .unwrap();
        assert_eq!(
            adapter.complete(&request()).await.unwrap_err(),
            crate::Error::Unreachable
        );

        // A server that accepts and then says nothing.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let silent = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        });
        let limits = Limits {
            total_timeout: std::time::Duration::from_millis(200),
            idle_timeout: std::time::Duration::from_millis(100),
            ..Limits::default()
        };
        let adapter = connect(&device(silent, "127.0.0.1"), None, limits)
            .await
            .unwrap();
        assert_eq!(
            adapter.complete(&request()).await.unwrap_err(),
            crate::Error::Timeout
        );

        // A status with a body that must never reach the learner.
        let (port, _seen) = server(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 7\r\nContent-Length: 29\r\nConnection: close\r\n\r\nsensitive provider diagnostic",
        )
        .await;
        let adapter = connect(
            &device(port, "127.0.0.1"),
            Some("sk-secret"),
            Limits::default(),
        )
        .await
        .unwrap();
        let error = adapter.complete(&request()).await.unwrap_err();
        assert_eq!(
            error,
            crate::Error::Http {
                status: 429,
                retry_after: Some(7)
            }
        );
        let shown = error.to_string();
        assert!(
            !shown.contains("sensitive") && !shown.contains("sk-secret"),
            "{shown}"
        );
    }

    #[tokio::test]
    async fn a_remote_connection_needs_a_key_and_blank_is_none() {
        // A public literal resolves without the network.
        let resolved = || async {
            remote("https://8.8.8.8/v1", Protocol::Compatible)
                .resolve()
                .await
                .unwrap()
        };
        for missing in [None, Some(""), Some("   ")] {
            let message = invalid(
                resolved()
                    .await
                    .connect(missing, Limits::default())
                    .unwrap_err(),
            );
            assert_eq!(message, "Add an API key for this cloud connection");
        }
        assert!(resolved()
            .await
            .connect(Some("sk-x"), Limits::default())
            .is_ok());
        // A model on the device or the user's own network may need none.
        let (port, _seen) = server("").await;
        assert!(device(port, "127.0.0.1")
            .resolve()
            .await
            .unwrap()
            .connect(Some("  "), Limits::default())
            .is_ok());
    }

    #[tokio::test]
    async fn the_endpoint_is_checked_before_any_credential_is_needed() {
        // So a host can refuse a bad endpoint before it renews a sign-in.
        for (endpoint, location) in [
            ("https://10.0.0.5/v1", Location::Remote),
            ("https://chatgpt.com/backend-api", Location::Remote),
            ("http://192.168.1.4/v1", Location::Device),
        ] {
            let connection = Connection {
                endpoint: endpoint.into(),
                location,
                protocol: Protocol::Compatible,
            };
            assert!(
                matches!(connection.resolve().await, Err(crate::Error::Invalid(_))),
                "{endpoint}"
            );
        }
    }
}
