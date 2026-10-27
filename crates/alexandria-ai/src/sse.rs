//! Bounded server-sent-events decoding.
//!
//! Network chunks arrive at arbitrary byte boundaries, including in the
//! middle of a multi-byte UTF-8 character. Bytes are buffered until a full
//! line is available, and only complete lines are decoded, so characters are
//! never split. A line longer than the limit is an error rather than an
//! unbounded allocation.

use crate::{Error, Result};

/// One `data:` payload. Multi-line data fields are joined with `\n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Frame {
    Data(String),
    Done,
}

pub(crate) struct Decoder {
    buffer: Vec<u8>,
    data: Option<String>,
    max_line: usize,
}

impl Decoder {
    pub(crate) fn new(max_line: usize) -> Self {
        Self {
            buffer: Vec::new(),
            data: None,
            max_line,
        }
    }

    /// Feeds bytes, returning every frame they complete.
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<Frame>> {
        self.buffer.extend_from_slice(chunk);
        let mut frames = Vec::new();
        let mut start = 0;
        while let Some(offset) = self.buffer[start..].iter().position(|b| *b == b'\n') {
            let end = start + offset;
            let line = &self.buffer[start..end];
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.len() > self.max_line {
                return Err(Error::TooLarge);
            }
            let line = std::str::from_utf8(line)
                .map_err(|_| Error::Malformed)?
                .to_owned();
            if let Some(frame) = self.line(&line)? {
                frames.push(frame);
            }
            start = end + 1;
        }
        self.buffer.drain(..start);
        if self.buffer.len() > self.max_line {
            return Err(Error::TooLarge);
        }
        Ok(frames)
    }

    /// Flushes a final event that was not followed by a blank line.
    pub(crate) fn finish(&mut self) -> Result<Vec<Frame>> {
        let mut frames = Vec::new();
        if !self.buffer.is_empty() {
            let rest = std::mem::take(&mut self.buffer);
            let line = std::str::from_utf8(&rest).map_err(|_| Error::Malformed)?;
            if let Some(frame) = self.line(line.trim_end_matches('\r'))? {
                frames.push(frame);
            }
        }
        if let Some(frame) = self.line("")? {
            frames.push(frame);
        }
        Ok(frames)
    }

    fn line(&mut self, line: &str) -> Result<Option<Frame>> {
        if line.is_empty() {
            return Ok(self.data.take().map(|data| {
                if data == "[DONE]" {
                    Frame::Done
                } else {
                    Frame::Data(data)
                }
            }));
        }
        if line.starts_with(':') {
            return Ok(None);
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        if field == "data" {
            match &mut self.data {
                Some(existing) => {
                    if existing.len() + 1 + value.len() > self.max_line {
                        return Err(Error::TooLarge);
                    }
                    existing.push('\n');
                    existing.push_str(value);
                }
                None => self.data = Some(value.to_string()),
            }
        }
        // `event`, `id`, and `retry` carry nothing the compatible protocol uses.
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(decoder: &mut Decoder, chunks: &[&[u8]]) -> Vec<Frame> {
        let mut frames = Vec::new();
        for chunk in chunks {
            frames.extend(decoder.push(chunk).unwrap());
        }
        frames.extend(decoder.finish().unwrap());
        frames
    }

    #[test]
    fn frames_survive_arbitrary_chunk_boundaries() {
        let text = "data: {\"a\":\"é漢\"}\n\ndata: [DONE]\n\n".as_bytes();
        let expected = vec![Frame::Data("{\"a\":\"é漢\"}".into()), Frame::Done];
        for split in 1..text.len() {
            let mut decoder = Decoder::new(1024);
            let (a, b) = text.split_at(split);
            assert_eq!(all(&mut decoder, &[a, b]), expected, "split at {split}");
        }
        let mut decoder = Decoder::new(1024);
        let bytes: Vec<&[u8]> = text.chunks(1).collect();
        assert_eq!(all(&mut decoder, &bytes), expected);
    }

    #[test]
    fn comments_crlf_and_other_fields_are_ignored() {
        let mut decoder = Decoder::new(1024);
        let frames = all(
            &mut decoder,
            &[b": keep-alive\r\nevent: message\r\nid: 4\r\ndata: x\r\n\r\n"],
        );
        assert_eq!(frames, vec![Frame::Data("x".into())]);
    }

    #[test]
    fn multi_line_data_joins_and_trailing_event_flushes() {
        let mut decoder = Decoder::new(1024);
        let frames = all(&mut decoder, &[b"data: one\ndata: two"]);
        assert_eq!(frames, vec![Frame::Data("one\ntwo".into())]);
    }

    #[test]
    fn oversized_lines_are_refused_without_buffering_forever() {
        let mut decoder = Decoder::new(16);
        assert_eq!(decoder.push(&[b'a'; 17]), Err(Error::TooLarge));
        let mut decoder = Decoder::new(16);
        assert_eq!(
            decoder.push(b"data: 0123456789abcdef\n"),
            Err(Error::TooLarge)
        );
    }

    #[test]
    fn invalid_utf8_is_malformed() {
        let mut decoder = Decoder::new(64);
        assert_eq!(decoder.push(b"data: \xff\n"), Err(Error::Malformed));
    }
}
