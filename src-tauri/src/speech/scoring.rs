//! Turning per-token probabilities into per-word results.
//!
//! The engine teacher-forces the target sentence through Whisper's decoder
//! and reads, for each target token, the probability the model gave it after
//! hearing the recording. A word's score is the geometric mean of its
//! tokens' probabilities, so one badly heard syllable pulls the word down
//! without a long word being punished for its length.
//!
//! Thresholds were calibrated on synthetic speech (macOS `say`, US, UK and
//! Indian English voices plus Spanish, French, German, Portuguese, Arabic,
//! Chinese and Japanese; see the ignored `calibration` tests in `engine.rs`).
//! Words that were spoken scored a median of 0.94–0.99 (10th percentile
//! 0.65–0.94 by model); every target word the speaker replaced with another
//! scored below 0.02. Between the two sit words that were said but written
//! differently from what the model expects ("harbour" heard as "harbor":
//! 0.19–0.30) and words next to a replaced one (0.39), which is what
//! "unclear" is for. Silence and background noise are caught separately by
//! Whisper's no-speech probability (`engine::NO_SPEECH_AT`).

use std::ops::Range;

use serde::{Deserialize, Serialize};

use super::tokenizer::Tokenizer;

/// A word at or above this score came across clearly.
pub const CLEAR_AT: f32 = 0.5;
/// A word at or above this score (and below [`CLEAR_AT`]) was heard, but not
/// clearly; below it the word was most likely missed or replaced.
pub const UNCLEAR_AT: f32 = 0.15;

/// Longest target the engine accepts, in tokens. Whisper's decoder holds 448
/// positions; this leaves room for the prompt and stays well inside it.
pub const MAX_TARGET_TOKENS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordStatus {
    Clear,
    Unclear,
    Missed,
}

pub fn status_for(score: f32) -> WordStatus {
    if score >= CLEAR_AT {
        WordStatus::Clear
    } else if score >= UNCLEAR_AT {
        WordStatus::Unclear
    } else {
        WordStatus::Missed
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordScore {
    pub word: String,
    pub score: f32,
    pub status: WordStatus,
}

/// One unit the learner is scored on: a word, or a character in languages
/// written without spaces.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub word: String,
    /// Bytes of [`Target::text`] the unit covers.
    pub range: Range<usize>,
}

/// The target sentence, tokenised for teacher forcing.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// Normalised text, with Whisper's leading space in spaced languages
    /// (" She is a doctor.").
    pub text: String,
    pub ids: Vec<u32>,
    /// For each token, the units it is scored against. Empty for tokens that
    /// carry only punctuation or spaces, which are fed to the model but not
    /// scored.
    pub token_units: Vec<Vec<usize>>,
    pub units: Vec<Unit>,
}

/// Tokenise `text` and work out which tokens belong to which word.
///
/// Words are whitespace-separated. In languages written without spaces
/// (`per_character`), each letter is its own unit, and a token that spans two
/// characters (or half a character) counts toward each one it touches.
pub fn target(tokenizer: &Tokenizer, text: &str, per_character: bool) -> anyhow::Result<Target> {
    let words: Vec<&str> = text.split_whitespace().collect();
    // Whisper writes a leading space before text in spaced languages and
    // none before Chinese or Japanese, so the target follows suit.
    let lead = if per_character { "" } else { " " };
    let text = format!("{lead}{}", words.join(" "));

    let mut units = Vec::new();
    let mut word_start = 0;
    for (index, word) in words.iter().enumerate() {
        // Skip the separating (or leading) space.
        if index > 0 || !per_character {
            word_start += 1;
        }
        let word_range = word_start..word_start + word.len();
        if per_character {
            for (offset, ch) in word.char_indices() {
                if ch.is_alphanumeric() {
                    let start = word_range.start + offset;
                    units.push(Unit {
                        word: ch.to_string(),
                        range: start..start + ch.len_utf8(),
                    });
                }
            }
        } else if word.chars().any(char::is_alphanumeric) {
            units.push(Unit {
                word: (*word).to_string(),
                range: word_range.clone(),
            });
        }
        word_start = word_range.end;
    }
    if units.is_empty() {
        anyhow::bail!("the target text has no words to check");
    }

    // Byte ranges of alphanumeric characters; a token is scored only if it
    // touches one.
    let letters: Vec<Range<usize>> = text
        .char_indices()
        .filter(|(_, ch)| ch.is_alphanumeric())
        .map(|(at, ch)| at..at + ch.len_utf8())
        .collect();
    let overlaps = |a: &Range<usize>, b: &Range<usize>| a.start < b.end && b.start < a.end;

    let encoded = tokenizer.encode_with_spans(&text)?;
    if encoded.len() > MAX_TARGET_TOKENS {
        anyhow::bail!("the target text is too long to check (keep it to one or two sentences)");
    }
    let mut ids = Vec::with_capacity(encoded.len());
    let mut token_units = Vec::with_capacity(encoded.len());
    for (id, span) in encoded {
        ids.push(id);
        let scored = letters.iter().any(|letter| overlaps(letter, &span));
        token_units.push(if scored {
            units
                .iter()
                .enumerate()
                .filter(|(_, unit)| overlaps(&unit.range, &span))
                .map(|(index, _)| index)
                .collect()
        } else {
            Vec::new()
        });
    }
    Ok(Target {
        text,
        ids,
        token_units,
        units,
    })
}

