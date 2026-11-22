//! Per-activity grading. Every item score is an integer in basis points
//! (0..=10_000); the overall score is one division at the very end, so the
//! result is identical on every platform.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::text;

pub const FULL: u32 = 10_000;

/// Graded outcome before it becomes the ABI envelope.
pub struct Outcome {
    /// Sum of item scores in basis points.
    pub sum_bp: u64,
    /// Number of items the sum is over.
    pub items: u64,
    pub details: Value,
}

impl Outcome {
    pub fn score(&self) -> f64 {
        if self.items == 0 {
            return 0.0;
        }
        self.sum_bp as f64 / (self.items * FULL as u64) as f64
    }
}

fn bp(num: usize, den: usize) -> u32 {
    if den == 0 {
        return 0;
    }
    ((num as u64 * FULL as u64) / den as u64) as u32
}

fn bp_to_f64(v: u32) -> f64 {
    v as f64 / FULL as f64
}

fn parse<T: for<'de> Deserialize<'de>>(value: &Value, what: &str) -> Result<T, String> {
    T::deserialize(value).map_err(|e| format!("invalid {what}: {e}"))
}

fn answers_from(submission: &Value) -> Result<Vec<String>, String> {
    #[derive(Deserialize)]
    struct Answers {
        #[serde(default)]
        answers: Vec<Option<String>>,
    }
    let a: Answers = parse(submission, "submission")?;
    Ok(a.answers
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect())
}

fn private(content: &Value) -> &Value {
    content.get("grader_private").unwrap_or(&Value::Null)
}

pub fn grade(content: &Value, submission: &Value) -> Result<Outcome, String> {
    let activity = content
        .get("activity")
        .and_then(Value::as_str)
        .ok_or("content.activity is missing")?;
    match activity {
        "dictation" => dictation(content, submission),
        "reading" => reading(content, submission),
        "writing" => writing(content, submission),
        "builder" => builder(content, submission),
        "read_aloud" => Err("read_aloud is reviewed by an instructor, not graded".to_string()),
        other => Err(format!("unknown activity '{other}'")),
    }
}

// ---------------------------------------------------------------- dictation

#[derive(Deserialize)]
struct DictationContent {
    items: Vec<DictationItem>,
}

#[derive(Deserialize)]
struct DictationItem {
    text: String,
}

fn dictation(content: &Value, submission: &Value) -> Result<Outcome, String> {
    let c: DictationContent = parse(content, "dictation content")?;
    if c.items.is_empty() {
        return Err("dictation needs at least one item".into());
    }
    let answers = answers_from(submission)?;
    let mut sum = 0u64;
    let mut items = Vec::with_capacity(c.items.len());
    for (i, item) in c.items.iter().enumerate() {
        let expected = text::tokens(&item.text);
        if expected.is_empty() {
            return Err(format!("dictation item {i} has no words"));
        }
        let typed = text::tokens(answers.get(i).map(String::as_str).unwrap_or(""));
        let (matched, ops) = text::word_diff(&expected, &typed);
        let score = bp(matched, expected.len().max(typed.len()));
        sum += score as u64;
        let diff: Vec<Value> = ops
            .iter()
            .map(|(op, w)| json!({ "word": w, "status": op.as_str() }))
            .collect();
        items.push(json!({
            "score": bp_to_f64(score),
            "matched": matched,
            "expected": expected.len(),
            "typed": typed.len(),
            "diff": diff,
        }));
    }
    Ok(Outcome {
        sum_bp: sum,
        items: c.items.len() as u64,
        details: json!({ "activity": "dictation", "items": items }),
    })
}

// ------------------------------------------------------------------ reading

#[derive(Deserialize)]
struct ReadingContent {
    #[serde(default)]
    questions: Vec<ReadingQuestion>,
}

#[derive(Deserialize)]
struct ReadingQuestion {
    #[serde(default)]
    options: Vec<Value>,
}

