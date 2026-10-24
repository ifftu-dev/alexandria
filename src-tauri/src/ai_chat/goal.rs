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
//! A model may also propose a change to an attached goal. It names skills;
//! [`resolve_change`] turns names into the learner's own skill ids, and the
//! learner applies the change through the Goals service.

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;

use super::artifact::{Body, GoalChangeDraft, Proposal, SkillRef};

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

fn find(conn: &Connection, goal_id: &str) -> Result<Option<StoredGoal>, String> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'learner.targets'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| "storage_error: goals could not be read".to_string())?;
    let goals: Vec<StoredGoal> = raw
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    Ok(goals.into_iter().find(|g| g.id == goal_id))
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

/// Whether the learner's goal now reflects a change: the skills added are on
/// it, the skills removed are not, and a rename took effect.
pub fn change_applied(conn: &Connection, change: &Body) -> Result<bool, String> {
    let Body::GoalChange {
        goal_id,
        rename,
        add,
        remove,
        ..
    } = change
    else {
        return Ok(false);
    };
    let Some(goal) = find(conn, goal_id)? else {
        return Ok(false);
    };
    Ok(add.iter().all(|s| goal.goal_skill_ids.contains(&s.id))
        && remove.iter().all(|s| !goal.goal_skill_ids.contains(&s.id))
        && rename.as_ref().is_none_or(|r| &goal.label == r))
}
