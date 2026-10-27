//! The OpenAI Responses protocol, as used with a ChatGPT plan.
//!
//! A user who signs in with ChatGPT lets Alexandria send requests that count
//! against their ChatGPT plan instead of an API key. That route accepts only
//! `POST /v1/responses`, streamed, with nothing stored server-side, and it
//! rejects sampling and output-length fields (OpenAI, "Sign in with ChatGPT",
//! Preview limitations). This adapter therefore always streams, always sends
//! `store: false`, and never sends `temperature` or `max_output_tokens`; the
//! visible answer stays bounded by [`Limits::max_output_bytes`].
//!
//! Translation from the canonical shape: system messages are joined into
//! `instructions` (explicit system items are rejected on this route), and the
//! remaining turns become `input` messages in order.
//!
//! Plan errors that need their own handling are recognised by their stable
//! `error.code`: an exhausted allowance is [`Error::UsageLimit`], an account
//! that cannot use its plan here is [`Error::NotEligible`]. No other part of
//! an error body is read or shown.

use std::fmt;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::endpoint::Endpoint;
use crate::http::{http_error, model_ids, next_chunk, send, send_error};
use crate::model::{ChatRequest, Completion, FinishReason, Limits, Role, StreamEvent, Usage};
use crate::sse::{Decoder, Frame};
use crate::{Error, Result};

/// Most bytes of an error body read to find its code.
const ERROR_BODY_BYTES: usize = 16 * 1024;

/// Most bytes of a model list. The plan route describes each model in
/// detail (instructions, tool settings), about 570 KB for nine models in
/// October 2026, well past the default response bound.
const MODEL_LIST_BYTES: usize = 4 * 1024 * 1024;

/// Room on one event line beyond the answer itself: `response.completed`
/// repeats the whole answer alongside usage and item metadata.
const COMPLETED_EVENT_OVERHEAD: usize = 64 * 1024;

/// A connection to one Responses endpoint. The token is never printed.
pub struct Responses {
    endpoint: Endpoint,
    key: Option<String>,
    limits: Limits,
}

impl fmt::Debug for Responses {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Responses")
            .field("endpoint", &self.endpoint.base().as_str())
            .field("location", &self.endpoint.location())
            .field("has_key", &self.key.is_some())
            .finish()
    }
}

impl Responses {
    pub(crate) fn new(endpoint: Endpoint, key: Option<String>) -> Self {
        Self {
            endpoint,
            key: key.filter(|k| !k.is_empty()),
            limits: Limits::default(),
        }
    }

    pub(crate) fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// One complete answer. The route only streams, so this streams and
    /// collects.
    pub async fn complete(&self, request: &ChatRequest) -> Result<Completion> {
        let (tx, mut rx) = mpsc::channel(64);
        let drain = async { while rx.recv().await.is_some() {} };
        let (completion, ()) = tokio::join!(self.stream(request, tx), drain);
        completion
    }

    /// Streams an answer, with the same contract as
    /// [`crate::compatible::Compatible::stream`]. Success requires
    /// `response.completed`; a stream that ends without it is interrupted.
    pub async fn stream(
        &self,
        request: &ChatRequest,
        events: mpsc::Sender<StreamEvent>,
    ) -> Result<Completion> {
        let body = body(request)?;
        let bytes = serde_json::to_vec(&body).map_err(|_| Error::Malformed)?;
        if bytes.len() > self.limits.max_request_bytes {
            return Err(Error::Invalid("This request is too large to send".into()));
        }
        let client = self.endpoint.client(Some(self.limits.total_timeout))?;
        let builder = client
            .post(self.endpoint.join("responses")?)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .body(bytes);
        let mut response = self.auth(builder).send().await.map_err(send_error)?;
        if !response.status().is_success() {
            return Err(status_error(&mut response).await);
        }

        // `response.completed` carries the full answer on one line, so a line
        // may be as long as the largest answer allowed, plus metadata.
        let mut decoder = Decoder::new(
            self.limits
                .max_line_bytes
                .max(self.limits.max_output_bytes * 2 + COMPLETED_EVENT_OVERHEAD),
        );
        let mut state = StreamState::default();
        loop {
            let chunk = next_chunk(&mut response, &self.limits).await?;
            let (frames, body_ended) = match chunk {
                Some(bytes) => (decoder.push(&bytes)?, false),
                None => (decoder.finish()?, true),
            };
            for frame in frames {
                if self.apply(frame, &mut state, &events).await? {
                    return self.finish(state, &events).await;
                }
            }
            if body_ended {
                return Err(Error::Interrupted);
            }
        }
    }

