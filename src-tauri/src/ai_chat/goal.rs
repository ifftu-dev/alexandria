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

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;

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
