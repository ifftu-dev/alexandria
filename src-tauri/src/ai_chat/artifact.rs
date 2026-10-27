//! Chat actions: changes a model asks to make to the learner's data.
//!
//! A model may end its reply with one fenced `artifact` block naming an
//! action from a fixed list: save a note or flashcards, add a goal, change an
//! attached goal, add a note to the attached lesson, or enroll in a course on
//! an attached goal's path. The block is parsed here into a [`Draft`]; the
//! app then resolves it against the learner's own data and stores it as a
//! proposal. A model never supplies ids: goals, skills, lessons, and courses
//! are matched by name to what the learner already has. Nothing is written
//! until the learner allows the proposal (see [`super::action`]). The
//! learner can also save any answer as a note.
//!
//! Saved artifacts keep the sources the originating answer carried, and they
//! outlive the chat they came from. Their text is checked against stored
//! assessment items like every other model output.

use serde::{Deserialize, Serialize};

pub const MAX_TITLE_CHARS: usize = 120;
pub const MAX_NOTE_CHARS: usize = 8_000;
pub const MAX_CARDS: usize = 20;
pub const MAX_CARD_CHARS: usize = 400;
pub const MAX_GOAL_CHARS: usize = 500;
/// Most skills a goal change adds or removes.
pub const MAX_GOAL_CHANGES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub front: String,
    pub back: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    Note {
        text: String,
    },
    Flashcards {
        cards: Vec<Card>,
    },
    Goal {
        text: String,
        /// The learner's goal, once they added it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        goal_id: Option<String>,
    },
    /// A change to one of the learner's goals. Skills are resolved by the
    /// app from names; the model never supplies ids.
    GoalChange {
        goal_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rename: Option<String>,
        add: Vec<SkillRef>,
        remove: Vec<SkillRef>,
        text: String,
    },
    /// A note for one lesson's course notes. The lesson is the one attached
    /// to the chat; the title of the proposal is the note's title.
    CourseNote {
        course_id: String,
        element_id: String,
        /// The lesson's title, as shown to the learner.
        lesson: String,
        text: String,
    },
    /// Enrolling in a course recommended on an attached goal's path. The
    /// title of the proposal is the course's title.
    Enroll {
        course_id: String,
        goal_id: String,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRef {
    pub id: String,
    pub name: String,
}

/// A goal change as the model wrote it, before the app resolves the goal and
/// skill names against the learner's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalChangeDraft {
    pub goal: String,
    pub rename: Option<String>,
    pub add: Vec<String>,
    pub remove: Vec<String>,
    pub text: String,
}

/// An action as the model wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Draft {
    /// Needs nothing from the learner's data: a note, flashcards, or a new
    /// goal.
    Ready(Proposal),
    GoalChange(GoalChangeDraft),
    /// A note for the lesson attached to the chat.
    CourseNote {
        title: String,
        text: String,
    },
    /// A course to enroll in, named by its title.
    Enroll {
        course: String,
        text: String,
    },
}

impl Body {
    pub fn kind(&self) -> &'static str {
        match self {
            Body::Note { .. } => "note",
            Body::Flashcards { .. } => "flashcards",
            Body::Goal { .. } => "goal",
            Body::GoalChange { .. } => "goal_change",
            Body::CourseNote { .. } => "course_note",
            Body::Enroll { .. } => "enroll",
        }
    }

    /// Everything a learner would read, for the assessment-item check.
    pub fn text(&self) -> String {
        match self {
            Body::Note { text }
            | Body::Goal { text, .. }
            | Body::CourseNote { text, .. }
            | Body::Enroll { text, .. } => text.clone(),
            Body::GoalChange {
                rename,
                add,
                remove,
                text,
                ..
            } => {
                let names = add.iter().chain(remove).map(|s| s.name.as_str());
                let mut parts: Vec<&str> = rename.iter().map(String::as_str).collect();
                parts.extend(names);
                parts.push(text);
                parts.join("\n")
            }
            Body::Flashcards { cards } => cards
                .iter()
                .map(|c| format!("{} {}", c.front, c.back))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub title: String,
    pub body: Body,
}

#[derive(Deserialize)]
struct RawCard {
    front: String,
    back: String,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(rename = "type")]
    kind: String,
    title: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    cards: Option<Vec<RawCard>>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    add: Vec<String>,
    #[serde(default)]
    remove: Vec<String>,
}

/// A single-line title, trimmed and bounded.
pub fn clean_title(title: &str) -> Option<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    (!title.is_empty() && title.chars().count() <= MAX_TITLE_CHARS).then_some(title)
}

/// Note text keeps its line breaks; it is trimmed and bounded.
pub fn clean_note(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty() && text.chars().count() <= MAX_NOTE_CHARS).then(|| text.to_string())
}

fn clean_side(text: &str) -> Option<String> {
    clean_side_bounded(text, MAX_CARD_CHARS)
}

fn clean_side_bounded(text: &str, max: usize) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty() && text.chars().count() <= max).then_some(text)
}

