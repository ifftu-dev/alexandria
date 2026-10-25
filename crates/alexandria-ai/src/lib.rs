//! Provider-independent model access for Alexandria.
//!
//! This crate owns the parts of talking to a model that do not depend on who
//! is asking or why: where a request may go ([`endpoint`]), what a request and
//! its streamed answer look like ([`model`]), what vendors allow
//! ([`terms`]), and how each wire protocol is spoken ([`compatible`],
//! [`anthropic`], [`responses`]). [`connect`] is the one way from a saved
//! connection and its credential to an [`Adapter`], which dispatches by
//! [`Protocol`]. The crate knows nothing about profiles, courses, learner
//! records, or the webview. The host resolves context, reads secrets, calls
//! [`connect`], and hands an immutable [`model::ChatRequest`] to the adapter.
//!
//! Every failure is an [`Error`] whose message is safe to show a learner: it
//! never contains a key, a request body, or a provider's response body.

pub mod anthropic;
pub mod compatible;
pub mod connection;
pub mod endpoint;
mod http;
pub mod model;
pub mod responses;
mod sse;
pub mod terms;

pub use connection::{connect, Connection};
pub use endpoint::{Endpoint, Location};
pub use model::{ChatRequest, Completion, FinishReason, Message, Role, StreamEvent, Usage};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// The wire protocol a connection speaks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// OpenAI-compatible chat completions.
    #[default]
    Compatible,
    /// The Anthropic Messages API.
    Anthropic,
    /// The OpenAI Responses API, used with a ChatGPT plan sign-in.
    Responses,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Compatible => "compatible",
            Protocol::Anthropic => "anthropic",
            Protocol::Responses => "responses",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "compatible" => Some(Protocol::Compatible),
            "anthropic" => Some(Protocol::Anthropic),
            "responses" => Some(Protocol::Responses),
            _ => None,
        }
    }
}

/// One adapter for a resolved endpoint, whichever protocol it speaks.
#[derive(Debug)]
pub enum Adapter {
    Compatible(compatible::Compatible),
    Anthropic(anthropic::Anthropic),
    Responses(responses::Responses),
}

impl Adapter {
    pub(crate) fn new(protocol: Protocol, endpoint: Endpoint, key: Option<String>) -> Self {
        match protocol {
            Protocol::Compatible => Adapter::Compatible(compatible::Compatible::new(endpoint, key)),
            Protocol::Anthropic => Adapter::Anthropic(anthropic::Anthropic::new(endpoint, key)),
            Protocol::Responses => Adapter::Responses(responses::Responses::new(endpoint, key)),
        }
    }

    pub(crate) fn with_limits(self, limits: model::Limits) -> Self {
        match self {
            Adapter::Compatible(a) => Adapter::Compatible(a.with_limits(limits)),
            Adapter::Anthropic(a) => Adapter::Anthropic(a.with_limits(limits)),
            Adapter::Responses(a) => Adapter::Responses(a.with_limits(limits)),
        }
    }

    pub fn endpoint(&self) -> &Endpoint {
        match self {
            Adapter::Compatible(a) => a.endpoint(),
            Adapter::Anthropic(a) => a.endpoint(),
            Adapter::Responses(a) => a.endpoint(),
        }
    }

    pub async fn complete(&self, request: &ChatRequest) -> Result<Completion> {
        match self {
            Adapter::Compatible(a) => a.complete(request).await,
            Adapter::Anthropic(a) => a.complete(request).await,
            Adapter::Responses(a) => a.complete(request).await,
        }
    }

    pub async fn stream(
        &self,
        request: &ChatRequest,
        events: mpsc::Sender<StreamEvent>,
    ) -> Result<Completion> {
        match self {
            Adapter::Compatible(a) => a.stream(request, events).await,
            Adapter::Anthropic(a) => a.stream(request, events).await,
            Adapter::Responses(a) => a.stream(request, events).await,
        }
    }

    pub async fn list_models(&self) -> Result<Vec<String>> {
        match self {
            Adapter::Compatible(a) => a.list_models().await,
            Adapter::Anthropic(a) => a.list_models().await,
            Adapter::Responses(a) => a.list_models().await,
        }
    }
}

/// A sanitized failure. Display strings are suitable for the UI.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum Error {
    /// The configuration itself is unusable; nothing was sent.
    #[error("{0}")]
    Invalid(String),
    /// The endpoint could not be reached at all.
    #[error("Could not reach the selected model")]
    Unreachable,
    /// The endpoint stopped responding.
    #[error("Model request timed out")]
    Timeout,
    /// The connection broke after the request was sent, or the stream ended
    /// before the provider said it was finished.
    #[error("Model response interrupted")]
    Interrupted,
    /// The provider answered with a non-success status. The body is dropped.
    #[error("Model returned HTTP {status}; check the connection and provider allowance")]
    Http {
        status: u16,
        /// Seconds the provider asked us to wait, when it said so.
        retry_after: Option<u64>,
    },
    /// The provider's answer did not follow the protocol.
    #[error("Model returned an invalid response")]
    Malformed,
    /// The answer exceeded a configured bound and was cut off.
    #[error("Model response exceeds the size limit")]
    TooLarge,
    /// The provider finished without producing any text.
    #[error("The model returned no text")]
    Empty,
    /// The provider reported an error inside an otherwise valid stream.
    #[error("The model stopped with an error")]
    Provider,
    /// The endpoint does not offer this operation.
    #[error("This connection does not support {0}")]
    Unsupported(&'static str),
    /// The consumer stopped listening before the stream finished.
    #[error("The response was stopped")]
    Cancelled,
    /// A signed-in plan's usage allowance is used up for now.
    #[error("The plan's usage limit was reached")]
    UsageLimit,
    /// The signed-in account's plan cannot be used for this request.
    #[error("This account's plan cannot be used here")]
    NotEligible,
}

impl Error {
    /// Whether the provider may have accepted (and billed) the request even
    /// though we did not get a complete answer. Callers must not retry these
    /// automatically. A status response means the provider refused it.
    pub fn may_have_been_accepted(&self) -> bool {
        !matches!(
            self,
            Error::Invalid(_)
                | Error::Unreachable
                | Error::Unsupported(_)
                | Error::Http { .. }
                | Error::NotEligible
        )
    }

    /// Whether the status means the credentials were refused.
    pub fn is_auth(&self) -> bool {
        matches!(
            self,
            Error::Http {
                status: 401 | 403,
                ..
            }
        )
    }
}

pub type Result<T> = std::result::Result<T, Error>;
