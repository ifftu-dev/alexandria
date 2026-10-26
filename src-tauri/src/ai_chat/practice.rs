//! Practice questions as validated objects, not chat prose.
//!
//! In Practice mode the model is asked to end its reply with one question in
//! a fenced `practice` block of JSON. That block is parsed, validated, and
//! stored; the learner then answers against it and Alexandria checks the
//! answer itself. No model is asked whether an answer is right, so a score is
//! never invented, and a malformed block simply leaves a readable reply.
//!
//! Only kinds with a deterministic check are accepted: numeric (fractions,
//! decimals and percentages, within a tolerance), multiple choice, and short
//! text. Hints are given one at a time and showing the answer is explicit;
//! either makes later attempts on that question assisted. Practice never
//! touches credentials, progress, or assessments.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_PROMPT_CHARS: usize = 2_000;
pub const MAX_OPTIONS: usize = 6;
pub const MAX_OPTION_CHARS: usize = 300;
pub const MAX_HINTS: usize = 3;
pub const MAX_HINT_CHARS: usize = 600;
pub const MAX_EXPLANATION_CHARS: usize = 2_000;
pub const MAX_ANSWER_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Numeric,
    MultipleChoice,
    ShortText,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Numeric => "numeric",
            Kind::MultipleChoice => "multiple_choice",
            Kind::ShortText => "short_text",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "numeric" => Some(Kind::Numeric),
            "multiple_choice" => Some(Kind::MultipleChoice),
            "short_text" => Some(Kind::ShortText),
            _ => None,
        }
    }
}

/// The checked answer, in a form that needs no model to compare against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    Number {
        value: f64,
        tolerance: f64,
        display: String,
    },
    Choice {
        index: usize,
    },
    Text {
        accepted: Vec<String>,
    },
}

/// A validated practice question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub skill: String,
    pub kind: Kind,
    pub prompt: String,
    pub options: Vec<String>,
    pub answer: Answer,
    pub hints: Vec<String>,
    pub explanation: String,
}

/// What a model may write. Unknown fields are ignored.
#[derive(Debug, Deserialize)]
struct Raw {
    #[serde(default)]
    skill: Option<String>,
    kind: String,
    prompt: String,
    #[serde(default)]
    options: Vec<Value>,
    answer: Value,
    #[serde(default)]
    tolerance: Option<f64>,
    #[serde(default)]
    hints: Vec<Value>,
    #[serde(default)]
    explanation: Option<String>,
}

/// Splits a reply into the prose to show and the practice block, if any.
/// The block is removed from the prose whether or not it is valid.
pub fn split(reply: &str) -> (String, Option<&str>) {
    super::memory::split_block(reply, super::vocab::ReplyFence::Practice)
}

fn clip(text: &str, max: usize) -> Option<String> {
    let text = text.trim();
    (!text.is_empty() && text.chars().count() <= max).then(|| text.to_string())
}

/// Parses a number a learner or a model might write: `0.16`, `16%`, `2/3`,
/// `-1.5e3`, with commas as thousands separators.
pub fn parse_number(text: &str) -> Option<f64> {
    let t = text.trim().replace([',', ' '], "");
    if t.is_empty() || t.len() > 64 {
        return None;
    }
    let value = if let Some(p) = t.strip_suffix('%') {
        p.parse::<f64>().ok()? / 100.0
    } else if let Some((a, b)) = t.split_once('/') {
        let (a, b) = (a.parse::<f64>().ok()?, b.parse::<f64>().ok()?);
        if b == 0.0 {
            return None;
        }
        a / b
    } else {
        t.parse::<f64>().ok()?
    };
    value.is_finite().then_some(value)
}

