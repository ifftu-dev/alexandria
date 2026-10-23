//! The Anthropic Messages protocol.
//!
//! The base URL is the API root including its version path, for example
//! `https://api.anthropic.com/v1`; requests go to `messages` and `models`
//! under it. Requests use API version `2026-10-21`, which rejects sampling
//! parameters, so [`ChatRequest::temperature`] is never sent.
//!
//! Translation from the canonical shape:
//!
//! * every system message is joined into the top-level `system` field;
//! * consecutive turns with the same role are merged, and empty turns are
//!   dropped, because the protocol expects alternating non-empty turns;
//! * the conversation must end with a user turn: current models refuse an
//!   assistant prefill, so one is rejected here before anything is sent.
//!
//! History is replayed as text only. No thinking blocks are stored or sent,
//! which the protocol accepts; the model simply does not see its earlier
//! reasoning. Current models think by default and thinking counts against
//! `max_tokens`, so [`THINKING_HEADROOM`] is added to the requested output.
//! The visible answer stays bounded by [`Limits::max_output_bytes`].

use std::fmt;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::endpoint::Endpoint;
use crate::http::{http_error, is_json, model_ids, next_chunk, read_bounded, send, send_error};
use crate::model::{ChatRequest, Completion, FinishReason, Limits, Role, StreamEvent, Usage};
use crate::sse::{Decoder, Frame};
use crate::{Error, Result};

/// The API version every request declares.
pub const API_VERSION: &str = "2026-10-21";

/// Tokens added to `max_tokens` for the model's own reasoning.
pub const THINKING_HEADROOM: u32 = 8_192;

/// A connection to one Messages endpoint. The key is never printed.
pub struct Anthropic {
    endpoint: Endpoint,
    key: Option<String>,
    limits: Limits,
}

impl fmt::Debug for Anthropic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Anthropic")
            .field("endpoint", &self.endpoint.base().as_str())
            .field("location", &self.endpoint.location())
            .field("has_key", &self.key.is_some())
            .finish()
    }
}

