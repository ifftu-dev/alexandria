//! The interview signal feed on a call's encrypted room channel.
//!
//! In an interview held in Alexandria, the panel marks when it asks a
//! question, and the candidate's app (running the integrity check with voice
//! checks allowed) answers with the timing of the candidate's reply and the
//! cues just before it, or reports a second voice. Timings and booleans
//! only: there is no field for audio, words or anything the candidate said.
//! The feed is the candidate's app's own report, shown to the panel as a
//! reason to ask, never as a verdict. The panel also marks an unscripted
//! live segment as done, which the candidate's check records.
//!
//! Each message is signed by the sending node's key over the room's gossip
//! topic, so a relay cannot change who sent it and a signal does not replay
//! in another room. Question markers and live segments are accepted only
//! from the interview's [`Panel`].

use serde::{Deserialize, Serialize};

/// Prefix that keeps signals apart from chat and captions on the channel.
pub(super) const PREFIX: &[u8] = b"ALXIS2";
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
    /// The panel completed an unscripted live segment with the candidate
    /// (panel to candidate). The candidate's app records it in the running
    /// integrity check as the `live` verification rung's evidence.
    LiveSegment { segment_id: String },
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
            Self::LiveSegment { segment_id } => segment_id,
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

/// A signal as the webview sees it: who sent it and when. `sender` is the
/// node id whose key signed it, never a claim the payload makes alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalMessage {
    pub sender: String,
    pub sender_name: Option<String>,
    pub signal: InterviewSignal,
    pub timestamp: u64,
}

/// A message as it travels: signed by the sender's node key over the room's
/// topic, so a gossip relay cannot change who sent it and a signal from one
/// room does not replay in another.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Wire {
    message: SignalMessage,
    signature: Vec<u8>,
}

/// The node ids of the interview's panel, as this device knows them. On the
/// candidate's side these are the bootstrap peers of the room ticket the
/// organisation's Cloud handed over, which a panelist published. Elsewhere
/// it is empty.
pub(super) type Panel = std::collections::HashSet<String>;

fn signing_bytes(topic: &[u8], message: &SignalMessage) -> Vec<u8> {
    let mut bytes = b"alexandria-interview-signal/2\n".to_vec();
    bytes.extend_from_slice(&(topic.len() as u64).to_be_bytes());
    bytes.extend_from_slice(topic);
    // Postcard of a struct of owned fields cannot fail.
    bytes.extend(postcard::to_stdvec(message).unwrap_or_default());
    bytes
}

/// Sign, prefix and encode a valid signal from `key`'s node for the room
/// with gossip topic `topic`.
pub(super) fn encode(
    signal: InterviewSignal,
    sender_name: Option<String>,
    timestamp: u64,
    key: &iroh::SecretKey,
    topic: &[u8],
) -> Result<Vec<u8>, String> {
    signal.validate()?;
    let message = SignalMessage {
        sender: key.public().to_string(),
        sender_name,
        signal,
        timestamp,
    };
    let signature = key
        .sign(&signing_bytes(topic, &message))
        .to_bytes()
        .to_vec();
    let payload = postcard::to_stdvec(&Wire { message, signature }).map_err(|e| e.to_string())?;
    let mut encoded = Vec::with_capacity(PREFIX.len() + payload.len());
    encoded.extend_from_slice(PREFIX);
    encoded.extend_from_slice(&payload);
    Ok(encoded)
}

/// An accepted message from channel bytes; `None` when the bytes are
/// something else on the channel, `Err` when they claim to be a signal and
/// are not one this device accepts: malformed, not signed by the node they
/// name for this room, or a question marker or live segment from outside
/// the `panel`.
pub(super) fn receive(
    bytes: &[u8],
    topic: &[u8],
    panel: &Panel,
) -> Option<Result<SignalMessage, String>> {
    let payload = bytes.strip_prefix(PREFIX)?;
    Some(accept(payload, topic, panel))
}

