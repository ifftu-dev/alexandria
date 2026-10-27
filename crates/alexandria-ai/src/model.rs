//! Canonical, provider-independent request and response types.
//!
//! Conversation history is stored in Alexandria in this shape. Adapters
//! translate it to and from their wire format; nothing provider-specific
//! leaks back out except what [`Usage`] and [`FinishReason`] can express.

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// One fully assembled request. The host builds it; adapters only translate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub max_output_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

/// Token counts as the provider reported them. Missing values stay missing;
/// nothing here is estimated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// The model ended its answer.
    Stop,
    /// The output limit cut the answer short.
    Length,
    /// The provider withheld or truncated content.
    ContentFilter,
    /// The stream ended without saying why.
    Unknown,
}

impl FinishReason {
    pub(crate) fn from_wire(value: Option<&str>) -> Self {
        match value {
            Some("stop") | Some("end_turn") | Some("eos") => Self::Stop,
            Some("length") | Some("max_tokens") => Self::Length,
            Some("content_filter") => Self::ContentFilter,
            _ => Self::Unknown,
        }
    }
}

/// Normalized streaming events, in order. `TextDelta` may repeat; the others
/// arrive at most once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    TextDelta { text: String },
    Usage { usage: Usage },
    Finished { reason: FinishReason },
}

/// The assembled result of a request, streamed or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    pub text: String,
    pub usage: Option<Usage>,
    pub finish: FinishReason,
}

/// Bounds applied to every request. Defaults are conservative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest request body we will send.
    pub max_request_bytes: usize,
    /// Largest non-streamed response body we will read.
    pub max_response_bytes: usize,
    /// Largest assembled answer, in bytes of UTF-8 text.
    pub max_output_bytes: usize,
    /// Longest single line of a streamed response.
    pub max_line_bytes: usize,
    /// Longest silence between streamed chunks.
    pub idle_timeout: Duration,
    /// Longest a whole request may take, streamed or not.
    pub total_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_request_bytes: 1_048_576,
            max_response_bytes: 256_000,
            max_output_bytes: 128_000,
            max_line_bytes: 65_536,
            idle_timeout: Duration::from_secs(60),
            total_timeout: Duration::from_secs(600),
        }
    }
}
