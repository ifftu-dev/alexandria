//! The interview signal feed on a call's encrypted room channel.
//!
//! In an interview held in Alexandria, the panel marks when it asks a
//! question, and the candidate's app (running the integrity check with voice
//! checks allowed) answers with the timing of the candidate's reply and the
//! cues just before it, or reports a second voice. Timings and booleans
//! only: there is no field for audio, words or anything the candidate said.
//! The feed is the candidate's app's own report, shown to the panel as a
//! reason to ask, never as a verdict.

use serde::{Deserialize, Serialize};

/// Prefix that keeps signals apart from chat and captions on the channel.
pub(super) const PREFIX: &[u8] = b"ALXIS1";
const MAX_QUESTION_ID: usize = 128;
/// Ten minutes: no answer timing above it is meaningful.
const MAX_LATENCY_MS: u32 = 600_000;

/// One signal. Externally tagged so the wire (postcard) and the webview
/// (JSON) share one shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum InterviewSignal {
    /// The panel is asking a question; the id is opaque to the candidate.
    Question { question_id: String },
    /// The candidate's reply to a marked question, as their app timed it.
    Answer {
        question_id: String,
        /// From the end of the question to the candidate's first speech.
        latency_ms: u32,
        /// Median latency of the candidate's own first answers.
        baseline_ms: Option<u32>,
        gaze_down_before: bool,
        second_voice_before: bool,
    },
    /// A voice pitched well apart from the candidate's started speaking.
    SecondVoice,
}

impl InterviewSignal {
    pub fn validate(&self) -> Result<(), String> {
        let id = match self {
            Self::Question { question_id } => question_id,
            Self::Answer {
                question_id,
                latency_ms,
                baseline_ms,
                ..
            } => {
                if *latency_ms > MAX_LATENCY_MS || baseline_ms.is_some_and(|b| b > MAX_LATENCY_MS) {
                    return Err("answer timing out of range".into());
                }
                question_id
            }
            Self::SecondVoice => return Ok(()),
        };
        let valid = !id.is_empty()
            && id.len() <= MAX_QUESTION_ID
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if valid {
            Ok(())
        } else {
            Err("a question id is a short token".into())
        }
    }
}

/// A signal as it travels: who sent it and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalMessage {
    pub sender: String,
    pub sender_name: Option<String>,
    pub signal: InterviewSignal,
    pub timestamp: u64,
}

/// Prefix and encode a valid message for the channel.
pub(super) fn encode(message: &SignalMessage) -> Result<Vec<u8>, String> {
    message.signal.validate()?;
    let payload = postcard::to_stdvec(message).map_err(|e| e.to_string())?;
    let mut encoded = Vec::with_capacity(PREFIX.len() + payload.len());
    encoded.extend_from_slice(PREFIX);
    encoded.extend_from_slice(&payload);
    Ok(encoded)
}

/// A valid message from channel bytes; `None` when the bytes are something
/// else on the channel, `Err` when they claim to be a signal and are not.
pub(super) fn decode(bytes: &[u8]) -> Option<Result<SignalMessage, String>> {
    let payload = bytes.strip_prefix(PREFIX)?;
    Some(
        postcard::from_bytes::<SignalMessage>(payload)
            .map_err(|e| e.to_string())
            .and_then(|m| m.signal.validate().map(|()| m)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(signal: InterviewSignal) -> SignalMessage {
        SignalMessage {
            sender: "node".into(),
            sender_name: Some("Candidate".into()),
            signal,
            timestamp: 1_700_000_000_000,
        }
    }

    fn answer(question_id: &str, latency_ms: u32) -> InterviewSignal {
        InterviewSignal::Answer {
            question_id: question_id.into(),
            latency_ms,
            baseline_ms: Some(700),
            gaze_down_before: true,
            second_voice_before: false,
        }
    }

    #[test]
    fn each_signal_survives_the_channel() {
        for signal in [
            InterviewSignal::Question {
                question_id: "q-1".into(),
            },
            answer("q-1", 4_200),
            InterviewSignal::SecondVoice,
        ] {
            let sent = message(signal);
            assert_eq!(decode(&encode(&sent).unwrap()), Some(Ok(sent)));
        }
    }

    #[test]
    fn chat_and_captions_on_the_channel_are_not_signals() {
        assert_eq!(decode(b"ALXTR1whatever"), None);
        assert_eq!(decode(b"hello"), None);
    }

    #[test]
    fn a_signal_no_honest_app_sends_is_refused_both_ways() {
        for bad in [
            answer("", 100),
            answer("has spaces", 100),
            answer(&"x".repeat(129), 100),
            answer("q", 600_001),
        ] {
            let sent = message(bad);
            assert!(encode(&sent).is_err());
            let mut forged = PREFIX.to_vec();
            forged.extend(postcard::to_stdvec(&sent).unwrap());
            assert!(matches!(decode(&forged), Some(Err(_))));
        }
    }

    #[test]
    fn the_webview_shape_is_serde_external_tagging() {
        assert_eq!(
            serde_json::to_value(InterviewSignal::SecondVoice).unwrap(),
            serde_json::json!("second_voice")
        );
        let question: InterviewSignal =
            serde_json::from_value(serde_json::json!({ "question": { "question_id": "q" } }))
                .unwrap();
        assert_eq!(
            question,
            InterviewSignal::Question {
                question_id: "q".into()
            }
        );
    }
}