/// Parses an `artifact` block. Unknown types, missing fields, and anything
/// over its bounds are dropped.
pub fn parse(block: &str) -> Option<Draft> {
    let raw: Raw = serde_json::from_str(block).ok()?;
    let title = clean_title(&raw.title)?;
    let body = match raw.kind.trim() {
        "note" => Body::Note {
            text: clean_note(raw.text.as_deref()?)?,
        },
        "flashcards" => {
            let cards = raw.cards?;
            if cards.is_empty() || cards.len() > MAX_CARDS {
                return None;
            }
            Body::Flashcards {
                cards: cards
                    .iter()
                    .map(|c| {
                        Some(Card {
                            front: clean_side(&c.front)?,
                            back: clean_side(&c.back)?,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
            }
        }
        "goal" => Body::Goal {
            text: clean_side_bounded(raw.text.as_deref()?, MAX_GOAL_CHARS)?,
            goal_id: None,
        },
        "goal_change" => {
            let rename = match raw.name.as_deref() {
                Some(name) if !name.trim().is_empty() => Some(clean_title(name)?),
                _ => None,
            };
            return Some(Draft::GoalChange(GoalChangeDraft {
                goal: title,
                rename,
                add: clean_names(&raw.add)?,
                remove: clean_names(&raw.remove)?,
                text: clean_side_bounded(raw.text.as_deref()?, MAX_GOAL_CHARS)?,
            }));
        }
        "course_note" => {
            return Some(Draft::CourseNote {
                title,
                text: clean_note(raw.text.as_deref()?)?,
            });
        }
        "enroll" => {
            return Some(Draft::Enroll {
                course: title,
                text: clean_side_bounded(raw.text.as_deref()?, MAX_GOAL_CHARS)?,
            });
        }
        _ => return None,
    };
    Some(Draft::Ready(Proposal { title, body }))
}

fn clean_names(names: &[String]) -> Option<Vec<String>> {
    if names.len() > MAX_GOAL_CHANGES {
        return None;
    }
    names.iter().map(|n| clean_title(n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(block: &str) -> Proposal {
        match parse(block) {
            Some(Draft::Ready(p)) => p,
            other => panic!("{block}: {other:?}"),
        }
    }

    #[test]
    fn notes_and_flashcards_parse_and_are_bounded() {
        let note = ready(r#"{"type":"note","title":"  Bayes\nrule ","text":"P(A|B)\n= ..."}"#);
        assert_eq!(note.title, "Bayes rule");
        assert_eq!(
            note.body,
            Body::Note {
                text: "P(A|B)\n= ...".into()
            }
        );
        let cards = ready(
            r#"{"type":"flashcards","title":"Terms","cards":[{"front":"Prior","back":"Belief before evidence"}]}"#,
        );
        assert_eq!(cards.body.kind(), "flashcards");
        assert!(cards.body.text().contains("Belief before evidence"));

        let many = format!(
            r#"{{"type":"flashcards","title":"T","cards":[{}]}}"#,
            vec![r#"{"front":"a","back":"b"}"#; MAX_CARDS + 1].join(",")
        );
        let goal = ready(
            r#"{"type":"goal","title":"Data analyst","text":"Clean data\nand build dashboards","goal_id":"forged"}"#,
        );
        assert_eq!(
            goal.body,
            Body::Goal {
                text: "Clean data and build dashboards".into(),
                goal_id: None,
            },
            "a model cannot name an existing goal"
        );

        let long_goal = format!(
            r#"{{"type":"goal","title":"T","text":"{}"}}"#,
            "a".repeat(MAX_GOAL_CHARS + 1)
        );
        for bad in [
            many.as_str(),
            long_goal.as_str(),
            r#"{"type":"goal","title":"T"}"#,
            r#"{"type":"flashcards","title":"T","cards":[]}"#,
            r#"{"type":"flashcards","title":"T","cards":[{"front":"","back":"b"}]}"#,
            r#"{"type":"note","title":"","text":"x"}"#,
            r#"{"type":"note","title":"T"}"#,
            r#"{"type":"grade","title":"T","text":"A+"}"#,
            r#"{"type":"credential","title":"T","text":"issue one"}"#,
            "not json",
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn actions_that_need_the_learners_data_stay_drafts() {
        let Some(Draft::GoalChange(change)) = parse(
            r#"{"type":"goal_change","title":"Data analyst","name":"","add":["SQL"],"remove":["Bayes rule"],"text":"Focus on querying."}"#,
        ) else {
            panic!("goal change")
        };
        assert_eq!(change.goal, "Data analyst");
        assert_eq!(change.rename, None);
        assert_eq!(
            (change.add, change.remove),
            (vec!["SQL".to_string()], vec!["Bayes rule".to_string()])
        );
        let too_many = format!(
            r#"{{"type":"goal_change","title":"T","text":"x","add":[{}]}}"#,
            vec![r#""s""#; MAX_GOAL_CHANGES + 1].join(",")
        );
        assert!(parse(&too_many).is_none());

        assert_eq!(
            parse(
                r#"{"type":"course_note","title":"Bayes","text":"Update a prior.","course_id":"forged"}"#
            ),
            Some(Draft::CourseNote {
                title: "Bayes".into(),
                text: "Update a prior.".into()
            }),
            "the lesson comes from the chat, not the model"
        );
        assert_eq!(
            parse(
                r#"{"type":"enroll","title":" Intro to\nSQL ","text":"Covers joins.","course_id":"forged"}"#
            ),
            Some(Draft::Enroll {
                course: "Intro to SQL".into(),
                text: "Covers joins.".into()
            }),
            "a course is named by its title"
        );
        assert!(parse(r#"{"type":"enroll","title":"T"}"#).is_none());
        assert!(parse(r#"{"type":"course_note","title":"T","text":""}"#).is_none());
    }
}
