//! A learner goal as chat context.
//!
//! Goals live in the `learner.targets` setting. When the learner attaches one,
//! each prepare re-reads the goal and recomputes its path from the same
//! service the Goals page uses, so the text reflects proven skills as they
//! are now, not as a model remembers them. Progress comes from the learner's
//! credentials; nothing a model says changes it.
//!
//! The rendering names skills and courses only. It carries no assessment
//! items, answer keys, or attempt rows.
//!
//! A model may also propose a new goal, a change to an attached goal, or
//! enrolling in a course on an attached goal's path. It names skills and
//! courses; [`resolve_change`] and [`resolve_enroll`] turn names into the
//! learner's own ids. When the learner allows a proposal, [`add`] and
//! [`apply_change`] write the goal through the settings store, checking
//! every skill again.

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};

use super::artifact::{Body, GoalChangeDraft, Proposal, SkillRef};
use crate::settings::{registry::keys::LEARNER_TARGETS, registry::JsonSetting, SettingsStore};

/// Most target skills on a goal added or changed from chat.
pub const MAX_GOAL_SKILLS: usize = 50;

/// Most skills listed per status, so a broad goal stays within budget.
const MAX_LISTED: usize = 25;
/// Most course suggestions per available skill.
const MAX_COURSES: usize = 3;

#[derive(Deserialize)]
struct StoredGoal {
    id: String,
    label: String,
    #[serde(default)]
    goal_skill_ids: Vec<String>,
}

/// A saved goal's label, or `None` when the learner has no such goal.
pub fn label(conn: &Connection, goal_id: &str) -> Result<Option<String>, String> {
    Ok(find(conn, goal_id)?.map(|g| g.label))
}

/// Every saved goal as stored, so a write keeps fields this module does not
/// know about.
fn stored(conn: &Connection) -> Result<Vec<Value>, String> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'learner.targets'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| "storage_error: goals could not be read".to_string())?;
    Ok(raw
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default())
}

fn save(conn: &Connection, goals: Vec<Value>) -> Result<(), String> {
    SettingsStore::set(conn, LEARNER_TARGETS, JsonSetting(Value::Array(goals)))
        .map_err(|_| "storage_error: goals could not be saved".to_string())
}

fn find(conn: &Connection, goal_id: &str) -> Result<Option<StoredGoal>, String> {
    Ok(stored(conn)?
        .into_iter()
        .filter_map(|g| serde_json::from_value::<StoredGoal>(g).ok())
        .find(|g| g.id == goal_id))
}

/// Skill ids, de-duplicated in order, each checked to exist.
fn known_skills(conn: &Connection, ids: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        if out.contains(id) {
            continue;
        }
        if skill_name(conn, id)?.is_none() {
            return Err("invalid_input: a skill on this goal no longer exists".into());
        }
        out.push(id.clone());
    }
    if out.is_empty() || out.len() > MAX_GOAL_SKILLS {
        return Err(format!(
            "invalid_input: a goal needs between 1 and {MAX_GOAL_SKILLS} skills"
        ));
    }
    Ok(out)
}

