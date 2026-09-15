//! Learner reads for assistants holding `learning:read`: the local catalog,
//! published course outlines and lesson content.
//!
//! Only published courses written by someone else are visible. An author's
//! own courses, whose local copy may hold unpublished edits, stay behind
//! `drafts:read`. Lessons return text, video chapter markers, and the
//! questions of quizzes and essays rebuilt from an allowlist of learner-visible
//! fields, so answer keys, explanations and anything a new format adds are
//! never copied. Credential-bearing assessments, interactive elements and
//! plugins are withheld.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::{Error, Result};

pub const MAX_QUERY_CHARS: usize = 200;
pub const MAX_PAGE: u32 = 50;
pub const DEFAULT_PAGE: u32 = 20;
const MAX_OFFSET: u32 = 10_000;
/// Characters of lesson text per call; `next_start` continues.
pub const SECTION_CHARS: usize = 60_000;
/// Largest lesson body read from a content blob.
pub const MAX_CONTENT_BYTES: usize = 512_000;
const MAX_OUTLINE_ROWS: i64 = 1_000;
const MAX_QUESTIONS: usize = 100;
const MAX_QUESTION_BYTES: usize = 400_000;
const MAX_FIELD_CHARS: usize = 4_000;
const MAX_OPTION_CHARS: usize = 1_000;
const MAX_OPTIONS: usize = 20;

/// Courses a learning grant may see, for a query aliasing `courses` as `c`.
const VISIBLE: &str = "c.status = 'published' AND NOT EXISTS(SELECT 1 FROM local_identity i WHERE i.id = 1 AND i.stake_address = c.author_address)";

pub struct CatalogQuery<'a> {
    pub query: &'a str,
    pub skill_id: &'a str,
    pub stored_only: bool,
    pub limit: u32,
    pub cursor: &'a str,
}

pub fn search_catalog(conn: &Connection, q: &CatalogQuery<'_>) -> Result<Value> {
    let query = q.query.trim();
    if query.chars().count() > MAX_QUERY_CHARS || q.skill_id.len() > 200 {
        return Err(Error::Invalid("search terms are too long".into()));
    }
    let limit = if q.limit == 0 {
        DEFAULT_PAGE
    } else {
        q.limit.min(MAX_PAGE)
    };
    let offset: u32 = if q.cursor.is_empty() {
        0
    } else {
        q.cursor
            .parse()
            .ok()
            .filter(|offset| *offset <= MAX_OFFSET)
            .ok_or_else(|| Error::Invalid("unknown cursor".into()))?
    };
    let text = format!("%{}%", escape_like(query));
    let skill = format!("%{}%", escape_like(&serde_json::to_string(q.skill_id)?));
    let mut stmt = conn.prepare(&format!(
        "SELECT k.course_id, k.title, k.description, k.author_address, k.kind, k.tags, k.skill_ids,
                k.published_at, k.version,
                EXISTS(SELECT 1 FROM courses c WHERE c.id = k.course_id AND {VISIBLE}),
                EXISTS(SELECT 1 FROM enrollments e WHERE e.course_id = k.course_id AND e.status IN ('active','completed'))
         FROM catalog k
         WHERE (?1 = '' OR k.title LIKE ?2 ESCAPE '\\' OR k.description LIKE ?2 ESCAPE '\\' OR k.tags LIKE ?2 ESCAPE '\\')
           AND (?3 = '' OR k.skill_ids LIKE ?4 ESCAPE '\\')
           AND (?5 = 0 OR EXISTS(SELECT 1 FROM courses c WHERE c.id = k.course_id AND {VISIBLE}))
         ORDER BY k.published_at DESC, k.course_id
         LIMIT ?6 OFFSET ?7"
    ))?;
    let rows = stmt.query_map(
        params![
            query,
            text,
            q.skill_id,
            skill,
            q.stored_only,
            limit + 1,
            offset
        ],
        |r| {
            Ok(json!({
                "course_id": r.get::<_, String>(0)?,
                "title": r.get::<_, String>(1)?,
                "description": r.get::<_, Option<String>>(2)?,
                "author_address": r.get::<_, String>(3)?,
                "kind": r.get::<_, Option<String>>(4)?.unwrap_or_else(|| "course".into()),
                "tags": json_strings(r.get(5)?),
                "skill_ids": json_strings(r.get(6)?),
                "published_at": r.get::<_, String>(7)?,
                "version": r.get::<_, i64>(8)?,
                "stored_on_device": r.get::<_, bool>(9)?,
                "enrolled": r.get::<_, bool>(10)?,
            }))
        },
    )?;
    let mut items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    let next_cursor = (items.len() > limit as usize && offset + limit <= MAX_OFFSET)
        .then(|| (offset + limit).to_string());
    items.truncate(limit as usize);
    Ok(json!({"items": items, "next_cursor": next_cursor, "source": "local_catalog"}))
}

