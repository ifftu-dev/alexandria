use rusqlite::params;
use serde::Deserialize;

use crate::crypto::hash::entity_id;
use crate::db::executor::DatabaseWorkload;
use crate::domain::opinions::OpinionExample;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

const RESOURCES: &str = include_str!("../../../demo-world/content/resources.json");

macro_rules! media {
    ($($id:literal),+ $(,)?) => {
        const MEDIA: &[(&str, &[u8])] = &[$(($id, include_bytes!(concat!("../../../demo-world/content/videos/", $id, ".mp4")))),+];
    };
}
media!(
    "counting",
    "binary-search",
    "merge-sort",
    "hash-tables",
    "http",
    "state",
    "validation",
    "sql",
    "op_cs_01",
    "op_cs_02",
    "op_web_01",
    "op_cyber_01",
    "op_design_01",
    "op_design_02",
    "op_civics_01"
);

const THUMBNAILS: &[(&str, &[u8])] = &[
    (
        "op_cs_01",
        include_bytes!("../../../demo-world/content/thumbnails/op_cs_01.jpg"),
    ),
    (
        "op_cs_02",
        include_bytes!("../../../demo-world/content/thumbnails/op_cs_02.jpg"),
    ),
    (
        "op_web_01",
        include_bytes!("../../../demo-world/content/thumbnails/op_web_01.jpg"),
    ),
    (
        "op_cyber_01",
        include_bytes!("../../../demo-world/content/thumbnails/op_cyber_01.jpg"),
    ),
    (
        "op_design_01",
        include_bytes!("../../../demo-world/content/thumbnails/op_design_01.jpg"),
    ),
    (
        "op_design_02",
        include_bytes!("../../../demo-world/content/thumbnails/op_design_02.jpg"),
    ),
    (
        "op_civics_01",
        include_bytes!("../../../demo-world/content/thumbnails/op_civics_01.jpg"),
    ),
];

#[derive(Deserialize)]
struct Resources {
    media: Vec<Media>,
    opinions: Vec<ExampleOpinion>,
    courses: Vec<VideoCourse>,
    classrooms: Vec<Classroom>,
}
#[derive(Deserialize)]
struct Media {
    id: String,
    title: String,
    skill_id: Option<String>,
    duration_seconds: i64,
    scenes: Vec<Scene>,
}
#[derive(Deserialize)]
struct Scene {
    title: String,
    body: String,
}
#[derive(Deserialize)]
struct ExampleOpinion {
    id: String,
    subject_field_id: String,
    title: String,
    summary: String,
    media_id: String,
}
#[derive(Deserialize)]
struct VideoCourse {
    id: String,
    title: String,
    description: String,
    lesson_ids: Vec<String>,
    thumbnail_svg: String,
}
#[derive(Deserialize)]
struct Classroom {
    id: String,
    name: String,
    description: String,
    channels: Vec<String>,
}

fn media_bytes(id: &str) -> Result<&'static [u8], String> {
    MEDIA
        .iter()
        .find(|(key, _)| *key == id)
        .map(|(_, bytes)| *bytes)
        .ok_or_else(|| format!("missing bundled video: {id}"))
}
fn media_cid(id: &str) -> Result<String, String> {
    Ok(blake3::hash(media_bytes(id)?).to_hex().to_string())
}
fn html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) async fn install_for_profile(state: &AppState) -> Result<(), String> {
    use crate::content_store::content;
    for (_id, bytes) in MEDIA.iter().chain(THUMBNAILS) {
        let cid = blake3::hash(bytes).to_hex().to_string();
        if !content::has(&state.content_node, &cid)
            .await
            .map_err(|e| e.to_string())?
        {
            // These are original bundled examples, safe to address by their
            // public bytes. No profile data is included in a teaching clip.
            content::add_bytes_unencrypted(&state.content_node, bytes)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    let database = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let guard = database.lock().map_err(|e| e.to_string())?;
        let db = guard.as_ref().ok_or("database not initialized")?;
        install_rows(db.conn())?;
        log::info!("profile resources ready: 7 opinion examples, 3 classrooms, 2 video courses, 15 local videos");
        Ok::<_, String>(())
    }).await.map_err(|e| format!("resource install task failed: {e}"))?
}

