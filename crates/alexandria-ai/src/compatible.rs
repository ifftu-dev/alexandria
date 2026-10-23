//! The OpenAI-compatible chat-completions protocol.
//!
//! Most local runtimes (Ollama, LM Studio, vLLM, llama.cpp) and many hosted
//! APIs speak this protocol. Only the fields every implementation agrees on
//! are sent: `model`, `messages`, `stream`, `max_tokens`, and optionally
//! `temperature`. Usage and finish reasons are read when present and left
//! empty when not; nothing is invented.

use std::fmt;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::endpoint::Endpoint;
use crate::model::{ChatRequest, Completion, FinishReason, Limits, StreamEvent, Usage};
use crate::sse::{Decoder, Frame};
use crate::{Error, Result};

/// A connection to one compatible endpoint. The key is never printed.
pub struct Compatible {
    endpoint: Endpoint,
    key: Option<String>,
    limits: Limits,
}

impl fmt::Debug for Compatible {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Compatible")
            .field("endpoint", &self.endpoint.base().as_str())
            .field("location", &self.endpoint.location())
            .field("has_key", &self.key.is_some())
            .finish()
    }
}

impl Compatible {
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
        let bytes = self.read_bounded(&mut response).await?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)?;
        self.completion_from_json(&value)
    }

    /// Streams an answer. Each event is sent to `events` as it is decoded;
    /// the channel's capacity bounds how far decoding runs ahead of the
    /// consumer. Dropping the receiver stops the request.
    ///
    /// The returned [`Completion`] repeats what the events carried, so a
    /// caller that only needs the final text can ignore the channel.
    pub async fn stream(
        &self,
        request: &ChatRequest,
        events: mpsc::Sender<StreamEvent>,
    ) -> Result<Completion> {
        let mut response = self.post(request, true).await?;
        let is_json = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"));
        if is_json {
            // The server ignored `stream: true`. Accept the whole answer.
            let bytes = self.read_bounded(&mut response).await?;
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
            let chunk = tokio::time::timeout(self.limits.idle_timeout, response.chunk())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|e| {
                    if e.is_timeout() {
                        Error::Timeout
                    } else {
                        Error::Interrupted
                    }
                })?;
            let (frames, body_ended) = match chunk {
                Some(bytes) => (decoder.push(&bytes)?, false),
                None => (decoder.finish()?, true),
            };
            for frame in frames {
                if self.apply(frame, &mut state, &events).await? {
                    // `[DONE]`: stop reading even if the server keeps the
                    // connection open.
                    state.done = true;
                    return self.finish(state, &events).await;
                }
            }
            if body_ended {
                return self.finish(state, &events).await;
            }
        }
    }

    /// Lists model identifiers when the endpoint offers discovery.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let client = self
            .endpoint
            .client(Some(std::time::Duration::from_secs(20)))?;
        let mut request = client.get(self.endpoint.join("models")?);
        if let Some(key) = &self.key {
            request = request.bearer_auth(key);
        }
        let mut response = request.send().await.map_err(send_error)?;
        match response.status().as_u16() {
            404 | 405 | 501 => return Err(Error::Unsupported("model discovery")),
            s if !(200..300).contains(&s) => return Err(http_error(&response)),
            _ => {}
        }
        let bytes = self.read_bounded(&mut response).await?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| Error::Malformed)?;
        let list = value
            .get("data")
            .or_else(|| value.get("models"))
            .and_then(Value::as_array)
            .ok_or(Error::Malformed)?;
        let mut ids: Vec<String> = list
            .iter()
            .filter_map(|m| {
                m.get("id")
                    .or_else(|| m.get("name"))
                    .and_then(Value::as_str)
            })
            .filter(|id| !id.is_empty() && id.len() <= 200)
            .take(500)
            .map(str::to_owned)
            .collect();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    async fn post(&self, request: &ChatRequest, stream: bool) -> Result<reqwest::Response> {
        if request.model.trim().is_empty() || request.messages.is_empty() {
            return Err(Error::Invalid(
                "A request needs a model and at least one message".into(),
            ));
        }
        let mut body = json!({
            "model": request.model,
            "messages": request
                .messages
                .iter()
                .map(|m| json!({"role": m.role.as_str(), "content": m.content}))
                .collect::<Vec<_>>(),
            "stream": stream,
            "max_tokens": request.max_output_tokens,
        });
        if let Some(t) = request.temperature {
            body["temperature"] = json!(t);
        }
        let bytes = serde_json::to_vec(&body).map_err(|_| Error::Malformed)?;
        if bytes.len() > self.limits.max_request_bytes {
            return Err(Error::Invalid("This request is too large to send".into()));
        }
        let client = self.endpoint.client(Some(self.limits.total_timeout))?;
        let mut builder = client
            .post(self.endpoint.join("chat/completions")?)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(bytes);
        if stream {
            builder = builder.header(reqwest::header::ACCEPT, "text/event-stream");
        }
        if let Some(key) = &self.key {
            builder = builder.bearer_auth(key);
        }
        let response = builder.send().await.map_err(send_error)?;
        if !response.status().is_success() {
            return Err(http_error(&response));
        }
        Ok(response)
    }

    async fn read_bounded(&self, response: &mut reqwest::Response) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        loop {
            let chunk = tokio::time::timeout(self.limits.idle_timeout, response.chunk())
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|e| {
                    if e.is_timeout() {
                        Error::Timeout
                    } else {
                        Error::Interrupted
                    }
                })?;
            let Some(chunk) = chunk else { break };
            if bytes.len() + chunk.len() > self.limits.max_response_bytes {
                return Err(Error::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    fn completion_from_json(&self, value: &Value) -> Result<Completion> {
        if value.get("error").is_some() {
            return Err(Error::Provider);
        }
        let choice = value.pointer("/choices/0").ok_or(Error::Malformed)?;
        let text = choice
            .pointer("/message/content")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or(Error::Empty)?;
        if text.len() > self.limits.max_output_bytes {
            return Err(Error::TooLarge);
        }
        Ok(Completion {
            text: text.to_owned(),
            usage: usage_from(value),
            finish: FinishReason::from_wire(choice.get("finish_reason").and_then(Value::as_str)),
        })
    }

    /// Applies one frame. Returns true once the provider says it is done.
    async fn apply(
        &self,
        frame: Frame,
        state: &mut StreamState,
        events: &mpsc::Sender<StreamEvent>,
    ) -> Result<bool> {
        let data = match frame {
            Frame::Done => return Ok(true),
            Frame::Data(data) => data,
        };
        let value: Value = serde_json::from_str(&data).map_err(|_| Error::Malformed)?;
        if value.get("error").is_some() {
            return Err(Error::Provider);
        }
        if let Some(usage) = usage_from(&value) {
            state.usage = Some(usage);
        }
        if let Some(choice) = value.pointer("/choices/0") {
            if let Some(delta) = choice.pointer("/delta/content").and_then(Value::as_str) {
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
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                state.finish = Some(FinishReason::from_wire(Some(reason)));
            }
        }
        Ok(false)
    }

    async fn finish(
        &self,
        state: StreamState,
        events: &mpsc::Sender<StreamEvent>,
    ) -> Result<Completion> {
        let Some(finish) = state
            .finish
            .clone()
            .or(state.done.then_some(FinishReason::Unknown))
        else {
            // The connection closed without `[DONE]` or a finish reason. What
            // arrived may be a truncated answer; say so instead of passing it
            // off as complete.
            return Err(Error::Interrupted);
        };
        if state.text.trim().is_empty() {
            return Err(Error::Empty);
        }
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

#[derive(Default)]
struct StreamState {
    text: String,
    usage: Option<Usage>,
    finish: Option<FinishReason>,
    done: bool,
}

async fn send(events: &mpsc::Sender<StreamEvent>, event: StreamEvent) -> Result<()> {
    events.send(event).await.map_err(|_| Error::Cancelled)
}

fn usage_from(value: &Value) -> Option<Usage> {
    let usage = value.get("usage").filter(|u| u.is_object())?;
    let input = usage.get("prompt_tokens").and_then(Value::as_u64);
    let output = usage.get("completion_tokens").and_then(Value::as_u64);
    (input.is_some() || output.is_some()).then_some(Usage {
        input_tokens: input,
        output_tokens: output,
    })
}

fn send_error(e: reqwest::Error) -> Error {
    if e.is_timeout() {
        Error::Timeout
    } else if e.is_connect() {
        Error::Unreachable
    } else {
        Error::Interrupted
    }
}

fn http_error(response: &reqwest::Response) -> Error {
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|s| s.min(3600));
    Error::Http {
        status: response.status().as_u16(),
        retry_after,
    }
}