    /// Lists the models this account may use, in the server's order. The
    /// plan route answers with `models[]` entries carrying `slug` and
    /// `visibility`; only listed ones are offered.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let client = self
            .endpoint
            .client(Some(std::time::Duration::from_secs(20)))?;
        let mut response = self
            .auth(client.get(self.endpoint.join("models")?))
            .send()
            .await
            .map_err(send_error)?;
        match response.status().as_u16() {
            404 | 405 | 501 => return Err(Error::Unsupported("model discovery")),
            s if !(200..300).contains(&s) => return Err(status_error(&mut response).await),
            _ => {}
        }
        let limits = Limits {
            max_response_bytes: self.limits.max_response_bytes.max(MODEL_LIST_BYTES),
            ..self.limits
        };
        let bytes = crate::http::read_bounded(&mut response, &limits).await?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)?;
        let Some(models) = value.get("models").and_then(Value::as_array) else {
            return model_ids(&value);
        };
        let mut slugs: Vec<String> = Vec::new();
        for m in models {
            if m.get("visibility").and_then(Value::as_str) != Some("list") {
                continue;
            }
            if let Some(slug) = m.get("slug").and_then(Value::as_str) {
                if !slug.is_empty() && slug.len() <= 200 && !slugs.iter().any(|s| s == slug) {
                    slugs.push(slug.to_owned());
                }
            }
            if slugs.len() >= 500 {
                break;
            }
        }
        Ok(slugs)
    }

    fn auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.key {
            Some(key) => builder.bearer_auth(key),
            None => builder,
        }
    }

    /// Applies one frame. Returns true at `response.completed` or
    /// `response.incomplete`.
    async fn apply(
        &self,
        frame: Frame,
        state: &mut StreamState,
        events: &mpsc::Sender<StreamEvent>,
    ) -> Result<bool> {
        let data = match frame {
            // Not part of this protocol; completion requires the event.
            Frame::Done => return Ok(false),
            Frame::Data(data) => data,
        };
        let value: Value = serde_json::from_str(&data).map_err(|_| Error::Malformed)?;
        match value.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        if state.text.len() + delta.len() > self.limits.max_output_bytes {
                            return Err(Error::TooLarge);
                        }
                        state.text.push_str(delta);
                        send(
                            events,
                            StreamEvent::TextDelta {
                                text: delta.to_owned(),
                            },
                        )
                        .await?;
                    }
                }
            }
            Some("response.completed") => {
                state.usage = usage(value.pointer("/response/usage"));
                state.finish = Some(FinishReason::Stop);
                return Ok(true);
            }
            Some("response.incomplete") => {
                state.usage = usage(value.pointer("/response/usage"));
                state.finish = Some(
                    match value
                        .pointer("/response/incomplete_details/reason")
                        .and_then(Value::as_str)
                    {
                        Some("max_output_tokens") => FinishReason::Length,
                        Some("content_filter") => FinishReason::ContentFilter,
                        _ => FinishReason::Unknown,
                    },
                );
                return Ok(true);
            }
            Some("response.failed") => {
                return Err(code_error(
                    value
                        .pointer("/response/error/code")
                        .and_then(Value::as_str),
                ))
            }
            Some("error") => {
                return Err(code_error(
                    value
                        .get("code")
                        .or_else(|| value.pointer("/error/code"))
                        .and_then(Value::as_str),
                ))
            }
            // Item and part lifecycle events, reasoning summaries, and event
            // types added later carry nothing shown here.
            _ => {}
        }
        Ok(false)
    }

    async fn finish(
        &self,
        state: StreamState,
        events: &mpsc::Sender<StreamEvent>,
    ) -> Result<Completion> {
        if state.text.trim().is_empty() {
            return Err(Error::Empty);
        }
        let finish = state.finish.unwrap_or(FinishReason::Unknown);
        if let Some(usage) = state.usage {
            send(events, StreamEvent::Usage { usage }).await?;
        }
        send(
            events,
            StreamEvent::Finished {
                reason: finish.clone(),
            },
        )
        .await?;
        Ok(Completion {
            text: state.text,
            usage: state.usage,
            finish,
        })
    }
}