/// What `read_lesson` returns for an element type.
pub fn content_kind(element_type: &str) -> &'static str {
    match element_type {
        "text" => "text",
        "video" => "video_chapters",
        "quiz" | "objective_single_mcq" | "objective_multi_mcq" | "subjective_mcq" | "essay" => {
            "questions"
        }
        _ => "withheld",
    }
}

pub fn get_course(conn: &Connection, course_id: &str) -> Result<Value> {
    bounded_id(course_id)?;
    let enrolled: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM enrollments WHERE course_id = ?1 AND status IN ('active','completed'))",
        [course_id],
        |r| r.get(0),
    )?;
    let stored = conn
        .query_row(
            &format!(
                "SELECT c.title, c.description, c.author_address, c.author_name, c.kind, c.tags,
                        c.skill_ids, c.version, c.published_at
                 FROM courses c WHERE c.id = ?1 AND {VISIBLE}"
            ),
            [course_id],
            |r| {
                Ok(json!({
                    "course_id": course_id,
                    "title": r.get::<_, String>(0)?,
                    "description": r.get::<_, Option<String>>(1)?,
                    "author_address": r.get::<_, String>(2)?,
                    "author_name": r.get::<_, Option<String>>(3)?,
                    "kind": r.get::<_, Option<String>>(4)?.unwrap_or_else(|| "course".into()),
                    "tags": json_strings(r.get(5)?),
                    "skill_ids": json_strings(r.get(6)?),
                    "version": r.get::<_, i64>(7)?,
                    "published_at": r.get::<_, Option<String>>(8)?,
                    "stored_on_device": true,
                    "enrolled": enrolled,
                    "chapters": [],
                    "truncated": false,
                }))
            },
        )
        .optional()?;
    let Some(mut course) = stored else {
        // Announced but not stored here: the public summary, without chapters.
        return conn
            .query_row(
                "SELECT title, description, author_address, kind, tags, skill_ids, version, published_at
                 FROM catalog WHERE course_id = ?1",
                [course_id],
                |r| {
                    Ok(json!({
                        "course_id": course_id,
                        "title": r.get::<_, String>(0)?,
                        "description": r.get::<_, Option<String>>(1)?,
                        "author_address": r.get::<_, String>(2)?,
                        "author_name": null,
                        "kind": r.get::<_, Option<String>>(3)?.unwrap_or_else(|| "course".into()),
                        "tags": json_strings(r.get(4)?),
                        "skill_ids": json_strings(r.get(5)?),
                        "version": r.get::<_, i64>(6)?,
                        "published_at": r.get::<_, Option<String>>(7)?,
                        "stored_on_device": false,
                        "enrolled": enrolled,
                        "chapters": [],
                        "truncated": false,
                    }))
                },
            )
            .optional()?
            .ok_or(Error::NotFound);
    };

    let mut stmt = conn.prepare(
        "SELECT ch.id, ch.title, e.id, e.title, e.element_type, e.duration_seconds
         FROM course_chapters ch LEFT JOIN course_elements e ON e.chapter_id = ch.id
         WHERE ch.course_id = ?1
         ORDER BY ch.position, ch.id, e.position, e.id
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![course_id, MAX_OUTLINE_ROWS + 1], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<i64>>(5)?,
        ))
    })?;
    let mut chapters: Vec<Value> = Vec::new();
    for (count, row) in rows.enumerate() {
        if count as i64 == MAX_OUTLINE_ROWS {
            course["truncated"] = true.into();
            break;
        }
        let (chapter_id, chapter_title, element_id, element_title, element_type, duration) = row?;
        if chapters.last().and_then(|c| c["chapter_id"].as_str()) != Some(chapter_id.as_str()) {
            chapters
                .push(json!({"chapter_id": chapter_id, "title": chapter_title, "elements": []}));
        }
        if let (Some(element_id), Some(title), Some(element_type)) =
            (element_id, element_title, element_type)
        {
            if let Some(elements) = chapters
                .last_mut()
                .and_then(|chapter| chapter["elements"].as_array_mut())
            {
                elements.push(json!({
                    "element_id": element_id,
                    "title": title,
                    "content": content_kind(&element_type),
                    "element_type": element_type,
                    "duration_seconds": duration,
                }));
            }
        }
    }
    course["chapters"] = Value::Array(chapters);
    Ok(course)
}

