//! Provider-independent model access for Alexandria.
//!
//! This crate owns the parts of talking to a model that do not depend on who
//! is asking or why: where a request may go ([`endpoint`]), what a request and
//! its streamed answer look like ([`model`]), and how one wire protocol is
//! spoken ([`compatible`]). It knows nothing about profiles, courses, learner
//! records, or the webview. The host resolves context, reads secrets, and
//! hands an immutable [`model::ChatRequest`] to an adapter.
//!
//! Every failure is an [`Error`] whose message is safe to show a learner: it
//! never contains a key, a request body, or a provider's response body.

pub mod compatible;
pub mod endpoint;
pub mod model;
mod sse;

pub use endpoint::{Endpoint, Location};
pub use model::{ChatRequest, Completion, FinishReason, Message, Role, StreamEvent, Usage};

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
}

impl Error {
    /// Whether the provider may have accepted (and billed) the request even
    /// though we did not get a complete answer. Callers must not retry these
    /// automatically. A status response means the provider refused it.
    pub fn may_have_been_accepted(&self) -> bool {
        !matches!(
            self,
            Error::Invalid(_) | Error::Unreachable | Error::Unsupported(_) | Error::Http { .. }
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