fn install_rows(conn: &rusqlite::Connection) -> Result<(), String> {
    let resources: Resources = serde_json::from_str(RESOURCES).map_err(|e| e.to_string())?;
    let author: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    crate::db::with_transaction(conn, || {
        for item in &resources.media {
            let cid = media_cid(&item.id)?;
            crate::content_store::storage::upsert_pin(
                conn,
                &cid,
                "course",
                media_bytes(&item.id)?.len() as u64,
                false,
            )
            .map_err(|e| e.to_string())?;
        }
        for (id, bytes) in THUMBNAILS {
            let cid = blake3::hash(bytes).to_hex().to_string();
            crate::content_store::storage::upsert_pin(
                conn,
                &cid,
                "opinion",
                bytes.len() as u64,
                false,
            )
            .map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE demo_opinion_examples SET thumbnail_cid=?2 WHERE id=?1",
                params![id, cid],
            )
            .map_err(|e| e.to_string())?;
        }
        for opinion in &resources.opinions {
            let media = resources
                .media
                .iter()
                .find(|m| m.id == opinion.media_id)
                .ok_or("opinion media missing")?;
            conn.execute("INSERT OR IGNORE INTO demo_opinion_examples (id,subject_field_id,title,summary,video_cid,duration_seconds) VALUES (?1,?2,?3,?4,?5,?6)", params![opinion.id,opinion.subject_field_id,opinion.title,opinion.summary,media_cid(&media.id)?,media.duration_seconds]).map_err(|e| e.to_string())?;
        }
        for (id, bytes) in THUMBNAILS {
            conn.execute(
                "UPDATE demo_opinion_examples SET thumbnail_cid=?2 WHERE id=?1",
                params![id, blake3::hash(bytes).to_hex().to_string()],
            )
            .map_err(|e| e.to_string())?;
        }
        for course in &resources.courses {
            let id = entity_id(&["example-video-course-v1", &author, &course.id]);
            let lessons = course
                .lesson_ids
                .iter()
                .map(|key| {
                    resources
                        .media
                        .iter()
                        .find(|m| &m.id == key)
                        .ok_or("course video missing")
                })
                .collect::<Result<Vec<_>, _>>()?;
            let skills: Vec<&str> = lessons
                .iter()
                .filter_map(|m| m.skill_id.as_deref())
                .collect();
            let inserted = conn.execute("INSERT OR IGNORE INTO courses (id,title,description,author_address,tags,skill_ids,kind,status,provenance) VALUES (?1,?2,?3,?4,'[\"video\",\"bundled\"]',?5,'course','draft','ai_generated')", params![id,course.title,format!("AI-generated teaching videos with synthetic narration. {}",course.description),author,serde_json::to_string(&skills).map_err(|e|e.to_string())?]).map_err(|e| e.to_string())?;
            crate::commands::demo_courses::backfill_thumbnail(
                conn,
                &id,
                Some(&course.thumbnail_svg),
            )?;
            if inserted == 0 {
                continue;
            }
            let chapter_id = entity_id(&[&id, "lessons"]);
            conn.execute("INSERT INTO course_chapters (id,course_id,title,position) VALUES (?1,?2,'Video lessons and transcripts',0)",params![chapter_id,id]).map_err(|e| e.to_string())?;
            for (position, lesson) in lessons.iter().enumerate() {
                let element_id = entity_id(&[&chapter_id, &lesson.id]);
                conn.execute("INSERT INTO course_elements (id,chapter_id,title,element_type,content_cid,position,duration_seconds) VALUES (?1,?2,?3,'video',?4,?5,?6)",params![element_id,chapter_id,lesson.title,media_cid(&lesson.id)?,(position*2) as i64,lesson.duration_seconds]).map_err(|e|e.to_string())?;
                if let Some(skill) = &lesson.skill_id {
                    conn.execute("INSERT INTO element_skill_tags (element_id,skill_id,weight) VALUES (?1,?2,1.0)",params![element_id,skill]).map_err(|e|e.to_string())?;
                }
                let transcript = lesson
                    .scenes
                    .iter()
                    .map(|s| format!("<h2>{}</h2><p>{}</p>", html(&s.title), html(&s.body)))
                    .collect::<Vec<_>>()
                    .join("\n");
                let transcript_id = entity_id(&[&element_id, "transcript"]);
                conn.execute("INSERT INTO course_elements (id,chapter_id,title,element_type,content_inline,position) VALUES (?1,?2,?3,'text',?4,?5)",params![transcript_id,chapter_id,format!("{} — transcript",lesson.title),transcript,(position*2+1) as i64]).map_err(|e|e.to_string())?;
            }
        }
        for room in &resources.classrooms {
            let id = entity_id(&["example-classroom-v1", &author, &room.id]);
            let invite = format!("{:08X}", rand::random::<u32>());
            let inserted = conn.execute("INSERT OR IGNORE INTO classrooms (id,name,description,owner_address,invite_code) VALUES (?1,?2,?3,?4,?5)",params![id,format!("{} (Example)",room.name),room.description,author,invite]).map_err(|e|e.to_string())?;
            if inserted == 0 {
                continue;
            }
            conn.execute("INSERT INTO classroom_members (classroom_id,stake_address,role) VALUES (?1,?2,'owner')",params![id,author]).map_err(|e|e.to_string())?;
            for (position, name) in room.channels.iter().enumerate() {
                let channel_id = entity_id(&[&id, name]);
                conn.execute("INSERT INTO classroom_channels (id,classroom_id,name,channel_type,position) VALUES (?1,?2,?3,'text',?4)",params![channel_id,id,name,position as i64]).map_err(|e|e.to_string())?;
            }
        }
        Ok(())
    })
}