#[derive(Deserialize)]
struct ReadingKey {
    answers: Vec<u32>,
}

#[derive(Deserialize)]
struct ReadingSubmission {
    #[serde(default)]
    selected: Vec<Option<u32>>,
}

fn reading(content: &Value, submission: &Value) -> Result<Outcome, String> {
    let c: ReadingContent = parse(content, "reading content")?;
    let key: ReadingKey = parse(private(content), "reading grader_private")?;
    if key.answers.is_empty() {
        return Err("reading needs at least one answer".into());
    }
    if !c.questions.is_empty() && c.questions.len() != key.answers.len() {
        return Err("reading questions and answers differ in length".into());
    }
    for (i, &a) in key.answers.iter().enumerate() {
        if let Some(q) = c.questions.get(i) {
            if !q.options.is_empty() && a as usize >= q.options.len() {
                return Err(format!("reading answer {i} is out of range"));
            }
        }
    }
    let s: ReadingSubmission = parse(submission, "submission")?;
    let correct: Vec<bool> = key
        .answers
        .iter()
        .enumerate()
        .map(|(i, &a)| s.selected.get(i).copied().flatten() == Some(a))
        .collect();
    let correct_count = correct.iter().filter(|c| **c).count();
    Ok(Outcome {
        sum_bp: correct_count as u64 * FULL as u64,
        items: key.answers.len() as u64,
        details: json!({
            "activity": "reading",
            "correct": correct,
            "correct_count": correct_count,
            "total": key.answers.len(),
        }),
    })
}

// ------------------------------------------------------------------ writing

#[derive(Deserialize)]
struct WritingContent {
    items: Vec<Value>,
}

#[derive(Deserialize)]
struct WritingKeys {
    items: Vec<WritingKey>,
}

#[derive(Deserialize)]
struct WritingKey {
    #[serde(default)]
    accept: Vec<String>,
    #[serde(default)]
    require: Vec<String>,
    #[serde(default)]
    min_words: Option<u32>,
    /// Generic checks to run. Default: capital, end_punctuation, articles.
    #[serde(default)]
    checks: Option<Vec<String>>,
}

const DEFAULT_CHECKS: &[&str] = &["capital", "end_punctuation", "articles"];

fn writing(content: &Value, submission: &Value) -> Result<Outcome, String> {
    let c: WritingContent = parse(content, "writing content")?;
    let keys: WritingKeys = parse(private(content), "writing grader_private")?;
    if c.items.is_empty() {
        return Err("writing needs at least one item".into());
    }
    if c.items.len() != keys.items.len() {
        return Err("writing items and grader_private items differ in length".into());
    }
    let answers = answers_from(submission)?;
    let mut sum = 0u64;
    let mut items = Vec::with_capacity(keys.items.len());
    for (i, key) in keys.items.iter().enumerate() {
        let (score, detail) = writing_item(key, answers.get(i).map(String::as_str).unwrap_or(""))?;
        sum += score as u64;
        items.push(detail);
    }
    Ok(Outcome {
        sum_bp: sum,
        items: keys.items.len() as u64,
        details: json!({ "activity": "writing", "items": items }),
    })
}