impl Anthropic {
    pub fn new(endpoint: Endpoint, key: Option<String>) -> Self {
        Self {
            endpoint,
            key: key.filter(|k| !k.is_empty()),
            limits: Limits::default(),
        }
    }

    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// One request, one complete answer.
    pub async fn complete(&self, request: &ChatRequest) -> Result<Completion> {
        let mut response = self.post(request, false).await?;
        let bytes = read_bounded(&mut response, &self.limits).await?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)?;
        self.completion_from_json(&value)
    }

    /// Streams an answer, with the same contract as
    /// [`crate::compatible::Compatible::stream`].
    pub async fn stream(
        &self,
        request: &ChatRequest,
        events: mpsc::Sender<StreamEvent>,
    ) -> Result<Completion> {
        let mut response = self.post(request, true).await?;
        if is_json(&response) {
            // A gateway answered without streaming. Accept the whole answer.
            let bytes = read_bounded(&mut response, &self.limits).await?;
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)?;
            let completion = self.completion_from_json(&value)?;
            send(
                &events,
                StreamEvent::TextDelta {
                    text: completion.text.clone(),
                },
            )
            .await?;
            if let Some(usage) = completion.usage {
                send(&events, StreamEvent::Usage { usage }).await?;
            }
            send(
                &events,
                StreamEvent::Finished {
                    reason: completion.finish.clone(),
                },
            )
            .await?;
            return Ok(completion);
        }

        let mut decoder = Decoder::new(self.limits.max_line_bytes);
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
                // No `message_stop`: what arrived may be truncated.
                return Err(Error::Interrupted);
            }
        }
    }

    /// Lists model identifiers.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let client = self
            .endpoint
            .client(Some(std::time::Duration::from_secs(20)))?;
        let mut url = self.endpoint.join("models")?;
        url.query_pairs_mut().append_pair("limit", "1000");
        let mut response = self
            .headers(client.get(url))
            .send()
            .await
            .map_err(send_error)?;
        match response.status().as_u16() {
            404 | 405 | 501 => return Err(Error::Unsupported("model discovery")),
            s if !(200..300).contains(&s) => return Err(http_error(&response)),
            _ => {}
        }
        let bytes = read_bounded(&mut response, &self.limits).await?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)?;
        model_ids(&value)
    }

    fn headers(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let builder = builder.header("anthropic-version", API_VERSION);
        match &self.key {
            Some(key) => builder.header("x-api-key", key),
            None => builder,
        }
    }

    async fn post(&self, request: &ChatRequest, stream: bool) -> Result<reqwest::Response> {
        let body = body(request, stream)?;
        let bytes = serde_json::to_vec(&body).map_err(|_| Error::Malformed)?;
        if bytes.len() > self.limits.max_request_bytes {
            return Err(Error::Invalid("This request is too large to send".into()));
        }
        let client = self.endpoint.client(Some(self.limits.total_timeout))?;
        let mut builder = client
            .post(self.endpoint.join("messages")?)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(bytes);
        if stream {
            builder = builder.header(reqwest::header::ACCEPT, "text/event-stream");
        }
        let response = self.headers(builder).send().await.map_err(send_error)?;
        if !response.status().is_success() {
            return Err(http_error(&response));
        }
        Ok(response)
    }

    fn completion_from_json(&self, value: &Value) -> Result<Completion> {
        if value.get("type").and_then(Value::as_str) == Some("error") {
            return Err(Error::Provider);
        }
        let blocks = value
            .get("content")
            .and_then(Value::as_array)
            .ok_or(Error::Malformed)?;
        let text: String = blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect();
        if text.len() > self.limits.max_output_bytes {
            return Err(Error::TooLarge);
        }
        if text.trim().is_empty() {
            return Err(Error::Empty);
        }
        let mut usage = UsageParts::default();
        if let Some(u) = value.get("usage") {
            usage.read(u);
        }
        Ok(Completion {
            text,
            usage: usage.total(),
            finish: finish_reason(value.get("stop_reason").and_then(Value::as_str)),
        })
    }

    /// Applies one frame. Returns true at `message_stop`.
    async fn apply(
        &self,
        frame: Frame,
        state: &mut StreamState,
        events: &mpsc::Sender<StreamEvent>,
    ) -> Result<bool> {
        let data = match frame {
            // Not part of this protocol; a gateway may add it.
            Frame::Done => return Ok(true),
            Frame::Data(data) => data,
        };
        let value: Value = serde_json::from_str(&data).map_err(|_| Error::Malformed)?;
        match value.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                if let Some(u) = value.pointer("/message/usage") {
                    state.usage.read(u);
                }
            }
            Some("content_block_delta") => {
                // Only text is shown. Thinking and signature deltas, and any
                // delta type added later, are skipped.
                if value.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") {
                    if let Some(delta) = value.pointer("/delta/text").and_then(Value::as_str) {
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
            }
            Some("message_delta") => {
                if let Some(reason) = value.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    state.finish = Some(finish_reason(Some(reason)));
                }
                if let Some(u) = value.get("usage") {
                    state.usage.read(u);
                }
            }
            Some("message_stop") => return Ok(true),
            Some("error") => return Err(Error::Provider),
            // `ping`, block start/stop, and event types added later.
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
        let usage = state.usage.total();
        if let Some(usage) = usage {
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
            usage,
            finish,
        })
    }
}

/// The request body. Fails before sending when the conversation cannot be
/// expressed in this protocol.
fn body(request: &ChatRequest, stream: bool) -> Result<Value> {
    if request.model.trim().is_empty() {
        return Err(Error::Invalid(
            "A request needs a model and at least one message".into(),
        ));
    }
    let system: Vec<&str> = request
        .messages
        .iter()
        .filter(|m| m.role == Role::System && !m.content.trim().is_empty())
        .map(|m| m.content.as_str())
        .collect();
    let mut turns: Vec<(Role, String)> = Vec::new();
    for message in request
        .messages
        .iter()
        .filter(|m| m.role != Role::System && !m.content.trim().is_empty())
    {
        match turns.last_mut() {
            Some((role, content)) if *role == message.role => {
                content.push_str("\n\n");
                content.push_str(&message.content);
            }
            _ => turns.push((message.role, message.content.clone())),
        }
    }
    // The protocol starts with a user turn. Chat keeps whole turns, so this
    // only trims an answer whose question fell out of the history budget.
    while turns
        .first()
        .is_some_and(|(role, _)| *role == Role::Assistant)
    {
        turns.remove(0);
    }
    match turns.last() {
        Some((Role::User, _)) => {}
        Some(_) => {
            return Err(Error::Invalid(
                "This connection needs the conversation to end with your message".into(),
            ))
        }
        None => {
            return Err(Error::Invalid(
                "A request needs a model and at least one message".into(),
            ))
        }
    }
    let mut body = json!({
        "model": request.model,
        "max_tokens": request.max_output_tokens.saturating_add(THINKING_HEADROOM),
        "messages": turns
            .iter()
            .map(|(role, content)| json!({"role": role.as_str(), "content": content}))
            .collect::<Vec<_>>(),
        "stream": stream,
    });
    if !system.is_empty() {
        body["system"] = json!(system.join("\n\n"));
    }
    Ok(body)
}

