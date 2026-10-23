//! Transport helpers shared by every adapter: bounded reads, sanitized
//! errors, and event delivery. Nothing here knows a wire protocol.

use tokio::sync::mpsc;

use crate::model::{Limits, StreamEvent};
use crate::{Error, Result};

/// The next body chunk, bounded by the idle timeout. `None` at end of body.
pub(crate) async fn next_chunk(
    response: &mut reqwest::Response,
    limits: &Limits,
) -> Result<Option<bytes::Bytes>> {
    tokio::time::timeout(limits.idle_timeout, response.chunk())
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|e| {
            if e.is_timeout() {
                Error::Timeout
            } else {
                Error::Interrupted
            }
        })
}

/// Reads a whole non-streamed body, refusing one larger than the limit.
pub(crate) async fn read_bounded(
    response: &mut reqwest::Response,
    limits: &Limits,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = next_chunk(response, limits).await? {
        if bytes.len() + chunk.len() > limits.max_response_bytes {
            return Err(Error::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Whether the server answered a streaming request with a plain JSON body.
pub(crate) fn is_json(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
}

pub(crate) async fn send(events: &mpsc::Sender<StreamEvent>, event: StreamEvent) -> Result<()> {
    events.send(event).await.map_err(|_| Error::Cancelled)
}

pub(crate) fn send_error(e: reqwest::Error) -> Error {
    if e.is_timeout() {
        Error::Timeout
    } else if e.is_connect() {
        Error::Unreachable
    } else {
        Error::Interrupted
    }
}

/// A non-success status. The body is never read: it may echo the request.
pub(crate) fn http_error(response: &reqwest::Response) -> Error {
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

/// Model identifiers from a discovery response, bounded and sorted.
pub(crate) fn model_ids(value: &serde_json::Value) -> Result<Vec<String>> {
    use serde_json::Value;
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