fn normalize_text(text: &str) -> String {
    text.trim()
        .trim_matches(|c: char| c.is_ascii_punctuation())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Validates a model's practice block. `None` means it is not usable and
/// the reply is shown as prose only.
pub fn parse(block: &str) -> Option<Question> {
    let raw: Raw = serde_json::from_str(block).ok()?;
    let kind = Kind::parse(raw.kind.trim())?;
    let prompt = clip(&raw.prompt, MAX_PROMPT_CHARS)?;
    let skill = raw
        .skill
        .as_deref()
        .and_then(|s| clip(s, 120))
        .unwrap_or_default();
    let hints = raw
        .hints
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|h| clip(h, MAX_HINT_CHARS))
        .take(MAX_HINTS)
        .collect();
    let explanation = raw
        .explanation
        .as_deref()
        .and_then(|e| clip(e, MAX_EXPLANATION_CHARS))
        .unwrap_or_default();
    let (options, answer) = match kind {
        Kind::Numeric => {
            let value = match &raw.answer {
                Value::Number(n) => n.as_f64()?,
                Value::String(s) => parse_number(s)?,
                _ => return None,
            };
            let display = match &raw.answer {
                Value::String(s) => clip(s, 64)?,
                _ => value.to_string(),
            };
            // Half a percent of the answer by default, so 0.667 is accepted
            // for 2/3, never looser than the model asked for.
            let tolerance = raw
                .tolerance
                .filter(|t| t.is_finite() && *t >= 0.0)
                .unwrap_or_else(|| (value.abs() * 0.005).max(1e-9));
            (
                Vec::new(),
                Answer::Number {
                    value,
                    tolerance,
                    display,
                },
            )
        }
        Kind::MultipleChoice => {
            let options: Vec<String> = raw
                .options
                .iter()
                .map(|o| o.as_str().and_then(|s| clip(s, MAX_OPTION_CHARS)))
                .collect::<Option<Vec<_>>>()?;
            if !(2..=MAX_OPTIONS).contains(&options.len()) {
                return None;
            }
            let index = match &raw.answer {
                Value::Number(n) => usize::try_from(n.as_u64()?).ok()?,
                Value::String(s) => {
                    let want = normalize_text(s);
                    options.iter().position(|o| normalize_text(o) == want)?
                }
                _ => return None,
            };
            if index >= options.len() {
                return None;
            }
            (options, Answer::Choice { index })
        }
        Kind::ShortText => {
            let accepted: Vec<String> = match &raw.answer {
                Value::String(s) => vec![s.clone()],
                Value::Array(items) => items
                    .iter()
                    .filter_map(|i| i.as_str().map(str::to_owned))
                    .collect(),
                _ => return None,
            }
            .iter()
            .filter_map(|a| clip(a, MAX_ANSWER_CHARS))
            .map(|a| normalize_text(&a))
            .filter(|a| !a.is_empty())
            .take(8)
            .collect();
            if accepted.is_empty() {
                return None;
            }
            (Vec::new(), Answer::Text { accepted })
        }
    };
    Some(Question {
        skill,
        kind,
        prompt,
        options,
        answer,
        hints,
        explanation,
    })
}

/// Checks a learner's answer. `None` when the answer cannot be read at all
/// (an empty box, a word where a number was asked for), so it is not counted.
pub fn check(answer: &Answer, given: &str) -> Option<bool> {
    let given = given.trim();
    if given.is_empty() || given.chars().count() > MAX_ANSWER_CHARS {
        return None;
    }
    match answer {
        Answer::Number {
            value, tolerance, ..
        } => parse_number(given).map(|g| (g - value).abs() <= *tolerance),
        Answer::Choice { index } => given.parse::<usize>().ok().map(|g| g == *index),
        Answer::Text { accepted } => Some(accepted.contains(&normalize_text(given))),
    }
}

