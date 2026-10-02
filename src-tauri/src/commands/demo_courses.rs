use rusqlite::params;
use serde::Deserialize;

use crate::crypto::hash::entity_id;
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

const CORPUS: &str = include_str!("../../../demo-world/content/courses.json");

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
            |db| import_examples(db.conn()),
        )
        .await
}

fn lesson_content(element: &ExampleElement) -> Result<Option<(&'static str, String)>, String> {
    let Some(content) = element.content_inline.as_ref().filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    match element.element_type.as_str() {
        "text" => Ok(Some(("text", content.clone()))),
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

fn import_examples(conn: &rusqlite::Connection) -> Result<usize, String> {
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
        for course in corpus
            .courses
            .iter()
            .filter(|c| c.kind == "course" && c.id != "course_plugin_demo")
        {
            let id = entity_id(&["example-course-v1", &author, &course.id]);
            let inserted = conn.execute(
                "INSERT OR IGNORE INTO courses (id,title,description,author_address,tags,skill_ids,thumbnail_svg,kind,status,provenance) VALUES (?1,?2,?3,?4,?5,?6,?7,'course','draft','ai_generated')",
                params![id,course.title,format!("AI-generated example for review. Inline lessons and quizzes only. {}",course.description),author,serde_json::to_string(&course.tags).map_err(|e|e.to_string())?,serde_json::to_string(&course.skill_ids).map_err(|e|e.to_string())?,course.thumbnail_svg],
            ).map_err(|e|e.to_string())?;
            if inserted == 0 {
                continue;
            }
            created += 1;
            for (chapter_position, chapter) in course.chapters.iter().enumerate() {
                let chapter_id = entity_id(&[&id, &chapter.id]);
                let lessons = chapter
                    .elements
                    .iter()
                    .map(|element| lesson_content(element).map(|body| (element, body)))
                    .collect::<Result<Vec<_>, _>>()?;
                if lessons.iter().all(|(_, body)| body.is_none()) {
                    continue;
                }
                conn.execute("INSERT INTO course_chapters (id,course_id,title,description,position) VALUES (?1,?2,?3,?4,?5)",params![chapter_id,id,chapter.title,chapter.description,chapter_position as i64]).map_err(|e|e.to_string())?;
                for (position, (element, body)) in lessons
                    .into_iter()
                    .filter(|(_, body)| body.is_some())
                    .enumerate()
                {
                    let element_id = entity_id(&[&chapter_id, &element.id]);
                    conn.execute("INSERT INTO course_elements (id,chapter_id,title,element_type,content_inline,position) VALUES (?1,?2,?3,?4,?5,?6)",params![element_id,chapter_id,element.title,body.as_ref().map(|(kind,_)|*kind),body.as_ref().map(|(_,content)|content),position as i64]).map_err(|e|e.to_string())?;
                    for tag in &element.skill_tags {
                        conn.execute("INSERT INTO element_skill_tags (element_id,skill_id,weight) VALUES (?1,?2,?3)",params![element_id,tag.skill_id,tag.weight]).map_err(|e|e.to_string())?;
                    }
                }
            }
        }
        Ok(created)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_are_owned_drafts_and_reimport_preserves_edits() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        crate::db::bundled::install_bundled_data(db.conn()).unwrap();
        db.conn().execute("INSERT INTO local_identity (id,stake_address,payment_address) VALUES (1,'demo-owner','demo-key')",[]).unwrap();
        assert_eq!(import_examples(db.conn()).unwrap(), 7);
        let bad:i64=db.conn().query_row("SELECT count(*) FROM courses WHERE author_address!='demo-owner' OR status!='draft' OR provenance!='ai_generated' OR content_cid IS NOT NULL",[],|r|r.get(0)).unwrap();
        assert_eq!(bad, 0);
        let external:i64=db.conn().query_row("SELECT count(*) FROM course_elements WHERE element_type NOT IN ('text','quiz','objective_multi_mcq') OR content_cid IS NOT NULL",[],|r|r.get(0)).unwrap();
        assert_eq!(external, 0);
        let id = entity_id(&["example-course-v1", "demo-owner", "course_algo_101"]);
        db.conn()
            .execute(
                "UPDATE courses SET title='My edited course' WHERE id=?1",
                [&id],
            )
            .unwrap();
        assert_eq!(import_examples(db.conn()).unwrap(), 0);
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
}