/// Word scores from the natural-log probability of each target token, plus
/// the overall score (the mean word score).
pub fn score(target: &Target, log_probs: &[f32]) -> (Vec<WordScore>, f32) {
    let mut sums = vec![(0f32, 0usize); target.units.len()];
    for (units, &log_prob) in target.token_units.iter().zip(log_probs) {
        for &unit in units {
            sums[unit].0 += log_prob;
            sums[unit].1 += 1;
        }
    }
    let words: Vec<WordScore> = target
        .units
        .iter()
        .zip(sums)
        .map(|(unit, (sum, count))| {
            let score = if count == 0 {
                0.0
            } else {
                (sum / count as f32).exp().clamp(0.0, 1.0)
            };
            WordScore {
                word: unit.word.clone(),
                score,
                status: status_for(score),
            }
        })
        .collect();
    let overall = if words.is_empty() {
        0.0
    } else {
        words.iter().map(|w| w.score).sum::<f32>() / words.len() as f32
    };
    (words, overall)
}

#[cfg(test)]
mod tests {
    use super::super::tokenizer::tests::toy;
    use super::*;

    #[test]
    fn thresholds_map_scores_to_statuses() {
        assert_eq!(status_for(1.0), WordStatus::Clear);
        assert_eq!(status_for(CLEAR_AT), WordStatus::Clear);
        assert_eq!(status_for(CLEAR_AT - 0.001), WordStatus::Unclear);
        assert_eq!(status_for(UNCLEAR_AT), WordStatus::Unclear);
        assert_eq!(status_for(UNCLEAR_AT - 0.001), WordStatus::Missed);
        assert_eq!(status_for(0.0), WordStatus::Missed);
        assert_eq!(
            serde_json::to_string(&WordStatus::Unclear).unwrap(),
            "\"unclear\""
        );
    }

    #[test]
    fn groups_tokens_by_word_and_skips_punctuation() {
        let tokenizer = toy();
        // " She" " is" " ab" "c" "." — "abc." is one word of two scored tokens.
        let target = target(&tokenizer, "  She   is abc. ", false).unwrap();
        assert_eq!(target.text, " She is abc.");
        assert_eq!(target.ids, [20, 17, 14, 3, 5]);
        let words: Vec<_> = target.units.iter().map(|u| u.word.as_str()).collect();
        assert_eq!(words, ["She", "is", "abc."]);
        assert_eq!(
            target.token_units,
            [vec![0], vec![1], vec![2], vec![2], vec![]]
        );
    }

    #[test]
    fn word_score_is_the_geometric_mean_of_its_tokens() {
        let tokenizer = toy();
        let target = target(&tokenizer, "She is abc.", false).unwrap();
        let log_probs = [
            0.9f32.ln(),
            0.5f32.ln(),
            0.4f32.ln(),
            0.1f32.ln(),
            0.0001f32.ln(),
        ];
        let (words, overall) = score(&target, &log_probs);
        assert!((words[0].score - 0.9).abs() < 1e-5);
        assert_eq!(words[0].status, WordStatus::Clear);
        assert!((words[1].score - 0.5).abs() < 1e-5);
        // sqrt(0.4 * 0.1) = 0.2; the "." token is not scored.
        assert!((words[2].score - 0.2).abs() < 1e-5);
        assert_eq!(words[2].status, WordStatus::Unclear);
        assert!((overall - (0.9 + 0.5 + 0.2) / 3.0).abs() < 1e-5);
    }

    #[test]
    fn per_character_units_share_tokens_that_span_them() {
        let tokenizer = toy();
        // "abé": " ab" covers a and b; "é" is its own token.
        // No leading space in unspaced languages: "ab" covers a and b.
        let target = target(&tokenizer, "abé", true).unwrap();
        assert_eq!(target.text, "abé");
        let words: Vec<_> = target.units.iter().map(|u| u.word.as_str()).collect();
        assert_eq!(words, ["a", "b", "é"]);
        assert_eq!(target.ids, [13, 25]);
        assert_eq!(target.token_units, [vec![0, 1], vec![2]]);
        let (scores, _) = score(&target, &[0.25f32.ln(), 1.0f32.ln()]);
        assert!((scores[0].score - 0.25).abs() < 1e-5);
        assert!((scores[1].score - 0.25).abs() < 1e-5);
        assert!((scores[2].score - 1.0).abs() < 1e-5);
    }

    #[test]
    fn rejects_targets_without_words() {
        let tokenizer = toy();
        assert!(target(&tokenizer, "   ", false).is_err());
        assert!(target(&tokenizer, ". .", false).is_err());
    }
}