/// The request body for the plan route.
fn body(request: &ChatRequest) -> Result<Value> {
    if request.model.trim().is_empty() {
        return Err(Error::Invalid(
            "A request needs a model and at least one message".into(),
        ));
    }
    let instructions: Vec<&str> = request
        .messages
        .iter()
        .filter(|m| m.role == Role::System && !m.content.trim().is_empty())
        .map(|m| m.content.as_str())
        .collect();
    let input: Vec<Value> = request
        .messages
        .iter()
        .filter(|m| m.role != Role::System && !m.content.trim().is_empty())
        .map(|m| json!({"role": m.role.as_str(), "content": m.content}))
        .collect();
    if !request
        .messages
        .iter()
        .rev()
        .find(|m| m.role != Role::System && !m.content.trim().is_empty())
        .is_some_and(|m| m.role == Role::User)
    {
        return Err(Error::Invalid(
            "This connection needs the conversation to end with your message".into(),
        ));
    }
    let mut body = json!({
        "model": request.model,
        "input": input,
        "store": false,
        "stream": true,
    });
    if !instructions.is_empty() {
        body["instructions"] = json!(instructions.join("\n\n"));
    }
    Ok(body)
}

fn usage(value: Option<&Value>) -> Option<Usage> {
    let value = value?;
    let input = value.get("input_tokens").and_then(Value::as_u64);
    let output = value.get("output_tokens").and_then(Value::as_u64);
    (input.is_some() || output.is_some()).then_some(Usage {
        input_tokens: input,
        output_tokens: output,
    })
}

/// Maps a plan error code to the errors that need their own handling.
fn code_error(code: Option<&str>) -> Error {
    match code {
        Some("subscription_sharing_usage_limit_exceeded") => Error::UsageLimit,
        Some("subscription_sharing_user_not_eligible") => Error::NotEligible,
        _ => Error::Provider,
    }
}

/// A non-success status. Only a stable `error.code` is read from the body.
async fn status_error(response: &mut reqwest::Response) -> Error {
    let status = http_error(response);
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        if body.len() + chunk.len() > ERROR_BODY_BYTES {
            break;
        }
        body.extend_from_slice(&chunk);
    }
    let code = serde_json::from_slice::<Value>(&body).ok().and_then(|v| {
        v.pointer("/error/code")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    match code_error(code.as_deref()) {
        Error::Provider => status,
        specific => specific,
    }
}

#[derive(Default)]
struct StreamState {
    text: String,
    usage: Option<Usage>,
    finish: Option<FinishReason>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    #[test]
    fn instructions_are_lifted_and_plan_rejected_fields_are_never_sent() {
        let body = body(&ChatRequest {
            model: "gpt-x".into(),
            messages: vec![
                Message::system("Rules."),
                Message::user("a"),
                Message::assistant("b"),
                Message::system("Sources."),
                Message::user("c"),
            ],
            max_output_tokens: 1024,
            temperature: Some(0.2),
        })
        .unwrap();
        assert_eq!(body["instructions"], "Rules.\n\nSources.");
        assert_eq!(
            body["input"],
            json!([
                {"role": "user", "content": "a"},
                {"role": "assistant", "content": "b"},
                {"role": "user", "content": "c"},
            ])
        );
        assert_eq!(
            (body["store"].clone(), body["stream"].clone()),
            (json!(false), json!(true))
        );
        for field in [
            "temperature",
            "max_output_tokens",
            "top_p",
            "previous_response_id",
        ] {
            assert!(body.get(field).is_none(), "{field}");
        }
    }

    #[test]
    fn a_conversation_must_end_with_the_user() {
        let request = |messages| ChatRequest {
            model: "m".into(),
            messages,
            max_output_tokens: 1,
            temperature: None,
        };
        assert!(body(&request(vec![Message::user("a"), Message::assistant("b")])).is_err());
        assert!(body(&request(vec![Message::system("only")])).is_err());
    }

    #[test]
    fn plan_codes_map_to_their_own_errors() {
        assert_eq!(
            code_error(Some("subscription_sharing_usage_limit_exceeded")),
            Error::UsageLimit
        );
        assert_eq!(
            code_error(Some("subscription_sharing_user_not_eligible")),
            Error::NotEligible
        );
        assert_eq!(code_error(Some("server_error")), Error::Provider);
        assert_eq!(code_error(None), Error::Provider);
    }
}