/// The answer as shown once solved or revealed.
pub fn display(question: &Question) -> String {
    match &question.answer {
        Answer::Number { display, .. } => display.clone(),
        Answer::Choice { index } => question.options.get(*index).cloned().unwrap_or_default(),
        Answer::Text { accepted } => accepted.first().cloned().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_is_split_from_the_prose() {
        let reply = "Here is one.\n\n```practice\n{\"kind\":\"numeric\"}\n```\nGood luck.";
        let (prose, block) = split(reply);
        assert_eq!(prose, "Here is one.\n\nGood luck.");
        assert_eq!(block, Some("{\"kind\":\"numeric\"}"));
        assert_eq!(split("No question here.").1, None);
        let (prose, block) = split("Text ```practice {\"kind\":");
        assert_eq!((prose.as_str(), block), ("Text", None));
    }

    #[test]
    fn numeric_questions_accept_fractions_decimals_and_percents() {
        let q = parse(r#"{"skill":"Conditional probability","kind":"numeric","prompt":"P(even > 3 | even)?","answer":"2/3","hints":["List the even faces."],"explanation":"4 and 6 of 2,4,6."}"#).unwrap();
        assert_eq!(q.kind, Kind::Numeric);
        assert_eq!(display(&q), "2/3");
        for ok in ["2/3", "0.667", "0.6667", "66.7%"] {
            assert_eq!(check(&q.answer, ok), Some(true), "{ok}");
        }
        for wrong in ["0.6", "1/2", "70%"] {
            assert_eq!(check(&q.answer, wrong), Some(false), "{wrong}");
        }
        assert_eq!(check(&q.answer, "two thirds"), None);
        assert_eq!(check(&q.answer, ""), None);
        assert_eq!(parse_number("1,200"), Some(1200.0));
        assert_eq!(parse_number("1/0"), None);
    }

    #[test]
    fn multiple_choice_answers_by_index_or_option_text() {
        let by_index = parse(r#"{"kind":"multiple_choice","prompt":"Pick","options":["1/4","1/3","1/2"],"answer":1}"#).unwrap();
        assert_eq!(by_index.answer, Answer::Choice { index: 1 });
        let by_text = parse(r#"{"kind":"multiple_choice","prompt":"Pick","options":["1/4","1/3","1/2"],"answer":"1/2"}"#).unwrap();
        assert_eq!(by_text.answer, Answer::Choice { index: 2 });
        assert_eq!(check(&by_text.answer, "2"), Some(true));
        assert_eq!(check(&by_text.answer, "0"), Some(false));
        assert_eq!(display(&by_text), "1/2");
        // An answer outside the options, or too few options, is unusable.
        assert!(
            parse(r#"{"kind":"multiple_choice","prompt":"P","options":["a","b"],"answer":5}"#)
                .is_none()
        );
        assert!(
            parse(r#"{"kind":"multiple_choice","prompt":"P","options":["a"],"answer":0}"#)
                .is_none()
        );
    }

    #[test]
    fn short_text_ignores_case_spacing_and_trailing_punctuation() {
        let q = parse(r#"{"kind":"short_text","prompt":"Name the rule","answer":["Bayes' theorem","Bayes rule"]}"#).unwrap();
        assert_eq!(check(&q.answer, "  bayes'   THEOREM. "), Some(true));
        assert_eq!(check(&q.answer, "bayes rule"), Some(true));
        assert_eq!(check(&q.answer, "total probability"), Some(false));
    }

    #[test]
    fn malformed_or_unsupported_blocks_are_rejected() {
        for bad in [
            "not json",
            r#"{"kind":"essay","prompt":"Discuss","answer":"x"}"#,
            r#"{"kind":"numeric","prompt":"","answer":1}"#,
            r#"{"kind":"numeric","prompt":"Q","answer":"lots"}"#,
            r#"{"kind":"short_text","prompt":"Q","answer":[]}"#,
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
        let long = format!(
            r#"{{"kind":"numeric","prompt":"{}","answer":1}}"#,
            "x".repeat(MAX_PROMPT_CHARS + 1)
        );
        assert!(parse(&long).is_none());
    }

    #[test]
    fn hints_are_bounded_and_unknown_fields_ignored() {
        let q = parse(r#"{"kind":"numeric","prompt":"Q","answer":1,"hints":["a","b","c","d",5],"difficulty":3,"answer_key_leak":"x"}"#).unwrap();
        assert_eq!(q.hints, vec!["a", "b", "c"]);
    }
}
