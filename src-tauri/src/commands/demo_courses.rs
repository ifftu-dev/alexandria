use rusqlite::params;
use serde::Deserialize;

use crate::crypto::hash::entity_id;
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

// Temporary demo rollout: disable this single switch when new profiles should start empty.
const AUTO_SEED_DEMO_CONTENT: bool = true;

const CORPUS: &str = include_str!("../../../demo-world/content/courses.json");

pub(crate) async fn install_for_profile(state: &AppState) -> Result<(), String> {
    if !AUTO_SEED_DEMO_CONTENT || super::dev_seeds::enabled() {
        return Ok(());
    }
    let database = state.db.clone();
    let plugins_dir = state.plugins_dir()?;
    tokio::task::spawn_blocking(move || {
        let guard = database.lock().map_err(|e| e.to_string())?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        let created = install_profile_content(db, &plugins_dir)?;
        log::info!("profile demo content ready: {created} new courses");
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| format!("demo content install task failed: {e}"))??;
    crate::commands::demo_resources::install_for_profile(state).await
}

fn install_profile_content(
    db: &crate::db::Database,
    plugins_dir: &std::path::Path,
) -> Result<usize, String> {
    let created = import_examples(db.conn(), false)? + import_examples(db.conn(), true)?;
    // Install in bundle dependency order, including the editors collection.
    // Refresh existing files too, so each profile receives this build's bytes.
    for bundle in crate::plugins::builtins::BUILTIN_PLUGINS {
        let cid = crate::plugins::verifier::compute_plugin_cid(bundle.manifest_json);
        crate::plugins::registry::install_builtin(db, plugins_dir, bundle)?;
        let manifest = crate::plugins::manifest::parse_and_validate(bundle.manifest_json)?;
        let announcement = crate::plugins::catalog::announcement_from_manifest(
            &cid,
            &manifest,
            &chrono::Utc::now().to_rfc3339(),
        );
        crate::plugins::catalog::upsert_announcement(db, &announcement, "builtin")?;
    }
    Ok(created)
}

#[derive(Deserialize)]
struct Corpus {
    courses: Vec<ExampleCourse>,
}

#[derive(Deserialize)]
struct ExampleCourse {
    id: String,
    title: String,
    description: String,
    kind: String,
    tags: Vec<String>,
    skill_ids: Vec<String>,
    thumbnail_svg: Option<String>,
    chapters: Vec<ExampleChapter>,
}

#[derive(Deserialize)]
struct ExampleChapter {
    id: String,
    title: String,
    description: Option<String>,
    elements: Vec<ExampleElement>,
}

#[derive(Deserialize)]
struct ExampleElement {
    id: String,
    title: String,
    element_type: String,
    content_inline: Option<String>,
    /// A bundled lesson video (`demo_resources::lesson_videos`).
    #[serde(default)]
    media_id: Option<String>,
    #[serde(default)]
    skill_tags: Vec<ExampleSkillTag>,
}

#[derive(Deserialize)]
struct ExampleSkillTag {
    skill_id: String,
    weight: f64,
}

#[tauri::command]
pub async fn import_demo_courses(state: State<'_, AppState>) -> Result<usize, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "courses.import_examples",
            |db| import_examples(db.conn(), false),
        )
        .await
}

#[tauri::command]
pub async fn import_plugin_demo_course(state: State<'_, AppState>) -> Result<String, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Instructor,
            state.profile_lease(),
            "courses.import_plugin_example",
            |db| {
                import_examples(db.conn(), true)?;
                let author: String = db
                    .conn()
                    .query_row(
                        "SELECT stake_address FROM local_identity WHERE id=1",
                        [],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(entity_id(&[
                    "example-course-v1",
                    &author,
                    "course_plugin_demo",
                ]))
            },
        )
        .await
}

