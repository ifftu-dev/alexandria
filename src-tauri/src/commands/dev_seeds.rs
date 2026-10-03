use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard, OnceLock};

use rusqlite::{params, params_from_iter, types::Value, Connection};
use serde::{Deserialize, Serialize};

use crate::crypto::hash::entity_id;
use crate::db::{executor::DatabaseWorkload, Database};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

pub(crate) const fn enabled() -> bool {
    cfg!(any(debug_assertions, feature = "dev-seeding"))
}
pub(super) fn require_enabled() -> Result<(), String> {
    if enabled() {
        Ok(())
    } else {
        Err("Test data is available only in development builds".into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedResource {
    pub id: String,
    pub title: String,
    pub category: String,
    pub description: String,
    pub dependencies: Vec<String>,
    pub installed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedCatalog {
    pub enabled: bool,
    pub resources: Vec<SeedResource>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedResult {
    pub id: String,
    pub status: String,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedDraft {
    pub id: String,
    pub title: String,
    pub body: String,
}
const DRAFTS: &[(&str, &str, &str)] = &[
    ("algorithms", "When is a simple algorithm the better choice?", "Testing prompt: compare a straightforward solution with an optimized one. What input size makes the added complexity worthwhile? Replace this prompt with your own analysis before publishing."),
    ("web-state", "Where should application state live?", "Testing prompt: compare local component state, shared state, and server state. Describe a concrete example and the tradeoffs."),
    ("testing", "Which tests earn their maintenance cost?", "Testing prompt: share one regression that a focused test would catch. How would you avoid testing implementation details?"),
    ("accessibility", "What makes a form accessible?", "Testing prompt: review keyboard navigation, labels, validation messages, and focus. Add your own example before publishing."),
    ("cryptography", "How should applications explain key recovery?", "Testing prompt: discuss the usability and security tradeoffs of account recovery without sharing keys or recovery phrases."),
    ("learning", "What evidence demonstrates understanding?", "Testing prompt: compare recall questions, projects, and explanations. What would convince you that someone can apply a concept?"),
];

fn bundled_snapshot() -> Result<MutexGuard<'static, Database>, String> {
    static SNAPSHOT: OnceLock<Result<Mutex<Database>, String>> = OnceLock::new();
    SNAPSHOT
        .get_or_init(|| {
            let db = Database::open_in_memory().map_err(|e| e.to_string())?;
            db.run_migrations().map_err(|e| e.to_string())?;
            crate::db::bundled::install_bundled_data(db.conn())?;
            Ok(Mutex::new(db))
        })
        .as_ref()
        .map_err(Clone::clone)?
        .lock()
        .map_err(|e| e.to_string())
}

fn exists(conn: &Connection, table: &str, column: &str, id: &str) -> Result<bool, String> {
    conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {column}=?1)"),
        [id],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}
pub(super) fn owner(conn: &Connection) -> Result<String, String> {
    conn.query_row(
        "SELECT stake_address FROM local_identity WHERE id=1",
        [],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}
pub(super) fn catalog(db: &Database) -> Result<Vec<SeedResource>, String> {
    let author = owner(db.conn())?;
    let mut items = vec![];
    let mut add = |id: String,
                   title: String,
                   category: &str,
                   description: String,
                   dependencies: Vec<String>,
                   installed: bool| {
        items.push(SeedResource {
            id,
            title,
            category: category.into(),
            description,
            dependencies,
            installed,
        });
    };
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("../../../demo-world/content/courses.json"))
            .map_err(|e| e.to_string())?;
    for c in corpus["courses"]
        .as_array()
        .ok_or("Invalid course corpus")?
        .iter()
        .filter(|c| c["kind"] == "course")
    {
        let id = c["id"].as_str().ok_or("Missing course ID")?;
        let dependencies = if id == "course_plugin_demo" {
            vec![
                "plugin:music-reviews",
                "plugin:irl-review",
                "plugin:editors",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect()
        } else {
            vec![]
        };
        add(
            format!("course:{id}"),
            c["title"].as_str().unwrap_or(id).into(),
            "Courses",
            c["description"].as_str().unwrap_or("").into(),
            dependencies,
            exists(
                db.conn(),
                "courses",
                "id",
                &entity_id(&["example-course-v1", &author, id]),
            )?,
        );
    }
    let resources: serde_json::Value =
        serde_json::from_str(include_str!("../../../demo-world/content/resources.json"))
            .map_err(|e| e.to_string())?;
    for (key, prefix, category, table, namespace, title_key) in [
        (
            "courses",
            "video",
            "Courses",
            "courses",
            "example-video-course-v1",
            "title",
        ),
        (
            "classrooms",
            "classroom",
            "Classrooms",
            "classrooms",
            "example-classroom-v1",
            "name",
        ),
        ("media", "media", "Media", "pins", "", "title"),
    ] {
        for r in resources[key].as_array().ok_or("Invalid resource corpus")? {
            let id = r["id"].as_str().ok_or("Missing resource ID")?;
            let deps = r["lesson_ids"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(|v| v.as_str())
                        .map(|id| format!("media:{id}"))
                        .collect()
                })
                .unwrap_or_default();
            let (column, row_id) = if prefix == "media" {
                ("cid", super::demo_resources::media_cid(id)?)
            } else {
                ("id", entity_id(&[namespace, &author, id]))
            };
            add(
                format!("{prefix}:{id}"),
                r[title_key].as_str().unwrap_or(id).into(),
                category,
                r["description"]
                    .as_str()
                    .unwrap_or("Bundled local teaching video with synthetic narration.")
                    .into(),
                deps,
                exists(db.conn(), table, column, &row_id)?,
            );
        }
    }
    for r in resources["media"]
        .as_array()
        .ok_or("Invalid media corpus")?
    {
        let id = r["id"].as_str().ok_or("Missing media ID")?;
        if let Ok(bytes) = super::demo_resources::thumbnail_bytes(id) {
            let cid = blake3::hash(bytes).to_hex().to_string();
            add(
                format!("thumbnail:{id}"),
                format!("{} — thumbnail", r["title"].as_str().unwrap_or(id)),
                "Media",
                "Bundled JPEG thumbnail.".into(),
                vec![],
                exists(db.conn(), "pins", "cid", &cid)?,
            );
        }
    }
    for b in crate::plugins::builtins::BUILTIN_PLUGINS {
        let manifest = crate::plugins::manifest::parse_and_validate(b.manifest_json)?;
        let deps = manifest
            .dependencies
            .iter()
            .map(|id| {
                crate::plugins::builtins::BUILTIN_PLUGINS
                    .iter()
                    .find(|b| {
                        crate::plugins::manifest::parse_and_validate(b.manifest_json)
                            .is_ok_and(|m| m.id == *id)
                    })
                    .map(|b| format!("plugin:{}", b.slug))
                    .ok_or_else(|| format!("Unbundled dependency: {id}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let cid = crate::plugins::verifier::compute_plugin_cid(b.manifest_json);
        add(
            format!("plugin:{}", b.slug),
            manifest.name,
            "Plugins",
            "Exact bundled plugin and grader bytes.".into(),
            deps,
            crate::plugins::registry::get_installed(db, &cid)?.is_some(),
        );
    }
    let source = bundled_snapshot()?;
    for (table, prefix, category) in [
        ("question_banks", "bank", "Assessments"),
        ("goal_templates", "goal", "Core data"),
    ] {
        let mut stmt = source
            .conn()
            .prepare(&format!("SELECT id,label FROM {table} ORDER BY label"))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (id, title) = row.map_err(|e| e.to_string())?;
            add(
                format!("{prefix}:{id}"),
                title,
                category,
                if prefix == "bank" {
                    "Question bank and assessment items. No attempts or credentials."
                } else {
                    "Bundled goal template. No learner progress."
                }
                .into(),
                vec![],
                exists(db.conn(), table, "id", &id)?,
            );
        }
    }
    for (id, title, body) in DRAFTS {
        add(
            format!("draft:{id}"),
            (*title).into(),
            "Discussions",
            (*body).into(),
            vec![],
            exists(db.conn(), "developer_discussion_drafts", "id", id)?,
        );
    }
    Ok(items)
}
pub(super) fn plan(
    items: &[SeedResource],
    selected: &[String],
) -> Result<Vec<SeedResource>, String> {
    fn visit(
        id: &str,
        items: &[SeedResource],
        visiting: &mut BTreeSet<String>,
        seen: &mut BTreeSet<String>,
        out: &mut Vec<SeedResource>,
    ) -> Result<(), String> {
        if seen.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id.into()) {
            return Err("Seed dependency cycle".into());
        }
        let item = items
            .iter()
            .find(|i| i.id == id)
            .ok_or_else(|| format!("Unknown seed: {id}"))?;
        for dep in &item.dependencies {
            visit(dep, items, visiting, seen, out)?;
        }
        visiting.remove(id);
        seen.insert(id.into());
        out.push(item.clone());
        Ok(())
    }
    let mut out = vec![];
    let mut seen = BTreeSet::new();
    for id in selected {
        visit(id, items, &mut BTreeSet::new(), &mut seen, &mut out)?;
    }
    Ok(out)
}

// Copies only rows from our embedded, migrated fixture, never caller-supplied SQL.
fn copy_rows(
    source: &Connection,
    dest: &Connection,
    table: &str,
    filter: &str,
    id: &str,
) -> Result<(), String> {
    let mut stmt = source
        .prepare(&format!("SELECT * FROM {table} WHERE {filter}"))
        .map_err(|e| e.to_string())?;
    let count = stmt.column_count();
    let columns = stmt
        .column_names()
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(",");
    let placeholders = vec!["?"; count].join(",");
    let insert = format!("INSERT OR IGNORE INTO {table} ({columns}) VALUES ({placeholders})");
    let rows = stmt
        .query_map([id], |r| {
            (0..count)
                .map(|i| r.get::<_, Value>(i))
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|e| e.to_string())?;
    for row in rows {
        dest.execute(&insert, params_from_iter(row.map_err(|e| e.to_string())?))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub(super) fn install(
    db: &Database,
    plugins_dir: &std::path::Path,
    item: &SeedResource,
) -> Result<(), String> {
    let (kind, id) = item.id.split_once(':').ok_or("Invalid seed ID")?;
    match kind {
        "course" => {
            super::demo_courses::import_selected(db.conn(), id == "course_plugin_demo", Some(id))?;
        }
        "video" | "classroom" | "media" | "thumbnail" => {
            super::demo_resources::install_selected_rows(db.conn(), Some(&item.id))?
        }
        "plugin" => {
            let bundle = crate::plugins::builtins::BUILTIN_PLUGINS
                .iter()
                .find(|b| b.slug == id)
                .ok_or("Unknown plugin")?;
            crate::plugins::registry::install_builtin(db, plugins_dir, bundle)?;
            let manifest = crate::plugins::manifest::parse_and_validate(bundle.manifest_json)?;
            let cid = crate::plugins::verifier::compute_plugin_cid(bundle.manifest_json);
            let announcement = crate::plugins::catalog::announcement_from_manifest(
                &cid,
                &manifest,
                &chrono::Utc::now().to_rfc3339(),
            );
            crate::plugins::catalog::upsert_announcement(db, &announcement, "builtin")?;
        }
        "bank" | "goal" => {
            let source = bundled_snapshot()?;
            crate::db::with_transaction(db.conn(), || {
                if kind == "goal" {
                    copy_rows(source.conn(), db.conn(), "goal_templates", "id=?1", id)?;
                } else {
                    copy_rows(source.conn(), db.conn(), "question_banks", "id=?1", id)?;
                    copy_rows(
                        source.conn(),
                        db.conn(),
                        "assessment_items",
                        "bank_id=?1",
                        id,
                    )?;
                    copy_rows(
                        source.conn(),
                        db.conn(),
                        "assessment_item_skills",
                        "item_id IN (SELECT id FROM assessment_items WHERE bank_id=?1)",
                        id,
                    )?;
                }
                Ok(())
            })?;
        }
        "draft" => {
            let (_, title, body) = DRAFTS.iter().find(|d| d.0 == id).ok_or("Unknown draft")?;
            db.conn().execute("INSERT OR IGNORE INTO developer_discussion_drafts (id,title,body) VALUES (?1,?2,?3)", params![id,title,body]).map_err(|e| e.to_string())?;
        }
        _ => return Err("Unknown seed kind".into()),
    }
    Ok(())
}

#[tauri::command]
pub async fn dev_seed_catalog(state: State<'_, AppState>) -> Result<SeedCatalog, String> {
    if !enabled() {
        return Ok(SeedCatalog {
            enabled: false,
            resources: vec![],
        });
    }
    let resources = state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "dev_seeds.catalog",
            catalog,
        )
        .await?;
    Ok(SeedCatalog {
        enabled: true,
        resources,
    })
}
#[tauri::command]
pub async fn dev_seed_plan(
    state: State<'_, AppState>,
    selected: Vec<String>,
) -> Result<Vec<SeedResource>, String> {
    require_enabled()?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "dev_seeds.plan",
            move |db| plan(&catalog(db)?, &selected),
        )
        .await
}
#[tauri::command]
pub async fn dev_seed_run(
    state: State<'_, AppState>,
    selected: Vec<String>,
) -> Result<Vec<SeedResult>, String> {
    require_enabled()?;
    let items = state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "dev_seeds.validate",
            move |db| plan(&catalog(db)?, &selected),
        )
        .await?;
    let plugins_dir = state.plugins_dir()?;
    let mut results = vec![];
    let mut failed = BTreeSet::new();
    for item in items {
        let id = item.id.clone();
        let result = async {
            if item.dependencies.iter().any(|id| failed.contains(id)) {
                return Err("A required resource failed. Retry after resolving its error.".into());
            }
            // Repair an evicted blob even when its pin row is still present.
            let bytes = if let Some(media_id) = id.strip_prefix("media:") {
                Some(super::demo_resources::media_bytes(media_id)?)
            } else if let Some(thumbnail_id) = id.strip_prefix("thumbnail:") {
                Some(super::demo_resources::thumbnail_bytes(thumbnail_id)?)
            } else {
                None
            };
            if let Some(bytes) = bytes {
                let cid = blake3::hash(bytes).to_hex().to_string();
                if !crate::content_store::content::has(&state.content_node, &cid)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    crate::content_store::content::add_bytes_unencrypted(
                        &state.content_node,
                        bytes,
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                }
            }
            let dir = plugins_dir.clone();
            state
                .db_executor
                .execute(
                    DatabaseWorkload::Instructor,
                    state.profile_lease(),
                    "dev_seeds.install",
                    move |db| {
                        let existing = catalog(db)?
                            .into_iter()
                            .find(|r| r.id == item.id)
                            .ok_or("Resource no longer available")?
                            .installed;
                        if !existing {
                            install(db, &dir, &item)?;
                        }
                        Ok(if existing { "kept" } else { "added" }.to_string())
                    },
                )
                .await
        }
        .await;
        match result {
            Ok(status) => results.push(SeedResult {
                id,
                status,
                error: None,
            }),
            Err(error) => {
                failed.insert(id.clone());
                results.push(SeedResult {
                    id,
                    status: "failed".into(),
                    error: Some(error),
                });
            }
        }
    }
    Ok(results)
}
#[tauri::command]
pub async fn dev_seed_drafts(state: State<'_, AppState>) -> Result<Vec<SeedDraft>, String> {
    require_enabled()?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "dev_seeds.drafts",
            |db| {
                let mut stmt = db
                    .conn()
                    .prepare("SELECT id,title,body FROM developer_discussion_drafts ORDER BY title")
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok(SeedDraft {
                            id: r.get(0)?,
                            title: r.get(1)?,
                            body: r.get(2)?,
                        })
                    })
                    .map_err(|e| e.to_string())?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())
            },
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(author: &str) -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        crate::db::bundled::install_foundation(db.conn()).unwrap();
        db.conn()
            .execute(
                "INSERT INTO local_identity (id,stake_address,payment_address) VALUES (1,?1,'key')",
                [author],
            )
            .unwrap();
        db
    }
    fn count(db: &Database, table: &str) -> i64 {
        db.conn()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    #[test]
    fn selected_seeds_resolve_dependencies_preserve_edits_and_isolate_profiles() {
        let first = profile("first");
        let second = profile("second");
        let dir = tempfile::tempdir().unwrap();
        let items = catalog(&first).unwrap();
        assert_eq!(count(&first, "question_banks"), 0);
        assert!(plan(&items, &["bogus".into()]).is_err());
        let selected = plan(
            &items,
            &[
                "video:algorithm-video-lab".into(),
                "course:course_plugin_demo".into(),
                "bank:qb_js".into(),
                "draft:testing".into(),
            ],
        )
        .unwrap();
        assert!(selected.iter().any(|r| r.id == "media:binary-search"));
        let editor = selected
            .iter()
            .position(|r| r.id == "plugin:editor-javascript")
            .unwrap();
        let collection = selected
            .iter()
            .position(|r| r.id == "plugin:editors")
            .unwrap();
        assert!(editor < collection);
        for item in &selected {
            install(&first, dir.path(), item).unwrap();
        }
        assert_eq!(count(&first, "courses"), 2);
        assert_eq!(count(&first, "question_banks"), 1);
        assert_eq!(count(&first, "assessment_items"), 4);
        assert_eq!(count(&first, "developer_discussion_drafts"), 1);
        assert_eq!(count(&second, "courses"), 0);
        assert_eq!(count(&second, "developer_discussion_drafts"), 0);
        first
            .conn()
            .execute("UPDATE courses SET title='My edited course'", [])
            .unwrap();
        for item in &selected {
            install(&first, dir.path(), item).unwrap();
        }
        assert_eq!(count(&first, "courses"), 2);
        assert!(first
            .conn()
            .query_row(
                "SELECT NOT EXISTS(SELECT 1 FROM courses WHERE title != 'My edited course')",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
        assert!(plan(
            &catalog(&first).unwrap(),
            &selected.iter().map(|i| i.id.clone()).collect::<Vec<_>>()
        )
        .unwrap()
        .iter()
        .all(|i| i.installed));
        assert_eq!(count(&first, "assessment_attempts"), 0);
    }
    #[test]
    fn everything_installs_without_publication_or_fabricated_authority() {
        let db = profile("all-resources");
        let dir = tempfile::tempdir().unwrap();
        let items = catalog(&db).unwrap();
        let selected = plan(
            &items,
            &items.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
        )
        .unwrap();
        for item in &selected {
            install(&db, dir.path(), item).unwrap();
        }
        assert_eq!(count(&db, "courses"), 10);
        assert_eq!(count(&db, "classrooms"), 3);
        assert_eq!(count(&db, "plugin_installed"), 9);
        assert_eq!(count(&db, "question_banks"), 2);
        assert_eq!(count(&db, "developer_discussion_drafts"), 6);
        assert_eq!(count(&db, "discussion_events"), 0);
        assert_eq!(count(&db, "assessment_attempts"), 0);
        assert_eq!(count(&db, "credentials"), 0);
        assert_eq!(count(&db, "enrollments"), 0);
        assert!(catalog(&db).unwrap().iter().all(|i| i.installed));
    }
}
