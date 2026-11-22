//! Pure text helpers shared by every activity: normalization, word-level
//! LCS diff, and the beginner writing checks. No floats here — callers turn
//! counts into scores.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Lowercase, turn every character that is not a letter, digit or
/// apostrophe into a space, then split on whitespace. Curly apostrophes
/// count as `'`. Apostrophes at the edge of a word (quotes) are dropped,
/// inner ones (`don't`, `I'm`) are kept.
pub fn tokens(text: &str) -> Vec<String> {
    let mut cleaned = String::with_capacity(text.len());
    for ch in text.chars() {
        let ch = if ch == '\u{2019}' || ch == '\u{2018}' || ch == '`' {
            '\''
        } else {
            ch
        };
        if ch.is_alphanumeric() || ch == '\'' {
            for lower in ch.to_lowercase() {
                cleaned.push(lower);
            }
        } else {
            cleaned.push(' ');
        }
    }
    cleaned
        .split_whitespace()
        .map(|w| String::from(w.trim_matches('\'')))
        .filter(|w| !w.is_empty())
        .collect()
}

/// Normalized form used for whole-answer comparisons: tokens joined by a
/// single space.
pub fn normalize(text: &str) -> String {
    tokens(text).join(" ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffOp {
    /// Word present in both, in order.
    Ok,
    /// Expected word the learner left out.
    Missing,
    /// Typed word that is not in the expected text.
    Extra,
}

impl DiffOp {
    pub fn as_str(self) -> &'static str {
        match self {
            DiffOp::Ok => "ok",
            DiffOp::Missing => "missing",
            DiffOp::Extra => "extra",
        }
    }
}

/// Word-level longest-common-subsequence diff. Returns the LCS length and
/// an ordered list of `(op, word)`. Ties prefer reporting the missing
/// expected word before the extra typed word, so a substitution reads as
/// "missing X, extra Y".
pub fn word_diff(expected: &[String], typed: &[String]) -> (usize, Vec<(DiffOp, String)>) {
    let n = expected.len();
    let m = typed.len();
    // dp[i][j] = LCS of expected[i..] and typed[j..]
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if expected[i] == typed[j] {
                dp[i + 1][j + 1] + 1
            } else if dp[i + 1][j] >= dp[i][j + 1] {
                dp[i + 1][j]
            } else {
                dp[i][j + 1]
            };
        }
    }
    let mut ops = Vec::with_capacity(n + m);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if expected[i] == typed[j] {
            ops.push((DiffOp::Ok, expected[i].clone()));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push((DiffOp::Missing, expected[i].clone()));
            i += 1;
        } else {
            ops.push((DiffOp::Extra, typed[j].clone()));
            j += 1;
        }
    }
    while i < n {
        ops.push((DiffOp::Missing, expected[i].clone()));
        i += 1;
    }
    while j < m {
        ops.push((DiffOp::Extra, typed[j].clone()));
        j += 1;
    }
    (dp[0][0] as usize, ops)
}

/// First letter or digit of the answer is not a lowercase letter.
pub fn starts_with_capital(text: &str) -> bool {
    match text.chars().find(|c| c.is_alphanumeric()) {
        Some(c) if c.is_alphabetic() => c.is_uppercase(),
        Some(_) => true,
        None => false,
    }
}

/// The answer ends in `.`, `?` or `!` (closing quotes/brackets after the
/// mark are allowed).
pub fn ends_with_terminal(text: &str) -> bool {
    let trimmed = text.trim_end_matches(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | ')' | '\u{201D}' | '\u{2019}')
    });
    matches!(trimmed.chars().last(), Some('.' | '?' | '!'))
}