fn plugin_metadata(element: &ExampleElement) -> Result<Option<(String, String)>, String> {
    if element.element_type != "plugin" {
        return Ok(None);
    }
    let slug = match element.id.as_str() {
        "el_plugin_demo_workout" => "workout-assist",
        "el_plugin_demo_music_reviews" => "music-reviews",
        "el_plugin_demo_irl_review" | "el_eng_6_4" | "el_eng_6_5" => "irl-review",
        "el_plugin_demo_pronunciation"
        | "el_eng_lab_dictation"
        | "el_eng_lab_reading"
        | "el_eng_lab_builder"
        | "el_eng_lab_writing"
        | "el_eng_lab_read_aloud" => "language-lab",
        "el_plugin_demo_editor_js_double" => "editor-javascript",
        "el_plugin_demo_editor_ts_sum" => "editor-typescript",
        "el_plugin_demo_editor_cpp_double" => "editor-cpp",
        "el_plugin_demo_editor_python_double" => "editor-python",
        _ => return Err("Unknown example plugin element".into()),
    };
    let bundle = crate::plugins::builtins::BUILTIN_PLUGINS
        .iter()
        .find(|b| b.slug == slug)
        .ok_or("Example plugin is not bundled")?;
    let manifest = crate::plugins::manifest::parse_and_validate(bundle.manifest_json)
        .map_err(|e| e.to_string())?;
    Ok(Some((
        crate::plugins::verifier::compute_plugin_cid(bundle.manifest_json),
        manifest.version,
    )))
}

/// What one imported element stores: inline content, or a bundled video.
struct Lesson {
    kind: &'static str,
    inline: Option<String>,
    cid: Option<String>,
    duration_seconds: Option<i64>,
}

fn lesson(element: &ExampleElement) -> Result<Option<Lesson>, String> {
    if element.element_type == "video" {
        // External videos in the corpus are skipped; bundled ones import.
        let Some(media_id) = element.media_id.as_deref() else {
            return Ok(None);
        };
        let video = super::demo_resources::lesson_videos()?
            .into_iter()
            .find(|v| v.id == media_id)
            .ok_or("Example video is not bundled")?;
        return Ok(Some(Lesson {
            kind: "video",
            inline: None,
            cid: Some(super::demo_resources::media_cid(media_id)?),
            duration_seconds: Some(video.duration_seconds),
        }));
    }
    Ok(lesson_content(element)?.map(|(kind, body)| Lesson {
        kind,
        inline: Some(body),
        cid: None,
        duration_seconds: None,
    }))
}

fn lesson_content(element: &ExampleElement) -> Result<Option<(&'static str, String)>, String> {
    let Some(content) = element.content_inline.as_ref().filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    match element.element_type.as_str() {
        "text" => Ok(Some(("text", content.clone()))),
        "plugin" => {
            plugin_metadata(element)?;
            let mut value: serde_json::Value =
                serde_json::from_str(content).map_err(|e| e.to_string())?;
            if let Some(code) = value["starter_code"].as_str() {
                value["starter_code"] = code.replace("\\n", "\n").into();
            }
            serde_json::to_string(&value)
                .map(|body| Some(("plugin", body)))
                .map_err(|e| e.to_string())
        }
        "quiz" => {
            // The retired corpus uses prompt/correct_indices. Convert it to
            // the current composer/player's single-choice quiz format.
            let mut value: serde_json::Value =
                serde_json::from_str(content).map_err(|e| e.to_string())?;
            let questions = value["questions"]
                .as_array_mut()
                .ok_or("Example quiz is missing questions")?;
            let mut multi = false;
            for question in questions {
                let answers = question["correct_indices"]
                    .as_array()
                    .ok_or("Example quiz is missing its answer")?;
                if answers.is_empty() {
                    return Err("Example quiz must have a correct answer".into());
                }
                let index = answers[0].as_u64().ok_or("Invalid quiz answer index")?;
                let options = question["options"].as_array().ok_or("Missing options")?;
                if index as usize >= options.len() || !options.iter().all(|v| v.is_string()) {
                    return Err("Invalid example quiz options".into());
                }
                if answers.len() > 1 {
                    multi = true;
                    question["type"] = "multiple_choice".into();
                }
                question["question"] = question["prompt"].clone();
                question["correct_index"] = index.into();
            }
            serde_json::to_string(&value)
                .map(|body| Some((if multi { "objective_multi_mcq" } else { "quiz" }, body)))
                .map_err(|e| e.to_string())
        }
        _ => Ok(None),
    }
}