fn accept(payload: &[u8], topic: &[u8], panel: &Panel) -> Result<SignalMessage, String> {
    use ed25519_dalek::{Signature, VerifyingKey};
    let Wire { message, signature } =
        postcard::from_bytes::<Wire>(payload).map_err(|e| e.to_string())?;
    message.signal.validate()?;
    let node: [u8; 32] = hex::decode(&message.sender)
        .ok()
        .and_then(|k| k.try_into().ok())
        .ok_or("a signal names its sender's node")?;
    let node = VerifyingKey::from_bytes(&node).map_err(|e| e.to_string())?;
    let signature = Signature::from_slice(&signature).map_err(|e| e.to_string())?;
    node.verify_strict(&signing_bytes(topic, &message), &signature)
        .map_err(|_| "a signal not signed by the node it names".to_string())?;
    let panel_only = matches!(
        message.signal,
        InterviewSignal::Question { .. } | InterviewSignal::LiveSegment { .. }
    );
    if panel_only && !panel.contains(&message.sender) {
        return Err("only the interview's panel sends question markers and live segments".into());
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: &[u8] = b"room-topic";
    const TIME: u64 = 1_700_000_000_000;

    fn key(seed: u8) -> iroh::SecretKey {
        iroh::SecretKey::from_bytes(&[seed; 32])
    }

    fn panel_of(keys: &[&iroh::SecretKey]) -> Panel {
        keys.iter().map(|k| k.public().to_string()).collect()
    }

    fn send(signal: InterviewSignal, from: &iroh::SecretKey) -> Vec<u8> {
        encode(signal, Some("Someone".into()), TIME, from, ROOM).unwrap()
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

    fn question() -> InterviewSignal {
        InterviewSignal::Question {
            question_id: "q-1".into(),
        }
    }

    fn live() -> InterviewSignal {
        InterviewSignal::LiveSegment {
            segment_id: "live-1".into(),
        }
    }

    #[test]
    fn each_signal_survives_the_channel_with_its_signers_id_as_sender() {
        let panelist = key(1);
        let panel = panel_of(&[&panelist]);
        for signal in [
            question(),
            answer("q-1", 4_200),
            InterviewSignal::SecondVoice,
            live(),
        ] {
            let received = receive(&send(signal.clone(), &panelist), ROOM, &panel)
                .unwrap()
                .unwrap();
            assert_eq!(received.signal, signal);
            assert_eq!(received.sender, panelist.public().to_string());
            assert_eq!(received.timestamp, TIME);
        }
    }

    #[test]
    fn a_sender_the_signature_does_not_bind_is_refused() {
        let (panelist, candidate) = (key(1), key(2));
        let panel = panel_of(&[&panelist]);
        // The candidate's own second node claims to be the panelist.
        let message = SignalMessage {
            sender: panelist.public().to_string(),
            sender_name: Some("Panelist".into()),
            signal: live(),
            timestamp: TIME,
        };
        let signature = candidate.sign(&signing_bytes(ROOM, &message)).to_bytes();
        let mut forged = PREFIX.to_vec();
        forged.extend(
            postcard::to_stdvec(&Wire {
                message,
                signature: signature.to_vec(),
            })
            .unwrap(),
        );
        assert!(matches!(receive(&forged, ROOM, &panel), Some(Err(_))));

        // A signal signed for another room does not carry over.
        let elsewhere = encode(live(), None, TIME, &panelist, b"other-room").unwrap();
        assert!(matches!(receive(&elsewhere, ROOM, &panel), Some(Err(_))));
    }

    #[test]
    fn question_markers_and_live_segments_come_only_from_the_panel() {
        let (panelist, outsider) = (key(1), key(3));
        let panel = panel_of(&[&panelist]);
        for signal in [question(), live()] {
            assert!(matches!(
                receive(&send(signal, &outsider), ROOM, &panel),
                Some(Err(_))
            ));
        }
        // The candidate's own reports are not panel signals.
        for signal in [answer("q-1", 900), InterviewSignal::SecondVoice] {
            assert!(matches!(
                receive(&send(signal, &outsider), ROOM, &panel),
                Some(Ok(_))
            ));
        }
        // A device that knows no panel accepts no marker at all.
        assert!(matches!(
            receive(&send(question(), &panelist), ROOM, &Panel::new()),
            Some(Err(_))
        ));
    }

    #[test]
    fn chat_and_captions_on_the_channel_are_not_signals() {
        let panel = Panel::new();
        assert_eq!(receive(b"ALXTR1whatever", ROOM, &panel), None);
        assert_eq!(receive(b"hello", ROOM, &panel), None);
    }

    #[test]
    fn a_signal_no_honest_app_sends_is_refused_both_ways() {
        let panelist = key(1);
        let panel = panel_of(&[&panelist]);
        for bad in [
            answer("", 100),
            answer("has spaces", 100),
            answer(&"x".repeat(129), 100),
            answer("q", 600_001),
            InterviewSignal::LiveSegment {
                segment_id: "not a token".into(),
            },
        ] {
            assert!(encode(bad.clone(), None, TIME, &panelist, ROOM).is_err());
            let message = SignalMessage {
                sender: panelist.public().to_string(),
                sender_name: None,
                signal: bad,
                timestamp: TIME,
            };
            let signature = panelist.sign(&signing_bytes(ROOM, &message)).to_bytes();
            let mut forged = PREFIX.to_vec();
            forged.extend(
                postcard::to_stdvec(&Wire {
                    message,
                    signature: signature.to_vec(),
                })
                .unwrap(),
            );
            assert!(matches!(receive(&forged, ROOM, &panel), Some(Err(_))));
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