#[tauri::command]
pub async fn list_demo_opinions(state: State<'_, AppState>) -> Result<Vec<OpinionExample>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "opinions.list-examples",
            |db| list_examples(db.conn()),
        )
        .await
}

fn list_examples(conn: &rusqlite::Connection) -> Result<Vec<OpinionExample>, String> {
    let mut stmt = conn.prepare("SELECT id,subject_field_id,title,summary,video_cid,duration_seconds,thumbnail_cid FROM demo_opinion_examples ORDER BY subject_field_id,id").map_err(|e|e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(OpinionExample {
                id: r.get(0)?,
                subject_field_id: r.get(1)?,
                title: r.get(2)?,
                summary: r.get(3)?,
                video_cid: r.get(4)?,
                duration_seconds: r.get(5)?,
                thumbnail_cid: r.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resources_are_local_owned_idempotent_and_have_real_bundled_media() {
        for author in ["first-profile", "second-profile"] {
            let db = crate::db::Database::open_in_memory().unwrap();
            db.run_migrations().unwrap();
            crate::db::bundled::install_bundled_data(db.conn()).unwrap();
            db.conn().execute("INSERT INTO local_identity (id,stake_address,payment_address) VALUES (1,?1,'key')",[author]).unwrap();
            install_rows(db.conn()).unwrap();
            let count = |table: &str| {
                db.conn()
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                        r.get::<_, i64>(0)
                    })
                    .unwrap()
            };
            assert_eq!(list_examples(db.conn()).unwrap().len(), 7);
            assert_eq!(count("classrooms"), 3);
            assert_eq!(count("classroom_members"), 3);
            assert_eq!(count("classroom_channels"), 7);
            assert_eq!(count("courses"), 2);
            assert_eq!(count("course_elements"), 16);
            assert_eq!(count("pins"), 22);
            for table in [
                "opinions",
                "credentials",
                "enrollments",
                "tutoring_sessions",
                "classroom_messages",
                "pinboard_observations",
            ] {
                assert_eq!(count(table), 0, "unexpected rows in {table}");
            }
            let owner: String = db
                .conn()
                .query_row("SELECT owner_address FROM classrooms LIMIT 1", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(owner, author);
            db.conn()
                .execute("UPDATE courses SET title='My edited title'", [])
                .unwrap();
            db.conn()
                .execute("UPDATE classrooms SET name='My edited room'", [])
                .unwrap();
            db.conn()
                .execute("UPDATE courses SET thumbnail_svg=NULL", [])
                .unwrap();
            install_rows(db.conn()).unwrap();
            let thumbnails: i64 = db
                .conn()
                .query_row(
                    "SELECT count(*) FROM courses WHERE thumbnail_svg LIKE '<svg%'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(thumbnails, 2);
            db.conn()
                .execute("UPDATE courses SET thumbnail_svg='custom artwork'", [])
                .unwrap();
            install_rows(db.conn()).unwrap();
            let custom: i64 = db
                .conn()
                .query_row(
                    "SELECT count(*) FROM courses WHERE thumbnail_svg='custom artwork'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(custom, 2);
            assert_eq!(count("courses"), 2);
            assert_eq!(count("classrooms"), 3);
            let edits: i64 = db
                .conn()
                .query_row(
                    "SELECT count(*) FROM courses WHERE title='My edited title'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(edits, 2);
            for example in list_examples(db.conn()).unwrap() {
                assert!(example.duration_seconds > 10);
                assert!(MEDIA
                    .iter()
                    .any(|(_, bytes)| blake3::hash(bytes).to_hex().as_str() == example.video_cid));
            }
        }
    }
}