fn finish_reason(value: Option<&str>) -> FinishReason {
    match value {
        Some("end_turn") | Some("stop_sequence") => FinishReason::Stop,
        Some("max_tokens") | Some("model_context_window_exceeded") => FinishReason::Length,
        Some("refusal") => FinishReason::ContentFilter,
        _ => FinishReason::Unknown,
    }
}

/// Usage as it arrives: input at the start, output growing to the end.
/// Input counts include cached prompt tokens, which are still input.
#[derive(Default)]
struct UsageParts {
    input: Option<u64>,
    cache_write: Option<u64>,
    cache_read: Option<u64>,
    output: Option<u64>,
}

impl UsageParts {
    fn read(&mut self, usage: &Value) {
        let field = |name: &str| usage.get(name).and_then(Value::as_u64);
        self.input = field("input_tokens").or(self.input);
        self.cache_write = field("cache_creation_input_tokens").or(self.cache_write);
        self.cache_read = field("cache_read_input_tokens").or(self.cache_read);
        self.output = field("output_tokens").or(self.output);
    }

    fn total(&self) -> Option<Usage> {
        let parts = [self.input, self.cache_write, self.cache_read];
        let input = parts
            .iter()
            .any(Option::is_some)
            .then(|| parts.iter().flatten().sum());
        (input.is_some() || self.output.is_some()).then_some(Usage {
            input_tokens: input,
            output_tokens: self.output,
        })
    }
}

#[derive(Default)]
struct StreamState {
    text: String,
    usage: UsageParts,
    finish: Option<FinishReason>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    fn request(messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            messages,
            max_output_tokens: 1024,
            temperature: Some(0.2),
        }
    }

    #[test]
    fn system_is_lifted_turns_merge_and_sampling_is_never_sent() {
        let body = body(
            &request(vec![
                Message::system("Rules."),
                Message::user("a"),
                Message::user("b"),
                Message::assistant(""),
                Message::system("Sources."),
                Message::assistant("c"),
                Message::user("d"),
            ]),
            true,
        )
        .unwrap();
        assert_eq!(body["system"], "Rules.\n\nSources.");
        assert_eq!(
            body["messages"],
            json!([
                {"role": "user", "content": "a\n\nb"},
                {"role": "assistant", "content": "c"},
                {"role": "user", "content": "d"},
            ])
        );
        assert!(body.get("temperature").is_none());
        assert_eq!(body["max_tokens"], 1024 + THINKING_HEADROOM);
    }

    #[test]
    fn a_trailing_assistant_turn_is_refused_before_sending() {
        let err = body(
            &request(vec![Message::user("a"), Message::assistant("b")]),
            false,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Invalid(_)));
        assert!(body(&request(vec![Message::system("only")]), false).is_err());
        let trimmed = body(
            &request(vec![Message::assistant("orphan"), Message::user("q")]),
            false,
        )
        .unwrap();
        assert_eq!(
            trimmed["messages"],
            json!([{"role": "user", "content": "q"}])
        );
    }

    #[test]
    fn usage_adds_cached_input_and_keeps_the_latest_output() {
        let mut usage = UsageParts::default();
        usage.read(&json!({"input_tokens": 10, "cache_read_input_tokens": 90, "output_tokens": 1}));
        usage.read(&json!({"output_tokens": 42}));
        assert_eq!(
            usage.total(),
            Some(Usage {
                input_tokens: Some(100),
                output_tokens: Some(42)
            })
        );
        assert_eq!(UsageParts::default().total(), None);
    }
}