pub struct LessonTarget {
    pub course_id: String,
    pub element_id: String,
    pub title: String,
    pub element_type: String,
    inline: Option<String>,
    blob: Option<String>,
    duration_seconds: Option<i64>,
}

impl LessonTarget {
    /// The content blob to fetch before rendering, when the body is not inline.
    pub fn blob(&self) -> Option<&str> {
        match content_kind(&self.element_type) {
            "text" | "questions" if self.inline.is_none() => self.blob.as_deref(),
            _ => None,
        }
    }
}

pub fn lesson_target(conn: &Connection, course_id: &str, element_id: &str) -> Result<LessonTarget> {
    bounded_id(course_id)?;
    bounded_id(element_id)?;
    conn.query_row(
        &format!(
            "SELECT e.title, e.element_type, e.content_inline, e.content_cid, e.duration_seconds
             FROM course_elements e
             JOIN course_chapters ch ON ch.id = e.chapter_id
             JOIN courses c ON c.id = ch.course_id
             WHERE e.id = ?1 AND ch.course_id = ?2 AND {VISIBLE}"
        ),
        params![element_id, course_id],
        |r| {
            Ok(LessonTarget {
                course_id: course_id.to_string(),
                element_id: element_id.to_string(),
                title: r.get(0)?,
                element_type: r.get(1)?,
                inline: r.get(2)?,
                blob: r.get(3)?,
                duration_seconds: r.get(4)?,
            })
        },
    )
    .optional()?
    .ok_or(Error::NotFound)
}

/// Where a lesson's body comes from.
#[derive(Clone, Copy)]
pub enum Body<'a> {
    /// Inline in the profile database, or not needed for this element type.
    Stored,
    /// Fetched from the content store; the error reason is not shown.
    Fetched(std::result::Result<&'a [u8], &'a str>),
}

