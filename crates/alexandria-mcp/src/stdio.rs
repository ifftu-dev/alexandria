//! Newline-delimited JSON-RPC framing for the stdio transport.
//!
//! rmcp's codec ends the input stream on the first unparseable line. This
//! framing answers invalid lines with JSON-RPC errors and keeps serving, while
//! an oversized frame still stops the process.

use rmcp::model::{ClientJsonRpcMessage, ErrorData, RequestId, ServerJsonRpcMessage};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::{bytes::BytesMut, codec::Decoder};

pub const MAX_FRAME_BYTES: usize = 262_144;

/// Either error ends the input stream.
#[derive(Debug)]
pub enum FrameError {
    TooLong,
    Io,
}

impl From<std::io::Error> for FrameError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}

/// Splits input into lines of at most `max` bytes, excluding the newline.
pub struct LineCodec {
    max: usize,
    searched: usize,
}

impl LineCodec {
    pub fn new(max: usize) -> Self {
        Self { max, searched: 0 }
    }
}

impl Decoder for LineCodec {
    type Item = Vec<u8>;
    type Error = FrameError;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Vec<u8>>, FrameError> {
        let limit = buf.len().min(self.max + 1);
        match buf[self.searched..limit].iter().position(|b| *b == b'\n') {
            Some(offset) => {
                let end = self.searched + offset;
                self.searched = 0;
                let line = buf.split_to(end + 1);
                Ok(Some(line[..end].to_vec()))
            }
            None if buf.len() > self.max => Err(FrameError::TooLong),
            None => {
                self.searched = limit;
                Ok(None)
            }
        }
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Vec<u8>>, FrameError> {
        match self.decode(buf)? {
            Some(line) => Ok(Some(line)),
            None if buf.is_empty() => Ok(None),
            None => {
                self.searched = 0;
                Ok(Some(buf.split_to(buf.len()).to_vec()))
            }
        }
    }
}

/// Converts input lines into SDK messages, answering invalid lines itself.
pub struct Framing {
    output: UnboundedSender<Vec<u8>>,
    started: bool,
}

impl Framing {
    pub fn new(output: UnboundedSender<Vec<u8>>) -> Self {
        Self {
            output,
            started: false,
        }
    }

    /// Unparseable lines receive -32700 and invalid requests -32600; invalid
    /// notifications are dropped because they never get responses.
    pub fn parse(&mut self, line: &[u8]) -> Option<ClientJsonRpcMessage> {
        let line = line.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.iter().all(u8::is_ascii_whitespace) {
            return None;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            self.reject(ErrorData::parse_error("Parse error", None), None);
            return None;
        };
        let id = value.get("id");
        if id.is_some_and(|id| !(id.is_string() || id.is_i64() || id.is_u64())) {
            // Request IDs are strings or integers, never null; rmcp would
            // otherwise accept such a request as a notification.
            self.reject(ErrorData::invalid_request("Invalid Request", None), None);
            return None;
        }
        let is_notification = value.get("method").is_some() && id.is_none();
        match serde_json::from_value::<ClientJsonRpcMessage>(value.clone()) {
            // rmcp ends the session unless the first message is a request.
            // Nothing sent before any request can refer to one, so drop it.
            Ok(message)
                if !self.started && !matches!(message, ClientJsonRpcMessage::Request(_)) =>
            {
                None
            }
            Ok(message) => {
                self.started = true;
                Some(message)
            }
            Err(_) if is_notification => None,
            Err(_) => {
                let id = id.and_then(|id| serde_json::from_value::<RequestId>(id.clone()).ok());
                self.reject(ErrorData::invalid_request("Invalid Request", None), id);
                None
            }
        }
    }

    fn reject(&self, error: ErrorData, id: Option<RequestId>) {
        if let Ok(bytes) = frame(&ServerJsonRpcMessage::error(error, id)) {
            let _ = self.output.send(bytes);
        }
    }
}

pub fn frame(message: &ServerJsonRpcMessage) -> std::io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(message).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    Ok(bytes)
}