/// Adds a goal the learner allowed, with the skills they picked. A goal
/// with the same skills already saved is returned instead of a copy.
pub fn add(
    conn: &Connection,
    label: &str,
    skill_ids: &[String],
    now: &str,
) -> Result<String, String> {
    let label = super::artifact::clean_title(label)
        .ok_or_else(|| "invalid_input: a goal needs a short name".to_string())?;
    let ids = known_skills(conn, skill_ids)?;
    let mut goals = stored(conn)?;
    let mut wanted = ids.clone();
    wanted.sort();
    for goal in goals
        .iter()
        .filter_map(|g| serde_json::from_value::<StoredGoal>(g.clone()).ok())
    {
        let mut have = goal.goal_skill_ids.clone();
        have.sort();
        have.dedup();
        if have == wanted {
            return Ok(goal.id);
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    goals.push(json!({
        "id": id,
        "label": label,
        "source_did": null,
        "goal_skill_ids": ids,
        "created_at": now,
        "kind": "learning_goal",
        "resolution_provenance": "goal_parsed",
    }));
    save(conn, goals)?;
    Ok(id)
}

/// Applies a goal change the learner allowed to the goal as it is now. A
/// skill already added or removed is skipped; the goal is never left empty.
pub fn apply_change(conn: &Connection, change: &Body) -> Result<(), String> {
    let Body::GoalChange {
        goal_id,
        rename,
        add,
        remove,
        ..
    } = change
    else {
        return Err("invalid_input: not a goal change".into());
    };
    let mut goals = stored(conn)?;
    let Some(goal) = goals
        .iter_mut()
        .find(|g| g.get("id").and_then(Value::as_str) == Some(goal_id.as_str()))
    else {
        return Err("not_found: that goal is not in your goals".into());
    };
    let current: Vec<String> = goal
        .get("goal_skill_ids")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let mut next: Vec<String> = current
        .into_iter()
        .filter(|id| !remove.iter().any(|s| &s.id == id))
        .collect();
    next.extend(add.iter().map(|s| s.id.clone()));
    let next = known_skills(conn, &next)?;
    goal["goal_skill_ids"] = json!(next);
    if let Some(name) = rename {
        goal["label"] = json!(name);
    }
    save(conn, goals)
}

/// The goal's label and its current path as text, or `None` when the goal
/// was removed.
pub fn render(conn: &Connection, goal_id: &str) -> Result<Option<(String, String)>, String> {
    let Some(goal) = find(conn, goal_id)? else {
        return Ok(None);
    };
    let path = alexandria_studio::skills::my_learning_path(conn, &goal.goal_skill_ids)
        .map_err(|_| "storage_error: the goal's path could not be computed".to_string())?;
    let name = |id: &str| {
        path.steps
            .iter()
            .find(|s| s.skill_id == id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let mut out = format!(
        "Goal: {}\nProgress: {} of {} skills on the path are proven by the learner's credentials.\n",
        goal.label, path.earned_count, path.total
    );
    let mut section =
        |title: &str,
         status: &str,
         detail: &dyn Fn(&alexandria_studio::skills::PathStep) -> String| {
            let steps: Vec<_> = path.steps.iter().filter(|s| s.status == status).collect();
            if steps.is_empty() {
                return;
            }
            out.push_str(&format!("\n{title}:\n"));
            for step in steps.iter().take(MAX_LISTED) {
                let target = if step.is_goal { " (goal skill)" } else { "" };
                let subject = step
                    .subject_name
                    .as_deref()
                    .map(|s| format!(", {s}"))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "- {} [{}{}]{}{}\n",
                    step.name,
                    step.bloom_level,
                    subject,
                    target,
                    detail(step)
                ));
            }
            if steps.len() > MAX_LISTED {
                out.push_str(&format!("- and {} more\n", steps.len() - MAX_LISTED));
            }
        };
    section(
        "Available next (prerequisites proven)",
        "available",
        &|step| {
            let courses: Vec<&str> = step
                .course_recs
                .iter()
                .take(MAX_COURSES)
                .map(|c| c.title.as_str())
                .collect();
            if courses.is_empty() {
                String::new()
            } else {
                format!("; courses: {}", courses.join("; "))
            }
        },
    );
    section("Locked (needs prerequisites first)", "locked", &|step| {
        let needs: Vec<String> = step
            .prerequisite_ids
            .iter()
            .take(MAX_COURSES)
            .map(|id| name(id))
            .collect();
        if needs.is_empty() {
            String::new()
        } else {
            format!("; needs: {}", needs.join(", "))
        }
    });
    section("Proven", "earned", &|_| String::new());
    Ok(Some((goal.label, out)))
}

fn skill_name(conn: &Connection, id: &str) -> Result<Option<String>, String> {
    conn.query_row("SELECT name FROM skills WHERE id = ?1", [id], |r| r.get(0))
        .optional()
        .map_err(|_| "storage_error: skills could not be read".to_string())
}

fn skill_named(conn: &Connection, name: &str) -> Result<Option<SkillRef>, String> {
    conn.query_row(
        "SELECT id, name FROM skills WHERE lower(name) = lower(?1) ORDER BY id LIMIT 1",
        [name],
        |r| {
            Ok(SkillRef {
                id: r.get(0)?,
                name: r.get(1)?,
            })
        },
    )
    .optional()
    .map_err(|_| "storage_error: skills could not be read".to_string())
}

/// Resolves a model's goal change against the goals attached to the chat.
///
/// The goal must be one the learner attached: named by its label, or the only
/// attached goal. Removed skills must be on the goal; added skills must exist
/// by exact name and not be on it yet. Names that resolve to nothing are
/// dropped. Returns `None` when nothing would change or the goal would be
/// left without skills.
pub fn resolve_change(
    conn: &Connection,
    attached: &[String],
    draft: &GoalChangeDraft,
) -> Result<Option<Proposal>, String> {
    let mut goals = Vec::new();
    for id in attached {
        if let Some(goal) = find(conn, id)? {
            goals.push(goal);
        }
    }
    let wanted = draft.goal.to_lowercase();
    let goal = match goals
        .iter()
        .position(|g| g.label.trim().to_lowercase() == wanted)
    {
        Some(i) => goals.swap_remove(i),
        None if goals.len() == 1 => goals.remove(0),
        None => return Ok(None),
    };
    let mut remove = Vec::new();
    for name in &draft.remove {
        let wanted = name.to_lowercase();
        for id in &goal.goal_skill_ids {
            if remove.iter().any(|s: &SkillRef| &s.id == id) {
                continue;
            }
            if let Some(found) = skill_name(conn, id)? {
                if found.to_lowercase() == wanted {
                    remove.push(SkillRef {
                        id: id.clone(),
                        name: found,
                    });
                }
            }
        }
    }
    let mut add: Vec<SkillRef> = Vec::new();
    for name in &draft.add {
        if let Some(skill) = skill_named(conn, name)? {
            if !goal.goal_skill_ids.contains(&skill.id) && !add.iter().any(|s| s.id == skill.id) {
                add.push(skill);
            }
        }
    }
    let rename = draft.rename.clone().filter(|r| r != &goal.label);
    let remaining = goal
        .goal_skill_ids
        .iter()
        .filter(|id| !remove.iter().any(|s| &s.id == *id))
        .count()
        + add.len();
    if (add.is_empty() && remove.is_empty() && rename.is_none()) || remaining == 0 {
        return Ok(None);
    }
    Ok(Some(Proposal {
        title: goal.label,
        body: Body::GoalChange {
            goal_id: goal.id,
            rename,
            add,
            remove,
            text: draft.text.clone(),
        },
    }))
}

/// Resolves a course a model suggested enrolling in. It must be recommended
/// for a skill not yet proven on one of the attached goals' paths, named by
/// its exact title, and not one the learner is already taking.
pub fn resolve_enroll(
    conn: &Connection,
    attached: &[String],
    course: &str,
    text: &str,
) -> Result<Option<Proposal>, String> {
    let wanted = course.trim().to_lowercase();
    for id in attached {
        let Some(goal) = find(conn, id)? else {
            continue;
        };
        let path = alexandria_studio::skills::my_learning_path(conn, &goal.goal_skill_ids)
            .map_err(|_| "storage_error: the goal's path could not be computed".to_string())?;
        let found = path
            .steps
            .iter()
            .filter(|s| s.status != "earned")
            .flat_map(|s| &s.course_recs)
            .find(|c| c.title.trim().to_lowercase() == wanted);
        let Some(found) = found else {
            continue;
        };
        let taking: bool = conn
            .query_row(
                "SELECT count(*) > 0 FROM enrollments WHERE course_id = ?1 AND status = 'active'",
                [&found.course_id],
                |r| r.get(0),
            )
            .map_err(|_| "storage_error: enrollments could not be read".to_string())?;
        if taking {
            return Ok(None);
        }
        return Ok(Some(Proposal {
            title: found.title.clone(),
            body: Body::Enroll {
                course_id: found.course_id.clone(),
                goal_id: goal.id,
                text: text.to_string(),
            },
        }));
    }
    Ok(None)
}