pub fn render_lesson(
    conn: &Connection,
    target: &LessonTarget,
    body: Body<'_>,
    start: usize,
) -> Result<Value> {
    let mut lesson = json!({
        "course_id": target.course_id,
        "element_id": target.element_id,
        "title": target.title,
        "element_type": target.element_type,
        "status": "returned",
        "reason": null,
        "text": null,
        "next_start": null,
        "instructions": null,
        "questions": [],
        "video_chapters": [],
        "duration_seconds": target.duration_seconds,
        "truncated": false,
    });
    match content_kind(&target.element_type) {
        "video_chapters" => {
            let mut stmt = conn.prepare(
                "SELECT title, start_seconds FROM video_chapters WHERE element_id = ?1 ORDER BY position LIMIT 200",
            )?;
            let chapters = stmt
                .query_map([&target.element_id], |r| {
                    Ok(json!({"title": r.get::<_, String>(0)?, "start_seconds": r.get::<_, i64>(1)?}))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            lesson["video_chapters"] = Value::Array(chapters);
        }
        "withheld" => withhold(&mut lesson, withheld_reason(&target.element_type)),
        kind => {
            let content = match (&target.inline, body) {
                (Some(inline), _) => Ok(inline.clone()),
                (None, Body::Fetched(Ok(bytes))) if bytes.len() > MAX_CONTENT_BYTES => {
                    Err("the lesson is too large to share")
                }
                (None, Body::Fetched(Ok(bytes))) => {
                    String::from_utf8(bytes.to_vec()).map_err(|_| "the lesson content is not text")
                }
                (None, Body::Fetched(Err(_))) => {
                    Err("the lesson is not stored on this device and no peer provided it in time")
                }
                (None, Body::Stored) => Err("this lesson has no content"),
            };
            match content {
                Err(reason) => {
                    lesson["status"] = "unavailable".into();
                    lesson["reason"] = reason.into();
                }
                Ok(content) if kind == "text" => {
                    let (text, next_start) = section(&content, start);
                    lesson["text"] = text.into();
                    lesson["next_start"] = json!(next_start);
                }
                Ok(content) => match serde_json::from_str::<Value>(&content)
                    .ok()
                    .and_then(|value| questions(&target.element_type, &value))
                {
                    Some(parsed) => {
                        lesson["instructions"] = json!(parsed.instructions);
                        lesson["questions"] = Value::Array(parsed.questions);
                        lesson["truncated"] = parsed.truncated.into();
                    }
                    None => withhold(
                        &mut lesson,
                        "the question format is not recognised, so nothing is shared".into(),
                    ),
                },
            }
        }
    }
    Ok(lesson)
}

fn withhold(lesson: &mut Value, reason: String) {
    lesson["status"] = "withheld".into();
    lesson["reason"] = reason.into();
}

fn withheld_reason(element_type: &str) -> String {
    match element_type {
        "assessment" => "credential-bearing assessments are not shared with assistants",
        "interactive" | "plugin" => "interactive content runs only inside Alexandria",
        _ => "this kind of element has no content an assistant can read",
    }
    .into()
}

/// Up to `SECTION_CHARS` characters from character `start`, and where the
/// next section starts.
fn section(text: &str, start: usize) -> (String, Option<usize>) {
    let Some((from, _)) = text.char_indices().nth(start) else {
        return (String::new(), None);
    };
    match text[from..].char_indices().nth(SECTION_CHARS) {
        Some((to, _)) => (
            text[from..from + to].to_string(),
            Some(start + SECTION_CHARS),
        ),
        None => (text[from..].to_string(), None),
    }
}

struct Questions {
    instructions: Option<String>,
    questions: Vec<Value>,
    truncated: bool,
}

/// Rebuilds an element's questions from the fields a learner sees before
/// answering. Fields are copied by name; nothing else is.
fn questions(element_type: &str, content: &Value) -> Option<Questions> {
    match element_type {
        "essay" => {
            let prompt = clipped(content, "question").or_else(|| clipped(content, "prompt"))?;
            let mut question = question(None, "essay", prompt, &Value::Null);
            question["guidelines"] = json!(clipped(content, "guidelines"));
            question["min_words"] = json!(content.get("min_words").and_then(Value::as_i64));
            question["max_words"] = json!(content.get("max_words").and_then(Value::as_i64));
            question["rubric_criteria"] = json!(strings(content.get("rubric_criteria")));
            Some(Questions {
                instructions: None,
                questions: vec![question],
                truncated: false,
            })
        }
        "quiz" => items(
            content.get("questions")?.as_array()?,
            clipped(content, "description"),
            |item| {
                let kind = item
                    .get("type")
                    .and_then(Value::as_str)
                    .filter(|kind| {
                        [
                            "single_choice",
                            "multiple_choice",
                            "true_false",
                            "short_answer",
                        ]
                        .contains(kind)
                    })
                    .unwrap_or("choice");
                Some(kind)
            },
        ),
        "objective_single_mcq" | "objective_multi_mcq" | "subjective_mcq" => {
            match content.get("questions").and_then(Value::as_array) {
                Some(list) => items(list, None, |_| Some(element_type)),
                None => {
                    let prompt = clipped(content, "question")?;
                    Some(Questions {
                        instructions: None,
                        questions: vec![question(None, element_type, prompt, content)],
                        truncated: false,
                    })
                }
            }
        }
        _ => None,
    }
}

fn items<'a>(
    list: &'a [Value],
    instructions: Option<String>,
    kind: impl Fn(&'a Value) -> Option<&'a str>,
) -> Option<Questions> {
    let mut questions = Vec::new();
    let mut bytes = 0;
    let mut truncated = list.len() > MAX_QUESTIONS;
    for item in list.iter().take(MAX_QUESTIONS) {
        let Some(prompt) = clipped(item, "prompt").or_else(|| clipped(item, "question")) else {
            continue;
        };
        let id = item.get("id").and_then(Value::as_str).map(clip_field);
        let question = question(id, kind(item)?, prompt, item);
        bytes += question.to_string().len();
        if bytes > MAX_QUESTION_BYTES {
            truncated = true;
            break;
        }
        questions.push(question);
    }
    Some(Questions {
        instructions,
        questions,
        truncated,
    })
}

fn question(id: Option<String>, question_type: &str, prompt: String, item: &Value) -> Value {
    let options = item
        .get("options")
        .and_then(Value::as_array)
        .map(|options| {
            options
                .iter()
                .take(MAX_OPTIONS)
                .filter_map(|option| match option {
                    Value::String(text) => Some(text.as_str()),
                    Value::Object(_) => option.get("text").and_then(Value::as_str),
                    _ => None,
                })
                .map(|text| text.chars().take(MAX_OPTION_CHARS).collect::<String>())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "id": id,
        "question_type": question_type,
        "prompt": prompt,
        "context": clipped(item, "context"),
        "options": options,
        "points": item.get("points").and_then(Value::as_f64),
        "guidelines": null,
        "min_words": null,
        "max_words": null,
        "rubric_criteria": [],
    })
}

fn clipped(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(clip_field)
}

fn clip_field(text: &str) -> String {
    text.chars().take(MAX_FIELD_CHARS).collect()
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(MAX_OPTIONS)
                .filter_map(Value::as_str)
                .map(clip_field)
                .collect()
        })
        .unwrap_or_default()
}

