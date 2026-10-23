//! Learning memory: what the learner chose to have chats remember.
//!
//! Two kinds, both written in the learner's own terms:
//!
//! * preferences: language, explanation style, accessibility needs;
//! * observations: a recurring difficulty, recorded as something noticed,
//!   not as a verified fact about the learner.
//!
//! Authoritative records (proven skills, assessment results) are never
//! memory; they belong to their own services. A model may propose an item by
//! ending its reply with a fenced `memory` block. The proposal is stored for
//! the learner to save, edit, or dismiss; a model never approves its own.
//! Only approved, active items are sent, they are listed in review, and a
//! prepared request is bound to each item's revision.

use serde::{Deserialize, Serialize};

pub const MAX_CONTENT_CHARS: usize = 300;
/// Most approved items kept at once.
pub const MAX_ITEMS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Preference,
    Observation,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Preference => "preference",
            Kind::Observation => "observation",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "preference" => Some(Kind::Preference),
            "observation" => Some(Kind::Observation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub kind: Kind,
    pub content: String,
}

#[derive(Deserialize)]
struct Raw {
    kind: String,
    content: String,
}

/// Normalizes and bounds memory text written by a learner or proposed by a
/// model. Single line, trimmed, at most [`MAX_CONTENT_CHARS`].
pub fn clean(content: &str) -> Option<String> {
    let text = content.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty() && text.chars().count() <= MAX_CONTENT_CHARS).then_some(text)
}

pub fn parse(block: &str) -> Option<Proposal> {
    let raw: Raw = serde_json::from_str(block).ok()?;
    Some(Proposal {
        kind: Kind::parse(raw.kind.trim())?,
        content: clean(&raw.content)?,
    })
}

/// Splits a fenced block tagged `tag` (```tag ... ```) out of a reply.
/// Returns the reply without the block, and the block body. An unterminated
/// block is dropped from the shown text.
pub fn split_block<'a>(reply: &'a str, tag: &str) -> (String, Option<&'a str>) {
    let fence = format!("```{tag}");
    let Some(start) = reply.find(&fence) else {
        return (reply.to_string(), None);
    };
    let body_start = start + fence.len();
    let Some(len) = reply[body_start..].find("```") else {
        return (reply[..start].trim_end().to_string(), None);
    };
    let body = &reply[body_start..body_start + len];
    let parts: Vec<&str> = [reply[..start].trim(), reply[body_start + len + 3..].trim()]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect();
    (parts.join("\n\n"), Some(body.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposals_are_bounded_single_line_and_typed() {
        let p = parse(r#"{"kind":"preference","content":"  Prefers worked\nnumbers   first "}"#)
            .unwrap();
        assert_eq!(
            p,
            Proposal {
                kind: Kind::Preference,
                content: "Prefers worked numbers first".into()
            }
        );
        assert!(parse(r#"{"kind":"diagnosis","content":"x"}"#).is_none());
        assert!(parse(r#"{"kind":"observation","content":""}"#).is_none());
        let long = format!(
            r#"{{"kind":"observation","content":"{}"}}"#,
            "x".repeat(MAX_CONTENT_CHARS + 1)
        );
        assert!(parse(&long).is_none());
        assert!(parse("not json").is_none());
    }

    #[test]
    fn blocks_split_by_tag() {
        let reply = "Answer.\n```memory\n{\"kind\":\"preference\",\"content\":\"x\"}\n```";
        let (text, block) = split_block(reply, "memory");
        assert_eq!(text, "Answer.");
        assert!(block.unwrap().contains("preference"));
        assert_eq!(split_block(reply, "practice").1, None);
    }
}
