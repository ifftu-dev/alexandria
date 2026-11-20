//! Keeps assessment items out of chat.
//!
//! Chat never reads the assessment bank as context, but a learner can paste
//! an item they saw, and a model asked for practice can reproduce one it was
//! trained on. Both are checked against the items stored on this device with
//! a character-shingle containment test: what fraction of an item's shingles
//! appear in the text. Containment rather than similarity, so an item pasted
//! inside a longer message is still caught.
//!
//! Only item prompts and options are compared. Answer keys are never read.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use rusqlite::Connection;
use serde_json::Value;

/// Shingle width in characters after normalization.
const SHINGLE: usize = 5;
/// Fraction of an item's shingles that must appear in the text.
const THRESHOLD: f64 = 0.6;
/// Items shorter than this (normalized) are too generic to compare.
const MIN_ITEM_CHARS: usize = 30;
/// Most items read per check. Banks on one device are far smaller.
const MAX_ITEMS: i64 = 20_000;

fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space && !out.is_empty() {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

fn shingles(normalized: &str) -> HashSet<u64> {
    let chars: Vec<char> = normalized.chars().collect();
    if chars.len() < SHINGLE {
        return HashSet::new();
    }
    chars
        .windows(SHINGLE)
        .map(|w| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            w.hash(&mut hasher);
            hasher.finish()
        })
        .collect()
}

/// Fraction of `item`'s shingles found in `text`.
fn containment(item: &HashSet<u64>, text: &HashSet<u64>) -> f64 {
    if item.is_empty() {
        return 0.0;
    }
    item.iter().filter(|s| text.contains(s)).count() as f64 / item.len() as f64
}

/// The learner-visible text of an item: its prompt and options.
fn item_text(content_public: &str) -> Option<String> {
    let value: Value = serde_json::from_str(content_public).ok()?;
    let mut parts: Vec<&str> = Vec::new();
    for key in ["prompt", "question", "stem"] {
        if let Some(s) = value.get(key).and_then(Value::as_str) {
            parts.push(s);
        }
    }
    if let Some(options) = value.get("options").and_then(Value::as_array) {
        for option in options {
            if let Some(s) = option
                .as_str()
                .or_else(|| option.get("text").and_then(Value::as_str))
            {
                parts.push(s);
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Most hosted items held at once; the oldest goes first.
const MAX_HELD: usize = 2_000;

/// Shingles of items hosted attempts showed in this run of the app. Only the
/// hashes are kept: a hosted item's text is never stored anywhere.
fn held() -> &'static std::sync::Mutex<std::collections::VecDeque<HashSet<u64>>> {
    static HELD: std::sync::OnceLock<std::sync::Mutex<std::collections::VecDeque<HashSet<u64>>>> =
        std::sync::OnceLock::new();
    HELD.get_or_init(Default::default)
}

/// Register an item a hosted attempt showed, so chat refuses it as it refuses
/// a stored item: the question on its own and with its options. The item's
/// text is not kept, only its shingle hashes, for the rest of this run.
pub fn hold_hosted_item(question: &str, options: &[String]) {
    let whole = format!("{question} {}", options.join(" "));
    let Ok(mut held) = held().lock() else {
        return;
    };
    for text in [question, whole.as_str()] {
        let normalized = normalize(text);
        if normalized.chars().count() < MIN_ITEM_CHARS {
            continue;
        }
        if held.len() >= MAX_HELD {
            held.pop_front();
        }
        held.push_back(shingles(&normalized));
    }
}

/// Whether `text` contains an item a hosted attempt showed. Fails closed.
fn matches_held_item(text: &HashSet<u64>) -> bool {
    let Ok(held) = held().lock() else {
        return true;
    };
    held.iter().any(|item| containment(item, text) >= THRESHOLD)
}

/// Whether `text` contains a stored assessment item, by prompt and options,
/// or an item a hosted attempt showed ([`hold_hosted_item`]). Fails closed:
/// if the items cannot be read, the text is treated as a match.
pub fn matches_assessment_item(conn: &Connection, text: &str) -> bool {
    let text = shingles(&normalize(text));
    if text.is_empty() {
        return false;
    }
    if matches_held_item(&text) {
        return true;
    }
    let Ok(mut stmt) =
        conn.prepare("SELECT content_public FROM assessment_items ORDER BY id LIMIT ?1")
    else {
        return true;
    };
    let Ok(rows) = stmt.query_map([MAX_ITEMS], |r| r.get::<_, String>(0)) else {
        return true;
    };
    for row in rows {
        let Ok(content) = row else { return true };
        let Some(item) = item_text(&content) else {
            continue;
        };
        let normalized = normalize(&item);
        if normalized.chars().count() < MIN_ITEM_CHARS {
            continue;
        }
        if containment(&shingles(&normalized), &text) >= THRESHOLD {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE assessment_items (id TEXT PRIMARY KEY, content_public TEXT NOT NULL);
             INSERT INTO assessment_items VALUES
               ('q1', '{\"kind\":\"single\",\"prompt\":\"A bag has 4 red and 6 blue marbles. Two are drawn without replacement. What is the probability the second is red given the first was red?\",\"options\":[\"4/10\",\"3/9\",\"4/9\"]}'),
               ('q2', '{\"prompt\":\"Pick one\",\"options\":[\"a\",\"b\"]}');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn a_pasted_item_is_caught_even_inside_a_longer_message() {
        let conn = db();
        let pasted = "Can you solve this for me quickly? A bag has 4 red and 6 blue marbles. Two are \
                      drawn without replacement. What is the probability the second is red, given the \
                      first was red? Options 4/10, 3/9, 4/9. Thanks!";
        assert!(matches_assessment_item(&conn, pasted));
        // Formatting changes do not hide it.
        assert!(matches_assessment_item(
            &conn,
            &pasted.to_uppercase().replace(' ', "  ")
        ));
    }

    #[test]
    fn ordinary_questions_on_the_same_topic_pass() {
        let conn = db();
        assert!(!matches_assessment_item(
            &conn,
            "Why does drawing without replacement change the probability of the second draw?"
        ));
        assert!(!matches_assessment_item(
            &conn,
            "A jar has 3 green and 5 yellow beads; what is P(two yellow) without replacement?"
        ));
        // Very short items are too generic to compare.
        assert!(!matches_assessment_item(&conn, "Pick one: a or b"));
    }

    #[test]
    fn unreadable_items_fail_closed() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(matches_assessment_item(
            &conn,
            "anything at all long enough to compare"
        ));
    }
}