fn json_strings(json: Option<String>) -> Vec<String> {
    json.and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
        .unwrap_or_default()
}

fn escape_like(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn bounded_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 200 {
        return Err(Error::Invalid("invalid identifier".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaks(value: &Questions) -> String {
        json!(value.questions).to_string()
    }

    #[test]
    fn quiz_questions_keep_prompts_and_options_but_never_answers() {
        let quiz = json!({"title": "Q", "description": "Read chapter 2 first", "pass_threshold": 0.7,
        "questions": [
            {"id": "q1", "type": "single_choice", "prompt": "Pick", "options": ["a", "b"],
             "correct_indices": [1], "explanation": "because b", "points": 2, "difficulty": 1},
            {"id": "q2", "type": "short_answer", "prompt": "Name it", "correct_answer": "graph", "points": 1},
            {"id": "q3", "type": "novel_type", "prompt": "New", "options": ["x"], "answer_key": "x"},
            {"id": "q4", "explanation": "no prompt, skipped"}
        ]});
        let parsed = questions("quiz", &quiz).unwrap();
        assert_eq!(parsed.instructions.as_deref(), Some("Read chapter 2 first"));
        assert_eq!(parsed.questions.len(), 3);
        assert_eq!(parsed.questions[0]["options"], json!(["a", "b"]));
        assert_eq!(parsed.questions[0]["points"], json!(2.0));
        assert_eq!(parsed.questions[2]["question_type"], "choice");
        let text = leaks(&parsed);
        for secret in [
            "correct",
            "explanation",
            "because b",
            "graph",
            "answer_key",
            "difficulty",
            "pass_threshold",
        ] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
    }

    #[test]
    fn multiple_choice_and_essays_share_only_what_learners_see() {
        let mcq = json!({"question": "Which?", "context": "Given a list", "options": [{"id": "o1", "text": "x", "is_correct": true}, {"id": "o2", "text": "y"}],
            "correct_option_index": 0, "correct_option_indices": [0], "explanation": "x wins"});
        let parsed = questions("objective_multi_mcq", &mcq).unwrap();
        assert_eq!(parsed.questions[0]["options"], json!(["x", "y"]));
        assert_eq!(parsed.questions[0]["context"], "Given a list");
        let authored = json!({"questions": [{"id": "a", "question": "Q?", "options": ["1", "2"], "correct_index": 1}]});
        let parsed_authored = questions("objective_single_mcq", &authored).unwrap();
        assert_eq!(parsed_authored.questions[0]["prompt"], "Q?");

        let essay = json!({"question": "Argue", "guidelines": "Cite sources", "min_words": 300, "max_words": 800,
            "rubric_criteria": ["Clarity"], "rubric": "PRIVATE GRADING RUBRIC"});
        let parsed_essay = questions("essay", &essay).unwrap();
        assert_eq!(parsed_essay.questions[0]["min_words"], 300);
        assert_eq!(
            parsed_essay.questions[0]["rubric_criteria"],
            json!(["Clarity"])
        );

        for text in [
            leaks(&parsed),
            leaks(&parsed_authored),
            leaks(&parsed_essay),
        ] {
            for secret in ["correct", "is_correct", "x wins", "PRIVATE"] {
                assert!(!text.contains(secret), "{secret} leaked: {text}");
            }
        }
    }

    #[test]
    fn unrecognised_formats_and_withheld_types_share_nothing() {
        assert!(questions("quiz", &json!({"items": []})).is_none());
        assert!(questions("objective_single_mcq", &json!({"prompt_text": "?"})).is_none());
        assert!(questions("assessment", &json!({"questions": []})).is_none());
        assert_eq!(content_kind("assessment"), "withheld");
        assert_eq!(content_kind("interactive"), "withheld");
        assert_eq!(content_kind("plugin"), "withheld");
    }

    #[test]
    fn text_sections_split_on_characters() {
        let text = "é".repeat(SECTION_CHARS + 5);
        let (first, next) = section(&text, 0);
        assert_eq!(first.chars().count(), SECTION_CHARS);
        assert_eq!(next, Some(SECTION_CHARS));
        let (rest, next) = section(&text, SECTION_CHARS);
        assert_eq!(rest.chars().count(), 5);
        assert_eq!(next, None);
        assert_eq!(section(&text, SECTION_CHARS + 5), (String::new(), None));
    }

    #[test]
    fn like_wildcards_in_queries_are_literal() {
        assert_eq!(escape_like("100%_a\\"), "100\\%\\_a\\\\");
    }
}