fn import_examples(conn: &rusqlite::Connection, plugin_only: bool) -> Result<usize, String> {
    import_selected(conn, plugin_only, None)
}

pub(super) fn import_selected(
    conn: &rusqlite::Connection,
    plugin_only: bool,
    selected: Option<&str>,
) -> Result<usize, String> {
    let corpus: Corpus = serde_json::from_str(CORPUS).map_err(|e| e.to_string())?;
    let author: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    crate::db::with_transaction(conn, || {
        let mut created = 0;
        for course in corpus.courses.iter().filter(|c| {
            c.kind == "course"
                && (c.id == "course_plugin_demo") == plugin_only
                && selected.is_none_or(|id| c.id == id)
        }) {
            let id = entity_id(&["example-course-v1", &author, &course.id]);
            let description = if plugin_only {
                format!("Bundled plugin example for review. {}", course.description)
            } else if course
                .chapters
                .iter()
                .flat_map(|chapter| &chapter.elements)
                .any(|element| element.element_type == "plugin" || element.media_id.is_some())
            {
                format!(
                    "AI-generated example for review. Narrated videos with synthetic voices, notes, quizzes and review tasks. {}",
                    course.description
                )
            } else {
                format!(
                    "AI-generated example for review. Inline lessons and quizzes only. {}",
                    course.description
                )
            };
            let chapters = plan_chapters(&id, course)?;
            let inserted = conn.execute(
                "INSERT OR IGNORE INTO courses (id,title,description,author_address,tags,skill_ids,thumbnail_svg,kind,status,provenance) VALUES (?1,?2,?3,?4,?5,?6,?7,'course','draft','ai_generated')",
                params![id,course.title,description,author,serde_json::to_string(&course.tags).map_err(|e|e.to_string())?,serde_json::to_string(&course.skill_ids).map_err(|e|e.to_string())?,course.thumbnail_svg],
            ).map_err(|e|e.to_string())?;
            backfill_thumbnail(conn, &id, course.thumbnail_svg.as_deref())?;
            if inserted == 1 {
                created += 1;
                write_chapters(conn, &id, &chapters)?;
            } else if refresh_wanted(conn, &id, course, &description, &chapters)? {
                conn.execute(
                    "UPDATE courses SET title=?2,description=?3,tags=?4,skill_ids=?5,updated_at=datetime('now') WHERE id=?1",
                    params![id,course.title,description,serde_json::to_string(&course.tags).map_err(|e|e.to_string())?,serde_json::to_string(&course.skill_ids).map_err(|e|e.to_string())?],
                ).map_err(|e|e.to_string())?;
                write_chapters(conn, &id, &chapters)?;
                log::info!("refreshed unedited example course {}", course.id);
            } else {
                continue;
            }
            let snapshot = db_snapshot(conn, &id)?;
            conn.execute(
                "INSERT INTO app_settings (key,value,scope) VALUES (?1,?2,'device') ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=datetime('now')",
                params![snapshot_key(&id), snapshot],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(created)
    })
}

/// One corpus element as this build imports it.
struct PlannedElement {
    id: String,
    title: String,
    lesson: Lesson,
    plugin: Option<(String, String)>,
    tags: Vec<(String, f64)>,
}

struct PlannedChapter {
    id: String,
    title: String,
    description: Option<String>,
    position: i64,
    elements: Vec<PlannedElement>,
}

fn plan_chapters(course_id: &str, course: &ExampleCourse) -> Result<Vec<PlannedChapter>, String> {
    let mut chapters = Vec::new();
    for (position, chapter) in course.chapters.iter().enumerate() {
        let id = entity_id(&[course_id, &chapter.id]);
        let mut elements = Vec::new();
        for element in &chapter.elements {
            let Some(lesson) = lesson(element)? else {
                continue;
            };
            elements.push(PlannedElement {
                id: entity_id(&[&id, &element.id]),
                title: element.title.clone(),
                lesson,
                plugin: plugin_metadata(element)?,
                tags: element
                    .skill_tags
                    .iter()
                    .map(|t| (t.skill_id.clone(), t.weight))
                    .collect(),
            });
        }
        if elements.is_empty() {
            continue;
        }
        chapters.push(PlannedChapter {
            id,
            title: chapter.title.clone(),
            description: chapter.description.clone(),
            position: position as i64,
            elements,
        });
    }
    Ok(chapters)
}

/// Insert the planned chapters and elements, or bring existing rows with the
/// same ids up to date. Upserts keep learner progress, notes and submissions,
/// which reference element ids without cascading.
fn write_chapters(
    conn: &rusqlite::Connection,
    course_id: &str,
    chapters: &[PlannedChapter],
) -> Result<(), String> {
    let mut keep = Vec::new();
    for chapter in chapters {
        conn.execute(
            "INSERT INTO course_chapters (id,course_id,title,description,position) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET title=excluded.title,description=excluded.description,position=excluded.position",
            params![chapter.id, course_id, chapter.title, chapter.description, chapter.position],
        )
        .map_err(|e| e.to_string())?;
        for (position, element) in chapter.elements.iter().enumerate() {
            let lesson = &element.lesson;
            conn.execute(
                "INSERT INTO course_elements (id,chapter_id,title,element_type,content_inline,position,plugin_cid,plugin_version,content_cid,duration_seconds) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(id) DO UPDATE SET chapter_id=excluded.chapter_id,title=excluded.title,element_type=excluded.element_type,content_inline=excluded.content_inline,position=excluded.position,plugin_cid=excluded.plugin_cid,plugin_version=excluded.plugin_version,content_cid=excluded.content_cid,duration_seconds=excluded.duration_seconds",
                params![element.id,chapter.id,element.title,lesson.kind,lesson.inline,position as i64,element.plugin.as_ref().map(|p| &p.0),element.plugin.as_ref().map(|p| &p.1),lesson.cid,lesson.duration_seconds],
            )
            .map_err(|e| e.to_string())?;
            conn.execute(
                "DELETE FROM element_skill_tags WHERE element_id=?1",
                [&element.id],
            )
            .map_err(|e| e.to_string())?;
            for (skill_id, weight) in &element.tags {
                conn.execute(
                    "INSERT INTO element_skill_tags (element_id,skill_id,weight) VALUES (?1,?2,?3)",
                    params![element.id, skill_id, weight],
                )
                .map_err(|e| e.to_string())?;
            }
            keep.push(element.id.clone());
        }
    }
    // Elements the corpus no longer has go, unless a learner record points at
    // them; chapters left empty go with them.
    let keep = serde_json::to_string(&keep).map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM course_elements WHERE chapter_id IN (SELECT id FROM course_chapters WHERE course_id=?1)
           AND id NOT IN (SELECT value FROM json_each(?2))
           AND id NOT IN (SELECT element_id FROM element_progress)
           AND id NOT IN (SELECT element_id FROM element_submissions)
           AND id NOT IN (SELECT element_id FROM course_notes WHERE element_id IS NOT NULL)",
        params![course_id, keep],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM course_chapters WHERE course_id=?1
           AND id NOT IN (SELECT chapter_id FROM course_elements)
           AND id NOT IN (SELECT chapter_id FROM course_notes WHERE chapter_id IS NOT NULL)",
        [course_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn snapshot_key(course_id: &str) -> String {
    format!("example_course_snapshot:{course_id}")
}

/// Hash of an example course's authored rows. Plugin CIDs and versions are
/// left out because they follow the bundled plugin bytes, not the author.
fn db_snapshot(conn: &rusqlite::Connection, course_id: &str) -> Result<String, String> {
    let mut lines: Vec<String> = Vec::new();
    let course: (String, Option<String>) = conn
        .query_row(
            "SELECT title,description FROM courses WHERE id=?1",
            [course_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    lines.push(serde_json::json!(["course", course.0, course.1]).to_string());
    let mut chapters = conn
        .prepare("SELECT id,title,description,position FROM course_chapters WHERE course_id=?1")
        .map_err(|e| e.to_string())?;
    for row in chapters
        .query_map([course_id], |r| {
            Ok(serde_json::json!([
                "chapter",
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?
            ]))
        })
        .map_err(|e| e.to_string())?
    {
        lines.push(row.map_err(|e| e.to_string())?.to_string());
    }
    let mut elements = conn
        .prepare("SELECT e.id,e.chapter_id,e.title,e.element_type,e.content_inline,e.content_cid,e.duration_seconds,e.position FROM course_elements e JOIN course_chapters c ON c.id=e.chapter_id WHERE c.course_id=?1")
        .map_err(|e| e.to_string())?;
    for row in elements
        .query_map([course_id], |r| {
            Ok(serde_json::json!([
                "element",
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<i64>>(6)?,
                r.get::<_, i64>(7)?
            ]))
        })
        .map_err(|e| e.to_string())?
    {
        lines.push(row.map_err(|e| e.to_string())?.to_string());
    }
    Ok(hash_lines(lines))
}

/// [`db_snapshot`] of the rows [`write_chapters`] would leave behind.
fn planned_snapshot(title: &str, description: &str, chapters: &[PlannedChapter]) -> String {
    let mut lines = vec![serde_json::json!(["course", title, description]).to_string()];
    for chapter in chapters {
        lines.push(
            serde_json::json!([
                "chapter",
                chapter.id,
                chapter.title,
                chapter.description,
                chapter.position
            ])
            .to_string(),
        );
        for (position, element) in chapter.elements.iter().enumerate() {
            let lesson = &element.lesson;
            lines.push(
                serde_json::json!([
                    "element",
                    element.id,
                    chapter.id,
                    element.title,
                    lesson.kind,
                    lesson.inline,
                    lesson.cid,
                    lesson.duration_seconds,
                    position as i64
                ])
                .to_string(),
            );
        }
    }
    hash_lines(lines)
}

fn hash_lines(mut lines: Vec<String>) -> String {
    lines.sort();
    blake3::hash(lines.join("\n").as_bytes())
        .to_hex()
        .to_string()
}

/// Whether an existing example draft should be brought up to this build's
/// corpus: only while nobody has edited it since it was imported.
fn refresh_wanted(
    conn: &rusqlite::Connection,
    course_id: &str,
    course: &ExampleCourse,
    description: &str,
    chapters: &[PlannedChapter],
) -> Result<bool, String> {
    let (title, status): (String, String) = conn
        .query_row(
            "SELECT title,status FROM courses WHERE id=?1",
            [course_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    if status != "draft" {
        return Ok(false);
    }
    let current = db_snapshot(conn, course_id)?;
    let recorded: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key=?1",
            [snapshot_key(course_id)],
            |r| r.get(0),
        )
        .ok();
    if let Some(recorded) = recorded {
        // Edited since import: keep the author's version.
        return Ok(recorded == current
            && current != planned_snapshot(&course.title, description, chapters));
    }
    // Imported before snapshots were recorded. Treat it as unedited when the
    // course title matches and every chapter and element is still a corpus
    // row under its corpus title.
    if title != course.title {
        return Ok(false);
    }
    let planned: std::collections::HashMap<&str, &str> = chapters
        .iter()
        .map(|c| (c.id.as_str(), c.title.as_str()))
        .chain(
            chapters
                .iter()
                .flat_map(|c| &c.elements)
                .map(|e| (e.id.as_str(), e.title.as_str())),
        )
        .collect();
    let mut rows = conn
        .prepare("SELECT id,title FROM course_chapters WHERE course_id=?1 UNION ALL SELECT e.id,e.title FROM course_elements e JOIN course_chapters c ON c.id=e.chapter_id WHERE c.course_id=?1")
        .map_err(|e| e.to_string())?;
    let rows = rows
        .query_map([course_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows
        .iter()
        .all(|(id, title)| planned.get(id.as_str()) == Some(&title.as_str())))
}

pub(crate) fn backfill_thumbnail(
    conn: &rusqlite::Connection,
    id: &str,
    svg: Option<&str>,
) -> Result<(), String> {
    conn.execute("UPDATE courses SET thumbnail_svg=?2 WHERE id=?1 AND COALESCE(trim(thumbnail_svg),'')='' AND COALESCE(trim(thumbnail_cid),'')=''",params![id,svg]).map_err(|e|e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_profile_gets_owned_content_and_plugins_without_learner_results() {
        let mut course_ids = Vec::new();
        for author in ["profile-one", "profile-two"] {
            let db = crate::db::Database::open_in_memory().unwrap();
            db.run_migrations().unwrap();
            crate::db::bundled::install_bundled_data(db.conn()).unwrap();
            db.conn().execute("INSERT INTO local_identity (id,stake_address,payment_address) VALUES (1,?1,'key')", [author]).unwrap();
            let dir = tempfile::tempdir().unwrap();
            assert_eq!(install_profile_content(&db, dir.path()).unwrap(), 10);
            let id = entity_id(&["example-course-v1", author, "course_algo_101"]);
            db.conn()
                .execute("UPDATE courses SET title='Keep my edit' WHERE id=?1", [&id])
                .unwrap();
            assert_eq!(install_profile_content(&db, dir.path()).unwrap(), 0);
            let title: String = db
                .conn()
                .query_row("SELECT title FROM courses WHERE id=?1", [&id], |r| r.get(0))
                .unwrap();
            assert_eq!(title, "Keep my edit");
            let owned: i64 = db
                .conn()
                .query_row(
                    "SELECT count(*) FROM courses WHERE author_address=?1 AND status='draft'",
                    [author],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(owned, 10);
            for bundle in crate::plugins::builtins::BUILTIN_PLUGINS {
                let cid = crate::plugins::verifier::compute_plugin_cid(bundle.manifest_json);
                let plugin = crate::plugins::registry::get_installed(&db, &cid)
                    .unwrap()
                    .unwrap();
                assert!(std::path::Path::new(&plugin.install_path).starts_with(dir.path()));
                assert_eq!(
                    std::fs::read(dir.path().join(&cid).join("manifest.json")).unwrap(),
                    bundle.manifest_json
                );
            }
            let results: i64 = db.conn().query_row("SELECT (SELECT count(*) FROM credentials) + (SELECT count(*) FROM enrollments)", [], |r| r.get(0)).unwrap();
            assert_eq!(results, 0);
            course_ids.push(id);
        }
        assert_ne!(course_ids[0], course_ids[1]);
    }

    #[test]
    fn plugin_showcase_uses_current_bundles_and_preserves_edits() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        crate::db::bundled::install_bundled_data(db.conn()).unwrap();
        db.conn().execute("INSERT INTO local_identity (id,stake_address,payment_address) VALUES (1,'plugin-owner','demo-key')", []).unwrap();
        assert_eq!(import_examples(db.conn(), true).unwrap(), 1);
        let mut query = db.conn().prepare("SELECT element_type,plugin_cid,plugin_version,content_inline FROM course_elements ORDER BY id").unwrap();
        let elements = query
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(elements.len(), 8);
        let mut read_aloud = 0;
        let mut editors = 0;
        for (kind, cid, version, content) in elements {
            assert_eq!(kind, "plugin");
            let bundle =
                crate::plugins::builtins::find_bundle_by_cid(&cid).expect("current bundled CID");
            let manifest =
                crate::plugins::manifest::parse_and_validate(bundle.manifest_json).unwrap();
            assert_eq!(version, manifest.version);
            let config: serde_json::Value = serde_json::from_str(&content).unwrap();
            if bundle.slug == "language-lab" {
                assert_eq!(config["activity"], "read_aloud");
                assert_eq!(config["lang"], "en");
                let items = config["items"].as_array().unwrap();
                assert!(!items.is_empty());
                assert!(items
                    .iter()
                    .all(|item| item["text"].as_str().is_some_and(|s| !s.trim().is_empty())));
                read_aloud += 1;
            }
            if let Some(code) = config["starter_code"].as_str() {
                editors += 1;
                assert!(code.contains('\n'));
                assert!(!code.contains("\\n"));
                assert!(!config["grader_private"]["tests"]
                    .as_array()
                    .unwrap()
                    .is_empty());
                assert!(bundle.grader_wasm.is_some());
            }
        }
        assert_eq!(read_aloud, 1);
        assert_eq!(editors, 4);
        let id = entity_id(&["example-course-v1", "plugin-owner", "course_plugin_demo"]);
        // An earlier showcase has no pronunciation chapter or recorded snapshot.
        let chapter_id = entity_id(&[&id, "ch_plugin_demo_language_lab"]);
        let imported = db_snapshot(db.conn(), &id).unwrap();
        db.conn()
            .execute("DELETE FROM course_chapters WHERE id=?1", [&chapter_id])
            .unwrap();
        db.conn()
            .execute("DELETE FROM app_settings WHERE key=?1", [snapshot_key(&id)])
            .unwrap();
        assert_ne!(db_snapshot(db.conn(), &id).unwrap(), imported);
        assert_eq!(import_examples(db.conn(), true).unwrap(), 0);
        assert_eq!(db_snapshot(db.conn(), &id).unwrap(), imported);
        db.conn()
            .execute("UPDATE courses SET title='My showcase' WHERE id=?1", [&id])
            .unwrap();
        assert_eq!(import_examples(db.conn(), true).unwrap(), 0);
        let (title, author, status): (String, String, String) = db
            .conn()
            .query_row(
                "SELECT title,author_address,status FROM courses WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (title.as_str(), author.as_str(), status.as_str()),
            ("My showcase", "plugin-owner", "draft")
        );
    }

    #[test]
    fn examples_are_owned_drafts_and_reimport_preserves_edits() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        crate::db::bundled::install_bundled_data(db.conn()).unwrap();
        db.conn().execute("INSERT INTO local_identity (id,stake_address,payment_address) VALUES (1,'demo-owner','demo-key')",[]).unwrap();
        assert_eq!(import_examples(db.conn(), false).unwrap(), 9);
        let bad:i64=db.conn().query_row("SELECT count(*) FROM courses WHERE author_address!='demo-owner' OR status!='draft' OR provenance!='ai_generated' OR content_cid IS NOT NULL",[],|r|r.get(0)).unwrap();
        assert_eq!(bad, 0);
        let external:i64=db.conn().query_row("SELECT count(*) FROM course_elements WHERE element_type NOT IN ('text','quiz','objective_multi_mcq','plugin','video') OR (content_cid IS NOT NULL) != (element_type='video')",[],|r|r.get(0)).unwrap();
        assert_eq!(external, 0);
        // Every imported video is a bundled lesson video, with its duration.
        let bundled = crate::commands::demo_resources::lesson_videos().unwrap();
        let mut query = db
            .conn()
            .prepare("SELECT content_cid,duration_seconds FROM course_elements WHERE element_type='video'")
            .unwrap();
        let videos: Vec<(String, i64)> = query
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(videos.len(), bundled.len());
        for (cid, seconds) in videos {
            let video = bundled
                .iter()
                .find(|v| crate::commands::demo_resources::media_cid(&v.id).unwrap() == cid)
                .expect("bundled video");
            assert_eq!(seconds, video.duration_seconds);
        }
        // Plugin elements outside the showcase are the English course's two
        // IRL Review speaking tasks and its five Language Lab activities, all
        // on the current bundled plugins.
        let mut query = db
            .conn()
            .prepare("SELECT plugin_cid,content_inline FROM course_elements WHERE element_type='plugin' ORDER BY id")
            .unwrap();
        let rows: Vec<(Option<String>, String)> = query
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut slugs = Vec::new();
        let mut activities = Vec::new();
        for (cid, content) in rows {
            let bundle = crate::plugins::builtins::find_bundle_by_cid(&cid.expect("plugin CID"))
                .expect("current bundled CID");
            slugs.push(bundle.slug);
            if bundle.slug == "language-lab" {
                let config: serde_json::Value = serde_json::from_str(&content).unwrap();
                activities.push(config["activity"].as_str().unwrap().to_owned());
            }
        }
        slugs.sort_unstable();
        assert_eq!(
            slugs,
            [
                "irl-review",
                "irl-review",
                "language-lab",
                "language-lab",
                "language-lab",
                "language-lab",
                "language-lab",
            ]
        );
        activities.sort();
        assert_eq!(
            activities,
            ["builder", "dictation", "read_aloud", "reading", "writing"]
        );
        let id = entity_id(&["example-course-v1", "demo-owner", "course_algo_101"]);
        db.conn()
            .execute(
                "UPDATE courses SET title='My edited course' WHERE id=?1",
                [&id],
            )
            .unwrap();
        assert_eq!(import_examples(db.conn(), false).unwrap(), 0);
        let title: String = db
            .conn()
            .query_row("SELECT title FROM courses WHERE id=?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "My edited course");
        let quiz: String = db
            .conn()
            .query_row(
                "SELECT content_inline FROM course_elements WHERE element_type='quiz' LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let quiz: serde_json::Value = serde_json::from_str(&quiz).unwrap();
        assert!(quiz["questions"][0]["question"].is_string());
        assert!(quiz["questions"][0]["correct_index"].is_u64());
    }

    #[test]
    fn unedited_examples_follow_the_corpus_and_edited_ones_stay() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        crate::db::bundled::install_bundled_data(db.conn()).unwrap();
        let conn = db.conn();
        conn.execute("INSERT INTO local_identity (id,stake_address,payment_address) VALUES (1,'demo-owner','demo-key')",[]).unwrap();
        assert_eq!(import_examples(conn, false).unwrap(), 9);
        let id = entity_id(&[
            "example-course-v1",
            "demo-owner",
            "course_english_hindi_101",
        ]);
        let videos = |conn: &rusqlite::Connection| -> i64 {
            conn.query_row("SELECT count(*) FROM course_elements e JOIN course_chapters c ON c.id=e.chapter_id WHERE c.course_id=?1 AND e.element_type='video'", [&id], |r| r.get(0)).unwrap()
        };
        let drop_videos = |conn: &rusqlite::Connection| {
            conn.execute("DELETE FROM course_elements WHERE element_type='video' AND chapter_id IN (SELECT id FROM course_chapters WHERE course_id=?1)", [&id]).unwrap();
        };
        let record = |conn: &rusqlite::Connection| {
            conn.execute(
                "UPDATE app_settings SET value=?2 WHERE key=?1",
                params![snapshot_key(&id), db_snapshot(conn, &id).unwrap()],
            )
            .unwrap();
        };
        assert_eq!(videos(conn), 21);
        let imported = db_snapshot(conn, &id).unwrap();

        // A copy imported before snapshots existed, without the videos.
        drop_videos(conn);
        conn.execute("DELETE FROM app_settings WHERE key=?1", [snapshot_key(&id)])
            .unwrap();
        assert_eq!(import_examples(conn, false).unwrap(), 0);
        assert_eq!(videos(conn), 21);
        assert_eq!(db_snapshot(conn, &id).unwrap(), imported);

        // A copy recorded by an earlier build, left as imported.
        drop_videos(conn);
        record(conn);
        assert_eq!(import_examples(conn, false).unwrap(), 0);
        assert_eq!(videos(conn), 21);
        let first: String = conn.query_row("SELECT e.element_type FROM course_elements e JOIN course_chapters c ON c.id=e.chapter_id WHERE c.course_id=?1 ORDER BY c.position,e.position LIMIT 1", [&id], |r| r.get(0)).unwrap();
        assert_eq!(first, "video");

        // An edited copy keeps the author's version.
        drop_videos(conn);
        record(conn);
        conn.execute(
            "UPDATE courses SET title='My English course' WHERE id=?1",
            [&id],
        )
        .unwrap();
        assert_eq!(import_examples(conn, false).unwrap(), 0);
        assert_eq!(videos(conn), 0);
        let title: String = conn
            .query_row("SELECT title FROM courses WHERE id=?1", [&id], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "My English course");
    }
}