fn writing_item(key: &WritingKey, answer: &str) -> Result<(u32, Value), String> {
    let words = text::tokens(answer);
    if words.is_empty() {
        return Ok((
            0,
            json!({ "score": 0.0, "accepted": false, "passed": 0, "total": 0, "failed": ["empty"] }),
        ));
    }
    let normalized = words.join(" ");
    if key.accept.iter().any(|a| text::normalize(a) == normalized) {
        return Ok((
            FULL,
            json!({ "score": 1.0, "accepted": true, "passed": 0, "total": 0, "failed": [] }),
        ));
    }

    let mut total = 0usize;
    let mut failed: Vec<String> = Vec::new();
    let mut article_issues: Vec<Value> = Vec::new();
    let checks: Vec<&str> = match &key.checks {
        Some(list) => list.iter().map(String::as_str).collect(),
        None => DEFAULT_CHECKS.to_vec(),
    };
    for check in checks {
        total += 1;
        let ok = match check {
            "capital" => text::starts_with_capital(answer),
            "end_punctuation" => text::ends_with_terminal(answer),
            "articles" => {
                let issues = text::article_issues(&words);
                for issue in &issues {
                    article_issues.push(json!({
                        "article": issue.article,
                        "next": issue.next,
                        "expected": issue.expected,
                    }));
                }
                issues.is_empty()
            }
            other => return Err(format!("unknown writing check '{other}'")),
        };
        if !ok {
            failed.push(check.to_string());
        }
    }
    for req in &key.require {
        let phrase = text::tokens(req);
        if phrase.is_empty() {
            continue;
        }
        total += 1;
        if !text::contains_phrase(&words, &phrase) {
            failed.push(format!("require:{}", phrase.join(" ")));
        }
    }
    if let Some(min) = key.min_words {
        total += 1;
        if (words.len() as u32) < min {
            failed.push("min_words".to_string());
        }
    }
    let passed = total - failed.len();
    let score = if total == 0 { FULL } else { bp(passed, total) };
    Ok((
        score,
        json!({
            "score": bp_to_f64(score),
            "accepted": false,
            "passed": passed,
            "total": total,
            "failed": failed,
            "article_issues": article_issues,
            "words": words.len(),
        }),
    ))
}

// ------------------------------------------------------------------ builder

#[derive(Deserialize)]
struct BuilderContent {
    items: Vec<BuilderItem>,
}

#[derive(Deserialize)]
struct BuilderItem {
    tiles: Vec<String>,
}

#[derive(Deserialize)]
struct BuilderKeys {
    items: Vec<BuilderKey>,
}

#[derive(Deserialize)]
struct BuilderKey {
    answer: String,
    #[serde(default)]
    also: Vec<String>,
}

#[derive(Deserialize)]
struct BuilderSubmission {
    #[serde(default)]
    orders: Vec<Option<Vec<i64>>>,
}

fn builder(content: &Value, submission: &Value) -> Result<Outcome, String> {
    let c: BuilderContent = parse(content, "builder content")?;
    let keys: BuilderKeys = parse(private(content), "builder grader_private")?;
    if c.items.is_empty() {
        return Err("builder needs at least one item".into());
    }
    if c.items.len() != keys.items.len() {
        return Err("builder items and grader_private items differ in length".into());
    }
    let s: BuilderSubmission = parse(submission, "submission")?;
    let mut correct = Vec::with_capacity(c.items.len());
    for (i, (item, key)) in c.items.iter().zip(keys.items.iter()).enumerate() {
        let order = s.orders.get(i).cloned().flatten().unwrap_or_default();
        correct.push(builder_item(&item.tiles, key, &order));
    }
    let correct_count = correct.iter().filter(|c| **c).count();
    Ok(Outcome {
        sum_bp: correct_count as u64 * FULL as u64,
        items: c.items.len() as u64,
        details: json!({
            "activity": "builder",
            "correct": correct,
            "correct_count": correct_count,
            "total": c.items.len(),
        }),
    })
}

fn builder_item(tiles: &[String], key: &BuilderKey, order: &[i64]) -> bool {
    let mut used = alloc::vec![false; tiles.len()];
    let mut words: Vec<&str> = Vec::with_capacity(order.len());
    for &idx in order {
        if idx < 0 || idx as usize >= tiles.len() || used[idx as usize] {
            return false;
        }
        used[idx as usize] = true;
        words.push(&tiles[idx as usize]);
    }
    if words.is_empty() {
        return false;
    }
    let built = text::normalize(&words.join(" "));
    core::iter::once(&key.answer)
        .chain(key.also.iter())
        .any(|a| text::normalize(a) == built)
}