/// Contiguous token sequence `needle` occurs in `hay`.
pub fn contains_phrase(hay: &[String], needle: &[String]) -> bool {
    if needle.is_empty() {
        return true;
    }
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Words starting with a vowel letter that still take "a".
const A_PREFIXES: &[&str] = &[
    "univers", "uniform", "unit", "union", "uniq", "use", "usu", "europ", "eulog", "ewe",
];
const A_EXACT: &[&str] = &["one", "once", "u"];
/// Words starting with a consonant letter that still take "an".
const AN_PREFIXES: &[&str] = &["hour", "honest", "honour", "honor", "heir"];

/// Which article a word wants, judged by spelling with a small exception
/// list. `None` for words starting with a digit (no opinion).
pub fn article_for(word: &str) -> Option<&'static str> {
    let first = word.chars().next()?;
    if !first.is_alphabetic() {
        return None;
    }
    if A_EXACT.contains(&word) || A_PREFIXES.iter().any(|p| word.starts_with(p)) {
        return Some("a");
    }
    if AN_PREFIXES.iter().any(|p| word.starts_with(p)) {
        return Some("an");
    }
    if matches!(first, 'a' | 'e' | 'i' | 'o' | 'u') {
        Some("an")
    } else {
        Some("a")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleIssue {
    pub article: String,
    pub next: String,
    pub expected: &'static str,
}

/// Every `a`/`an` followed by a word that wants the other article.
pub fn article_issues(words: &[String]) -> Vec<ArticleIssue> {
    let mut out = Vec::new();
    for pair in words.windows(2) {
        let article = pair[0].as_str();
        if article != "a" && article != "an" {
            continue;
        }
        if let Some(expected) = article_for(&pair[1]) {
            if expected != article {
                out.push(ArticleIssue {
                    article: pair[0].clone(),
                    next: pair[1].clone(),
                    expected,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Vec<String> {
        tokens(s)
    }

    #[test]
    fn tokens_normalize_case_punctuation_and_spaces() {
        assert_eq!(t("  Hello,   WORLD!! "), ["hello", "world"]);
        assert_eq!(t("I'm here. Don’t go"), ["i'm", "here", "don't", "go"]);
        assert_eq!(t("'quoted' word"), ["quoted", "word"]);
        assert_eq!(t("twenty-one"), ["twenty", "one"]);
        assert_eq!(normalize("The shop opens at 9."), "the shop opens at 9");
        assert!(t(" ...!? ").is_empty());
    }

    #[test]
    fn diff_reports_ok_missing_extra_in_order() {
        let (lcs, ops) = word_diff(&t("I drink tea every day"), &t("I drinks tea every day"));
        assert_eq!(lcs, 4);
        let ops: Vec<(&str, &str)> = ops.iter().map(|(o, w)| (o.as_str(), w.as_str())).collect();
        assert_eq!(
            ops,
            [
                ("ok", "i"),
                ("missing", "drink"),
                ("extra", "drinks"),
                ("ok", "tea"),
                ("ok", "every"),
                ("ok", "day"),
            ]
        );
    }

    #[test]
    fn diff_handles_empty_sides() {
        let (lcs, ops) = word_diff(&t("good morning"), &[]);
        assert_eq!(lcs, 0);
        assert!(ops.iter().all(|(o, _)| *o == DiffOp::Missing));
        let (lcs, ops) = word_diff(&[], &t("hi"));
        assert_eq!(lcs, 0);
        assert_eq!(ops, [(DiffOp::Extra, String::from("hi"))]);
    }

    #[test]
    fn capital_and_terminal_checks() {
        assert!(starts_with_capital("  \"My name is Ravi.\""));
        assert!(!starts_with_capital("my name"));
        assert!(starts_with_capital("9 am is early."));
        assert!(!starts_with_capital("   "));
        assert!(ends_with_terminal("Is it raining?  "));
        assert!(ends_with_terminal("He said \"yes.\""));
        assert!(!ends_with_terminal("No full stop"));
    }

    #[test]
    fn phrases_match_contiguously() {
        assert!(contains_phrase(
            &t("Hello, my name is Asha."),
            &t("my name is")
        ));
        assert!(!contains_phrase(
            &t("my good name is Asha"),
            &t("my name is")
        ));
    }

    #[test]
    fn articles_follow_spelling_with_exceptions() {
        assert_eq!(article_for("apple"), Some("an"));
        assert_eq!(article_for("book"), Some("a"));
        assert_eq!(article_for("hour"), Some("an"));
        assert_eq!(article_for("honest"), Some("an"));
        assert_eq!(article_for("heir"), Some("an"));
        assert_eq!(article_for("university"), Some("a"));
        assert_eq!(article_for("uniform"), Some("a"));
        assert_eq!(article_for("useful"), Some("a"));
        assert_eq!(article_for("european"), Some("a"));
        assert_eq!(article_for("one"), Some("a"));
        assert_eq!(article_for("umbrella"), Some("an"));
        assert_eq!(article_for("8"), None);

        let issues = article_issues(&t("I have a umbrella and an book, an hour, a university."));
        let got: Vec<(&str, &str, &str)> = issues
            .iter()
            .map(|i| (i.article.as_str(), i.next.as_str(), i.expected))
            .collect();
        assert_eq!(got, [("a", "umbrella", "an"), ("an", "book", "a")]);
    }
}
