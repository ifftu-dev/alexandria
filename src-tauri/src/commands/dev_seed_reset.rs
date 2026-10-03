use std::collections::BTreeSet;

use rusqlite::{types::Value, Connection};
use serde::{Deserialize, Serialize};

use super::dev_seeds::{self, SeedResource, SeedResult};
use crate::crypto::hash::entity_id;
use crate::db::{executor::DatabaseWorkload, Database};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedResetEffect {
    pub label: String,
    pub count: usize,
    pub action: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedResetItem {
    pub id: String,
    pub title: String,
    pub can_reset: bool,
    pub reason: Option<String>,
    pub effects: Vec<SeedResetEffect>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedResetPlan {
    pub resources: Vec<SeedResetItem>,
    pub token: String,
}
struct Mutation {
    table: &'static str,
    filter: String,
    id: String,
    label: &'static str,
    detach_column: Option<&'static str>,
}
impl Mutation {
    fn rows(&self, conn: &Connection) -> Result<Vec<String>, String> {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT * FROM {} WHERE {} ORDER BY rowid",
                self.table, self.filter
            ))
            .map_err(|e| e.to_string())?;
        let n = stmt.column_count();
        let rows = stmt
            .query_map([&self.id], |r| {
                let values = (0..n)
                    .map(|i| r.get::<_, Value>(i))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!("{values:?}"))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }
    fn apply(&self, conn: &Connection) -> Result<(), String> {
        let sql = if let Some(column) = self.detach_column {
            format!(
                "UPDATE {} SET {column}=NULL WHERE {}",
                self.table, self.filter
            )
        } else {
            format!("DELETE FROM {} WHERE {}", self.table, self.filter)
        };
        conn.execute(&sql, [&self.id]).map_err(|e| e.to_string())?;
        Ok(())
    }
}
fn course_id(conn: &Connection, id: &str) -> Result<String, String> {
    let (kind, key) = id.split_once(':').ok_or("Invalid seed")?;
    let namespace = if kind == "video" {
        "example-video-course-v1"
    } else {
        "example-course-v1"
    };
    Ok(entity_id(&[namespace, &dev_seeds::owner(conn)?, key]))
}
fn plugin(id: &str) -> Result<&'static crate::plugins::registry::BuiltinBundle<'static>, String> {
    crate::plugins::builtins::BUILTIN_PLUGINS
        .iter()
        .find(|b| Some(b.slug) == id.strip_prefix("plugin:"))
        .ok_or_else(|| "Unknown bundled plugin".into())
}
fn mutations(conn: &Connection, item: &SeedResource) -> Result<Vec<Mutation>, String> {
    let (kind, key) = item.id.split_once(':').ok_or("Invalid seed")?;
    let mut changes = vec![];
    let mut add = |table, filter: &str, id: &str, label, detach_column| {
        changes.push(Mutation {
            table,
            filter: filter.into(),
            id: id.into(),
            label,
            detach_column,
        })
    };
    match kind {
        "course" | "video" => {
            let id = course_id(conn, &item.id)?;
            let enrolled = "enrollment_id IN (SELECT id FROM enrollments WHERE course_id=?1)";
            let elements = "element_id IN (SELECT e.id FROM course_elements e JOIN course_chapters c ON c.id=e.chapter_id WHERE c.course_id=?1)";
            add(
                "integrity_sessions",
                enrolled,
                &id,
                "Integrity sessions retained; enrollment links cleared",
                Some("enrollment_id"),
            );
            add(
                "completion_claims",
                enrolled,
                &id,
                "Completion evidence retained; enrollment links cleared",
                Some("enrollment_id"),
            );
            add(
                "role_assessments",
                "course_id=?1",
                &id,
                "Role assessments retained; backing course links cleared",
                Some("course_id"),
            );
            add(
                "course_lesson_feedback",
                "course_id=?1",
                &id,
                "Lesson feedback",
                None,
            );
            add(
                "studio_tutor_threads",
                "course_id=?1",
                &id,
                "Tutor conversations",
                None,
            );
            add(
                "course_tutor_policies",
                "course_id=?1",
                &id,
                "Course tutor settings",
                None,
            );
            add(
                "studio_documents",
                "kind='course' AND id=?1",
                &id,
                "Instructor authoring document",
                None,
            );
            add(
                "plugin_irl_submissions",
                "course_id=?1 OR enrollment_id IN (SELECT id FROM enrollments WHERE course_id=?1)",
                &id,
                "Plugin review submissions",
                None,
            );
            add("course_notes", enrolled, &id, "Learner notes", None);
            add("element_progress", enrolled, &id, "Lesson progress", None);
            add(
                "element_submissions",
                enrolled,
                &id,
                "Graded lesson submissions",
                None,
            );
            add("enrollments", "course_id=?1", &id, "Enrollments", None);
            add(
                "plugin_element_state",
                elements,
                &id,
                "Plugin exercise state",
                None,
            );
            add("video_chapters", elements, &id, "Video chapters", None);
            add(
                "element_skill_tags",
                elements,
                &id,
                "Lesson skill tags",
                None,
            );
            add(
                "course_elements",
                "chapter_id IN (SELECT id FROM course_chapters WHERE course_id=?1)",
                &id,
                "Lessons and quizzes",
                None,
            );
            add(
                "course_chapters",
                "course_id=?1",
                &id,
                "Course chapters",
                None,
            );
            add("courses", "id=?1", &id, "Course (including edits)", None);
        }
        "classroom" => {
            let id = entity_id(&["example-classroom-v1", &dev_seeds::owner(conn)?, key]);
            for (table, label) in [
                ("classroom_calls", "Classroom call records"),
                ("classroom_messages", "Classroom messages"),
                ("classroom_join_requests", "Join requests"),
                ("classroom_members", "Classroom memberships"),
                ("classroom_group_keys", "Classroom group key"),
                ("classroom_channels", "Channels"),
            ] {
                add(table, "classroom_id=?1", &id, label, None);
            }
            add(
                "classrooms",
                "id=?1",
                &id,
                "Classroom (including edits)",
                None,
            );
        }
        "bank" => {
            add(
                "attempt_items",
                "attempt_id IN (SELECT id FROM assessment_attempts WHERE bank_id=?1)",
                key,
                "Attempt questions and answers",
                None,
            );
            add(
                "assessment_attempts",
                "bank_id=?1",
                key,
                "Assessment attempts and results",
                None,
            );
            add(
                "assessment_item_skills",
                "item_id IN (SELECT id FROM assessment_items WHERE bank_id=?1)",
                key,
                "Question skill mappings",
                None,
            );
            add(
                "assessment_items",
                "bank_id=?1",
                key,
                "Assessment questions (including edits)",
                None,
            );
            add("question_banks", "id=?1", key, "Question bank", None);
        }
        "goal" => add(
            "goal_templates",
            "id=?1",
            key,
            "Goal template (learner goals retained)",
            None,
        ),
        "draft" => add(
            "developer_discussion_drafts",
            "id=?1",
            key,
            "Local discussion draft",
            None,
        ),
        "media" | "thumbnail" => {
            let bytes = if kind == "media" {
                super::demo_resources::media_bytes(key)?
            } else {
                super::demo_resources::thumbnail_bytes(key)?
            };
            add(
                "pins",
                "cid=?1",
                blake3::hash(bytes).to_hex().as_ref(),
                "Media pin (cached bytes retained)",
                None,
            );
        }
        "plugin" => {
            let cid = crate::plugins::verifier::compute_plugin_cid(plugin(&item.id)?.manifest_json);
            add(
                "plugin_irl_submissions",
                "plugin_cid=?1",
                &cid,
                "Plugin review submissions",
                None,
            );
            add(
                "plugin_element_state",
                "plugin_cid=?1",
                &cid,
                "Plugin exercise state",
                None,
            );
            add(
                "plugin_permissions",
                "plugin_cid=?1",
                &cid,
                "Plugin permission grants",
                None,
            );
            add(
                "plugin_dependencies",
                "plugin_cid=?1 OR dependency_cid=?1",
                &cid,
                "Plugin dependency links",
                None,
            );
            add(
                "plugin_installed",
                "plugin_cid=?1",
                &cid,
                "Installed plugin (cached files retained)",
                None,
            );
        }
        _ => return Err("Unknown seed".into()),
    }
    Ok(changes)
}
fn any(conn: &Connection, sql: &str, id: &str) -> Result<bool, String> {
    conn.query_row(sql, [id], |r| r.get(0))
        .map_err(|e| e.to_string())
}
fn protected_reason(
    db: &Database,
    item: &SeedResource,
    selected: &BTreeSet<String>,
    items: &[SeedResource],
) -> Result<Option<String>, String> {
    if !item.installed {
        return Ok(Some("Not installed".into()));
    }
    if item.id.starts_with("plugin:") {
        let m = crate::plugins::manifest::parse_and_validate(plugin(&item.id)?.manifest_json)?;
        if m.scope == crate::domain::plugin::PluginScope::Global {
            return Ok(Some(
                "Required global plugin; retained for the app to work".into(),
            ));
        }
    }
    if items
        .iter()
        .any(|r| r.installed && !selected.contains(&r.id) && r.dependencies.contains(&item.id))
    {
        return Ok(Some(
            "Needed by another installed resource outside this reset".into(),
        ));
    }
    let selected_courses = selected
        .iter()
        .filter(|id| id.starts_with("course:") || id.starts_with("video:"))
        .map(|id| course_id(db.conn(), id))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if item.id.starts_with("plugin:")
        || item.id.starts_with("media:")
        || item.id.starts_with("thumbnail:")
    {
        let (kind, key) = item.id.split_once(':').ok_or("Invalid seed")?;
        let cid = match kind {
            "plugin" => {
                crate::plugins::verifier::compute_plugin_cid(plugin(&item.id)?.manifest_json)
            }
            "media" => super::demo_resources::media_cid(key)?,
            _ => blake3::hash(super::demo_resources::thumbnail_bytes(key)?)
                .to_hex()
                .to_string(),
        };
        let sql = if kind == "plugin" {
            "SELECT DISTINCT c.course_id FROM course_elements e JOIN course_chapters c ON e.chapter_id=c.id WHERE e.plugin_cid=?1"
        } else {
            "SELECT id FROM courses WHERE thumbnail_cid=?1 UNION SELECT c.course_id FROM course_elements e JOIN course_chapters c ON e.chapter_id=c.id WHERE e.content_cid=?1 OR e.plugin_config_cid=?1 OR instr(e.content_inline,?1)>0"
        };
        let mut stmt = db.conn().prepare(sql).map_err(|e| e.to_string())?;
        for row in stmt
            .query_map([&cid], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
        {
            if !selected_courses.contains(&row.map_err(|e| e.to_string())?) {
                return Ok(Some("Used by a course outside this reset".into()));
            }
        }
        let shared = if kind == "plugin" {
            any(
                db.conn(),
                "SELECT EXISTS(SELECT 1 FROM assessment_items WHERE plugin_cid=?1)",
                &cid,
            )?
        } else {
            any(db.conn(), "SELECT EXISTS(SELECT 1 FROM discussion_items WHERE instr(content_json,?1)>0) OR EXISTS(SELECT 1 FROM opinions WHERE video_cid=?1 OR thumbnail_cid=?1) OR EXISTS(SELECT 1 FROM catalog WHERE content_cid=?1 OR thumbnail_cid=?1) OR EXISTS(SELECT 1 FROM discussion_events WHERE instr(signed_json,?1)>0) OR EXISTS(SELECT 1 FROM classroom_messages WHERE instr(content,?1)>0) OR EXISTS(SELECT 1 FROM content_mappings WHERE blake3_hash=?1)", &cid)?
        };
        if shared {
            return Ok(Some(
                "Referenced by other assessments or published content; retained".into(),
            ));
        }
    }
    if item.id.starts_with("course:") || item.id.starts_with("video:") {
        let id = course_id(db.conn(), &item.id)?;
        if any(db.conn(), "SELECT EXISTS(SELECT 1 FROM integrity_sessions WHERE enrollment_id IN (SELECT id FROM enrollments WHERE course_id=?1) AND status='active')", &id)? {
            return Ok(Some("Finish the active assessment before resetting this course".into()));
        }
    }
    if let Some(key) = item.id.strip_prefix("bank:") {
        if any(db.conn(), "SELECT EXISTS(SELECT 1 FROM assessment_attempts WHERE bank_id=?1 AND graded_at IS NULL AND ended_at IS NULL)", key)? {
            return Ok(Some("Finish or end the active assessment before resetting this bank".into()));
        }
    }
    if item.id.starts_with("plugin:") {
        let cid = crate::plugins::verifier::compute_plugin_cid(plugin(&item.id)?.manifest_json);
        let selected_plugins = selected
            .iter()
            .filter(|id| id.starts_with("plugin:"))
            .map(|id| {
                plugin(id).map(|b| crate::plugins::verifier::compute_plugin_cid(b.manifest_json))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        let mut stmt = db
            .conn()
            .prepare("SELECT plugin_cid FROM plugin_dependencies WHERE dependency_cid=?1")
            .map_err(|e| e.to_string())?;
        for row in stmt
            .query_map([&cid], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
        {
            if !selected_plugins.contains(&row.map_err(|e| e.to_string())?) {
                return Ok(Some(
                    "Needed by another installed plugin outside this reset".into(),
                ));
            }
        }
    }
    if let Some(key) = item.id.strip_prefix("classroom:") {
        let id = entity_id(&["example-classroom-v1", &dev_seeds::owner(db.conn())?, key]);
        if any(db.conn(), "SELECT EXISTS(SELECT 1 FROM classroom_calls WHERE classroom_id=?1 AND status='active')", &id)? {
            return Ok(Some("End the active classroom call before resetting".into()));
        }
    }
    Ok(None)
}
fn build_plan(
    db: &Database,
    selected: &[String],
) -> Result<(SeedResetPlan, Vec<Mutation>), String> {
    let items = dev_seeds::catalog(db)?;
    let requested: BTreeSet<String> = selected.iter().cloned().collect();
    // Validate every ID, sort dependents before dependencies, and do not add
    // dependencies the user did not select for deletion.
    let ordered: Vec<_> = dev_seeds::plan(&items, selected)?
        .into_iter()
        .rev()
        .filter(|r| requested.contains(&r.id))
        .collect();
    let mut removable = requested.clone();
    loop {
        let before = removable.len();
        for item in &ordered {
            if protected_reason(db, item, &removable, &items)?.is_some() {
                removable.remove(&item.id);
            }
        }
        if removable.len() == before {
            break;
        }
    }
    let mut resources = vec![];
    let mut changes = vec![];
    let mut hash = blake3::Hasher::new();
    hash.update(dev_seeds::owner(db.conn())?.as_bytes());
    for item in ordered {
        let reason = protected_reason(db, &item, &removable, &items)?;
        let can_reset = reason.is_none();
        let mut effects = vec![];
        if can_reset {
            for mutation in mutations(db.conn(), &item)? {
                let rows = mutation.rows(db.conn())?;
                hash.update(format!("{}:{}:{:?}", item.id, mutation.table, rows).as_bytes());
                if !rows.is_empty() {
                    effects.push(SeedResetEffect {
                        label: mutation.label.into(),
                        count: rows.len(),
                        action: if mutation.detach_column.is_some() {
                            "detach"
                        } else {
                            "remove"
                        }
                        .into(),
                    });
                }
                changes.push(mutation);
            }
        }
        let title = if item.installed
            && (item.id.starts_with("course:") || item.id.starts_with("video:"))
        {
            let id = course_id(db.conn(), &item.id)?;
            let current: String = db
                .conn()
                .query_row("SELECT title FROM courses WHERE id=?1", [&id], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            if current == item.title {
                current
            } else {
                format!("{current} (seed: {})", item.title)
            }
        } else {
            item.title
        };
        resources.push(SeedResetItem {
            id: item.id,
            title,
            can_reset,
            reason,
            effects,
        });
    }
    hash.update(
        serde_json::to_string(&resources)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    );
    Ok((
        SeedResetPlan {
            resources,
            token: hash.finalize().to_hex().to_string(),
        },
        changes,
    ))
}
fn reset(db: &Database, selected: &[String], token: &str) -> Result<Vec<SeedResult>, String> {
    crate::db::with_transaction(db.conn(), || {
        let (plan, changes) = build_plan(db, selected)?;
        if plan.token != token {
            return Err("Affected data changed. Review the reset again before continuing.".into());
        }
        for mutation in changes {
            mutation.apply(db.conn())?;
        }
        Ok(plan
            .resources
            .into_iter()
            .map(|r| SeedResult {
                id: r.id,
                status: if r.can_reset { "removed" } else { "kept" }.into(),
                error: r.reason,
            })
            .collect())
    })
}
#[tauri::command]
pub async fn dev_seed_reset_plan(
    state: State<'_, AppState>,
    selected: Vec<String>,
) -> Result<SeedResetPlan, String> {
    dev_seeds::require_enabled()?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "dev_seeds.reset_plan",
            move |db| build_plan(db, &selected).map(|(plan, _)| plan),
        )
        .await
}
#[tauri::command]
pub async fn dev_seed_reset_run(
    state: State<'_, AppState>,
    selected: Vec<String>,
    token: String,
) -> Result<Vec<SeedResult>, String> {
    dev_seeds::require_enabled()?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "dev_seeds.reset_run",
            move |db| reset(db, &selected, &token),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup(selected: &[&str]) -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        crate::db::bundled::install_foundation(db.conn()).unwrap();
        db.conn().execute("INSERT INTO local_identity(id,stake_address,payment_address) VALUES(1,'reset-owner','key')", []).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let items = dev_seeds::catalog(&db).unwrap();
        for item in dev_seeds::plan(
            &items,
            &selected.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        )
        .unwrap()
        {
            dev_seeds::install(&db, dir.path(), &item).unwrap();
        }
        db
    }
    fn count(db: &Database, table: &str) -> i64 {
        db.conn()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    #[test]
    fn reset_reviews_edits_and_progress_rejects_stale_review_and_retains_integrity_history() {
        let selected: Vec<String> = vec!["course:course_algo_101".into()];
        let db = setup(&["course:course_algo_101"]);
        let id = course_id(db.conn(), &selected[0]).unwrap();
        db.conn()
            .execute(
                "UPDATE courses SET title='Edited by learner' WHERE id=?1",
                [&id],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO enrollments(id,course_id) VALUES('enrollment',?1)",
                [&id],
            )
            .unwrap();
        db.conn().execute("INSERT INTO integrity_sessions(id,enrollment_id,status) VALUES('history','enrollment','completed')", []).unwrap();
        let (review, _) = build_plan(&db, &selected).unwrap();
        assert!(review.resources[0]
            .effects
            .iter()
            .any(|e| e.label == "Enrollments" && e.count == 1));
        db.conn()
            .execute(
                "UPDATE courses SET title='Edited after review' WHERE id=?1",
                [&id],
            )
            .unwrap();
        assert!(reset(&db, &selected, &review.token)
            .unwrap_err()
            .contains("changed"));
        assert_eq!(count(&db, "courses"), 1);
        let (review, _) = build_plan(&db, &selected).unwrap();
        let results = reset(&db, &selected, &review.token).unwrap();
        assert_eq!(results[0].status, "removed");
        assert_eq!(count(&db, "courses"), 0);
        assert_eq!(count(&db, "course_elements"), 0);
        assert_eq!(count(&db, "enrollments"), 0);
        assert_eq!(count(&db, "integrity_sessions"), 1);
        assert!(db
            .conn()
            .query_row(
                "SELECT enrollment_id IS NULL FROM integrity_sessions",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
    }
    #[test]
    fn reset_all_removes_local_resources_and_allows_reseeding() {
        let db = setup(&[
            "video:algorithm-video-lab",
            "bank:qb_js",
            "classroom:algorithms",
            "draft:testing",
            "thumbnail:op_cs_01",
        ]);
        let selected: Vec<_> = dev_seeds::catalog(&db)
            .unwrap()
            .into_iter()
            .filter(|r| r.installed)
            .map(|r| r.id)
            .collect();
        let (review, _) = build_plan(&db, &selected).unwrap();
        assert!(review.resources.iter().all(|r| r.can_reset));
        reset(&db, &selected, &review.token).unwrap();
        for table in [
            "courses",
            "question_banks",
            "assessment_items",
            "classrooms",
            "classroom_members",
            "developer_discussion_drafts",
            "pins",
        ] {
            assert_eq!(count(&db, table), 0, "{table}");
        }
        assert!(build_plan(&db, &["course:unknown".into()]).is_err());
        let dir = tempfile::tempdir().unwrap();
        for item in dev_seeds::plan(&dev_seeds::catalog(&db).unwrap(), &selected).unwrap() {
            dev_seeds::install(&db, dir.path(), &item).unwrap();
        }
        assert_eq!(count(&db, "courses"), 1);
    }
    #[test]
    fn reset_keeps_shared_media_and_active_assessments() {
        let db = setup(&["video:algorithm-video-lab", "bank:qb_js"]);
        let media = vec!["media:binary-search".into()];
        let (review, _) = build_plan(&db, &media).unwrap();
        assert!(!review.resources[0].can_reset);
        db.conn().execute("INSERT INTO assessment_attempts(id,subject_did,bank_id,skill_id,seed,question_ids,option_orders) VALUES('attempt','learner','qb_js','skill_javascript',1,'[]','[]')", []).unwrap();
        let banks = vec!["bank:qb_js".into()];
        assert!(!build_plan(&db, &banks).unwrap().0.resources[0].can_reset);
        db.conn()
            .execute(
                "UPDATE assessment_attempts SET ended_at='2026-10-03',end_reason='interrupted'",
                [],
            )
            .unwrap();
        let (review, _) = build_plan(&db, &banks).unwrap();
        assert!(review.resources[0]
            .effects
            .iter()
            .any(|e| e.label == "Assessment attempts and results" && e.count == 1));
        reset(&db, &banks, &review.token).unwrap();
        assert_eq!(count(&db, "assessment_attempts"), 0);
    }
}
