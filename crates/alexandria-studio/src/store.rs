use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

use crate::model::*;
use crate::{Error, Result};

pub const SCHEMA: &str = include_str!("schema.sql");

pub fn owns_course(conn: &Connection, id: &str) -> Result<()> {
    let owns: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM courses c JOIN local_identity i ON i.id=1 AND c.author_address=i.stake_address WHERE c.id=?1)",
        [id], |r| r.get(0),
    )?;
    if owns {
        Ok(())
    } else {
        Err(Error::Permission)
    }
}

pub fn get<T: DeserializeOwned>(
    conn: &Connection,
    kind: &str,
    id: &str,
) -> Result<Option<StudioDocument<T>>> {
    let row: Option<(i64, String)> = conn
        .query_row(
            "SELECT revision,value FROM studio_documents WHERE kind=?1 AND id=?2",
            params![kind, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    row.map(|(revision, value)| {
        Ok(StudioDocument {
            id: id.into(),
            revision,
            value: serde_json::from_str(&value)?,
        })
    })
    .transpose()
}

pub fn list<T: DeserializeOwned>(conn: &Connection, kind: &str) -> Result<Vec<StudioDocument<T>>> {
    let mut stmt=conn.prepare("SELECT id,revision,value FROM studio_documents WHERE kind=?1 ORDER BY rowid DESC LIMIT 200")?;
    let rows = stmt.query_map([kind], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (id, revision, value) = row?;
        Ok(StudioDocument {
            id,
            revision,
            value: serde_json::from_str(&value)?,
        })
    })
    .collect()
}

pub fn put<T: Serialize + Clone>(
    conn: &Connection,
    kind: &str,
    id: &str,
    expected: i64,
    value: &T,
) -> Result<StudioDocument<T>> {
    bounded(id, 200, "Identifier")?;
    if id.is_empty() || expected < 0 {
        return Err(Error::Invalid("Invalid identifier or revision".into()));
    }
    let json = serde_json::to_string(value)?;
    bounded(&json, 2_000_000, "Document")?;
    let changed = if expected == 0 {
        conn.execute("INSERT INTO studio_documents(kind,id,revision,value) VALUES (?1,?2,1,?3) ON CONFLICT DO NOTHING",params![kind,id,json])?
    } else {
        conn.execute("UPDATE studio_documents SET revision=revision+1,value=?1 WHERE kind=?2 AND id=?3 AND revision=?4",params![json,kind,id,expected])?
    };
    if changed != 1 {
        return Err(Error::Conflict);
    }
    Ok(StudioDocument {
        id: id.into(),
        revision: expected + 1,
        value: value.clone(),
    })
}

pub fn course(conn: &Connection, id: &str) -> Result<StudioDocument<CourseStudio>> {
    owns_course(conn, id)?;
    Ok(get(conn, "course", id)?.unwrap_or(StudioDocument {
        id: id.into(),
        revision: 0,
        value: CourseStudio::default(),
    }))
}

pub fn save_course(
    conn: &Connection,
    doc: &StudioDocument<CourseStudio>,
) -> Result<StudioDocument<CourseStudio>> {
    owns_course(conn, &doc.id)?;
    validate_prompt(&doc.value.initial_prompt)?;
    bounded(&doc.value.audience, 4000, "Audience")?;
    bounded(&doc.value.prerequisites, 4000, "Prerequisites")?;
    bounded(&doc.value.outcome, 8000, "Outcome")?;
    if doc.value.sources.len() > 30 {
        return Err(Error::Invalid("At most 30 sources are supported".into()));
    }
    let mut ids = std::collections::HashSet::new();
    for source in &doc.value.sources {
        if source.title.trim().is_empty() || !ids.insert(&source.id) {
            return Err(Error::Invalid(
                "Sources need unique identifiers and titles".into(),
            ));
        }
        bounded(&source.title, 200, "Source title")?;
        bounded(&source.text, 32_000, "Source text")?;
    }
    validate_tutor_policy(&doc.value.tutor)?;
    let tx = conn.unchecked_transaction()?;
    let saved = put(&tx, "course", &doc.id, doc.revision, &doc.value)?;
    write_tutor_policy(&tx, &doc.id, &doc.value.tutor)?;
    tx.commit()?;
    Ok(saved)
}

fn validate_tutor_policy(policy: &TutorPolicy) -> Result<()> {
    if !matches!(policy.guidance.as_str(), "socratic" | "balanced" | "direct") {
        return Err(Error::Invalid(
            "Choose a supported tutor guidance style".into(),
        ));
    }
    validate_prompt(&policy.initial_prompt)
}

pub fn write_tutor_policy(conn: &Connection, course_id: &str, policy: &TutorPolicy) -> Result<()> {
    validate_tutor_policy(policy)?;
    conn.execute(
        "INSERT INTO course_tutor_policies(course_id,enabled,guidance,initial_prompt)
         VALUES (?1,?2,?3,?4)
         ON CONFLICT(course_id) DO UPDATE SET enabled=excluded.enabled,
             guidance=excluded.guidance,initial_prompt=excluded.initial_prompt",
        params![
            course_id,
            policy.enabled,
            policy.guidance,
            policy.initial_prompt
        ],
    )?;
    Ok(())
}

pub fn tutor_policy(conn: &Connection, course_id: &str) -> Result<TutorPolicy> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM courses WHERE id=?1)",
        [course_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(Error::Permission);
    }
    Ok(conn
        .query_row(
            "SELECT enabled,guidance,initial_prompt FROM course_tutor_policies WHERE course_id=?1",
            [course_id],
            |row| {
                Ok(TutorPolicy {
                    enabled: row.get(0)?,
                    guidance: row.get(1)?,
                    initial_prompt: row.get(2)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

fn can_use_tutor(conn: &Connection, course_id: &str) -> Result<()> {
    let allowed: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM courses c
            WHERE c.id=?1 AND (
                EXISTS(SELECT 1 FROM local_identity i WHERE i.id=1 AND i.stake_address=c.author_address)
                OR EXISTS(SELECT 1 FROM enrollments e WHERE e.course_id=c.id AND e.status IN ('active','completed'))
            )
        )",
        [course_id],
        |row| row.get(0),
    )?;
    if allowed {
        Ok(())
    } else {
        Err(Error::Permission)
    }
}

pub struct TutorContext {
    pub policy: TutorPolicy,
    pub connection: StudioConnection,
    pub secret: Option<String>,
    pub lesson_title: String,
    pub lesson_inline: Option<String>,
    pub lesson_cid: Option<String>,
    pub thread: TutorThread,
}

pub fn tutor_context(
    conn: &Connection,
    course_id: &str,
    element_id: &str,
    connection_id: &str,
) -> Result<TutorContext> {
    can_use_tutor(conn, course_id)?;
    let policy = tutor_policy(conn, course_id)?;
    if !policy.enabled {
        return Err(Error::Unavailable(
            "The instructor has not enabled the tutor".into(),
        ));
    }
    let lesson: Option<(String, Option<String>, Option<String>, String)> = conn
        .query_row(
            "SELECT e.title,e.content_inline,e.content_cid,e.element_type
             FROM course_elements e JOIN course_chapters ch ON ch.id=e.chapter_id
             WHERE e.id=?1 AND ch.course_id=?2",
            params![element_id, course_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let (lesson_title, lesson_inline, lesson_cid, kind) = lesson.ok_or(Error::Permission)?;
    if kind != "text" {
        return Err(Error::Permission);
    }
    let connection = connection(conn, connection_id)?.value;
    if !connection.enabled || connection.capability != "text" {
        return Err(Error::Unavailable("Choose an enabled text model".into()));
    }
    crate::provider::validate_connection(&connection)?;
    let thread_id = blake3::hash(serde_json::to_string(&(course_id, element_id))?.as_bytes())
        .to_hex()
        .to_string();
    let messages = conn
        .query_row(
            "SELECT messages FROM studio_tutor_threads WHERE id=?1",
            [&thread_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|json| serde_json::from_str(&json))
        .transpose()?
        .unwrap_or_default();
    Ok(TutorContext {
        policy,
        secret: secret(conn, connection_id)?,
        connection,
        lesson_title,
        lesson_inline,
        lesson_cid,
        thread: TutorThread {
            id: thread_id,
            course_id: course_id.into(),
            element_id: element_id.into(),
            connection_id: connection_id.into(),
            messages,
        },
    })
}

pub fn save_tutor_exchange(
    conn: &Connection,
    mut thread: TutorThread,
    expected_messages: usize,
    question: &str,
    answer: &str,
) -> Result<TutorThread> {
    can_use_tutor(conn, &thread.course_id)?;
    if !tutor_policy(conn, &thread.course_id)?.enabled {
        return Err(Error::Permission);
    }
    bounded(question, 4_000, "Tutor question")?;
    bounded(answer, 32_000, "Tutor answer")?;
    let current: Option<String> = conn
        .query_row(
            "SELECT messages FROM studio_tutor_threads WHERE id=?1",
            [&thread.id],
            |row| row.get(0),
        )
        .optional()?;
    let current_len = current
        .as_deref()
        .map(serde_json::from_str::<Vec<TutorMessage>>)
        .transpose()?
        .unwrap_or_default()
        .len();
    if current_len != expected_messages {
        return Err(Error::Conflict);
    }
    let now = chrono::Utc::now().to_rfc3339();
    thread.messages.push(TutorMessage {
        role: "learner".into(),
        text: question.into(),
        created_at: now.clone(),
    });
    thread.messages.push(TutorMessage {
        role: "tutor".into(),
        text: answer.into(),
        created_at: now,
    });
    if thread.messages.len() > 20 {
        let remove = thread.messages.len() - 20;
        thread.messages.drain(..remove);
    }
    conn.execute(
        "INSERT INTO studio_tutor_threads(id,course_id,element_id,connection_id,messages,updated_at)
         VALUES (?1,?2,?3,?4,?5,datetime('now'))
         ON CONFLICT(id) DO UPDATE SET connection_id=excluded.connection_id,
             messages=excluded.messages,updated_at=excluded.updated_at",
        params![
            thread.id,
            thread.course_id,
            thread.element_id,
            thread.connection_id,
            serde_json::to_string(&thread.messages)?
        ],
    )?;
    Ok(thread)
}

pub fn clear_tutor_thread(conn: &Connection, course_id: &str, element_id: &str) -> Result<()> {
    can_use_tutor(conn, course_id)?;
    conn.execute(
        "DELETE FROM studio_tutor_threads WHERE course_id=?1 AND element_id=?2",
        params![course_id, element_id],
    )?;
    Ok(())
}

pub fn submit_lesson_feedback(
    conn: &Connection,
    course_id: &str,
    element_id: &str,
    rating: i64,
    comment: &str,
) -> Result<LessonFeedback> {
    if !(1..=5).contains(&rating) {
        return Err(Error::Invalid("Choose a rating from 1 to 5".into()));
    }
    bounded(comment, 4_000, "Feedback")?;
    let enrollment_id: Option<String> = conn
        .query_row(
            "SELECT e.id FROM enrollments e
             WHERE e.course_id=?1 AND e.status IN ('active','completed')
               AND EXISTS(
                 SELECT 1 FROM course_elements el
                 JOIN course_chapters ch ON ch.id=el.chapter_id
                 WHERE el.id=?2 AND ch.course_id=e.course_id
               )
             ORDER BY e.updated_at DESC LIMIT 1",
            params![course_id, element_id],
            |row| row.get(0),
        )
        .optional()?;
    let enrollment_id = enrollment_id.ok_or(Error::Permission)?;
    let id = blake3::hash(serde_json::to_string(&(&enrollment_id, element_id))?.as_bytes())
        .to_hex()
        .to_string();
    conn.execute(
        "INSERT INTO course_lesson_feedback(id,enrollment_id,course_id,element_id,rating,comment,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,datetime('now'))
         ON CONFLICT(enrollment_id,element_id) DO UPDATE SET
             rating=excluded.rating,comment=excluded.comment,created_at=datetime('now')",
        params![id, enrollment_id, course_id, element_id, rating, comment],
    )?;
    conn.query_row(
        "SELECT f.id,f.course_id,f.element_id,el.title,f.rating,f.comment,f.created_at
         FROM course_lesson_feedback f JOIN course_elements el ON el.id=f.element_id
         WHERE f.id=?1",
        [&id],
        feedback_row,
    )
    .map_err(Into::into)
}

fn feedback_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LessonFeedback> {
    Ok(LessonFeedback {
        id: row.get(0)?,
        course_id: row.get(1)?,
        element_id: row.get(2)?,
        element_title: row.get(3)?,
        rating: row.get(4)?,
        comment: row.get(5)?,
        created_at: row.get(6)?,
    })
}

pub fn lesson_feedback(
    conn: &Connection,
    course_id: &str,
    element_id: Option<&str>,
) -> Result<Vec<LessonFeedback>> {
    owns_course(conn, course_id)?;
    let mut stmt = conn.prepare(
        "SELECT f.id,f.course_id,f.element_id,el.title,f.rating,f.comment,f.created_at
         FROM course_lesson_feedback f JOIN course_elements el ON el.id=f.element_id
         WHERE f.course_id=?1 AND (?2 IS NULL OR f.element_id=?2)
         ORDER BY f.created_at DESC LIMIT 200",
    )?;
    let feedback = stmt
        .query_map(params![course_id, element_id], feedback_row)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::from)?;
    Ok(feedback)
}

pub fn settings(conn: &Connection) -> Result<StudioDocument<StudioSettings>> {
    Ok(get(conn, "settings", "default")?.unwrap_or(StudioDocument {
        id: "default".into(),
        revision: 0,
        value: StudioSettings::default(),
    }))
}

pub fn save_settings(
    conn: &Connection,
    doc: &StudioDocument<StudioSettings>,
) -> Result<StudioDocument<StudioSettings>> {
    let unique: std::collections::HashSet<_> =
        doc.value.roles.iter().map(|r| r.role.as_str()).collect();
    if doc.value.roles.len() != ROLES.len()
        || unique.len() != ROLES.len()
        || ROLES.iter().any(|r| !unique.contains(r))
    {
        return Err(Error::Invalid(
            "Provide exactly one configuration for each role".into(),
        ));
    }
    for role in &doc.value.roles {
        validate_prompt(&role.initial_prompt)?;
        if let Some(id) = &role.connection_id {
            connection(conn, id)?;
        }
    }
    put(conn, "settings", "default", doc.revision, &doc.value)
}

pub fn connection(conn: &Connection, id: &str) -> Result<StudioDocument<StudioConnection>> {
    get(conn, "connection", id)?
        .ok_or_else(|| Error::Unavailable("Model connection not found".into()))
}

pub fn save_connection(
    conn: &Connection,
    doc: &StudioDocument<StudioConnection>,
    key: Option<&str>,
) -> Result<StudioDocument<StudioConnection>> {
    crate::provider::validate_connection(&doc.value)?;
    if doc.id != doc.value.id {
        return Err(Error::Invalid("Connection identifier mismatch".into()));
    }
    if let Some(key) = key {
        bounded(key, 4096, "API key")?;
    }
    let tx = conn.unchecked_transaction()?;
    if let Some(key) = key {
        tx.execute("INSERT INTO studio_secrets(connection_id,secret) VALUES (?1,?2) ON CONFLICT(connection_id) DO UPDATE SET secret=excluded.secret",params![doc.id,key])?;
    }
    let mut value = doc.value.clone();
    value.has_key = secret(&tx, &doc.id)?.is_some_and(|s| !s.is_empty());
    let saved = put(&tx, "connection", &doc.id, doc.revision, &value)?;
    tx.commit()?;
    Ok(saved)
}

pub fn secret(conn: &Connection, id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT secret FROM studio_secrets WHERE connection_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn save_workflow(
    conn: &Connection,
    doc: &StudioDocument<StudioWorkflow>,
) -> Result<StudioDocument<StudioWorkflow>> {
    validate_workflow(&doc.value)?;
    if doc.id != doc.value.id {
        return Err(Error::Invalid("Workflow identifier mismatch".into()));
    }
    for step in &doc.value.steps {
        if let Some(id) = &step.connection_id {
            connection(conn, id)?;
        }
    }
    put(conn, "workflow", &doc.id, doc.revision, &doc.value)
}

struct AuthoringTarget {
    title: String,
    content: Option<String>,
    kind: String,
}

fn authoring_target(
    conn: &Connection,
    course_id: &str,
    element_id: &str,
) -> Result<AuthoringTarget> {
    owns_course(conn, course_id)?;
    let row: Option<(String,Option<String>,String,Option<String>)>=conn.query_row(
        "SELECT e.title,e.content_inline,e.element_type,e.content_cid FROM course_elements e JOIN course_chapters ch ON ch.id=e.chapter_id WHERE e.id=?1 AND ch.course_id=?2",
        params![element_id,course_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
    ).optional()?;
    let (title, content, kind, cid) = row.ok_or(Error::Permission)?;
    if !matches!(
        kind.as_str(),
        "text"
            | "quiz"
            | "assessment"
            | "objective_single_mcq"
            | "objective_multi_mcq"
            | "subjective_mcq"
            | "essay"
    ) {
        return Err(Error::Invalid(
            "Choose a text lesson, quiz, or essay that the built-in editor can review".into(),
        ));
    }
    if cid.is_some() && content.is_none() {
        return Err(Error::Unavailable(
            "Import the lesson text before using an authoring workflow".into(),
        ));
    }
    Ok(AuthoringTarget {
        title,
        content,
        kind,
    })
}

fn fingerprint(target: &AuthoringTarget) -> Result<String> {
    Ok(blake3::hash(
        serde_json::to_string(&(&target.title, &target.content, &target.kind))?.as_bytes(),
    )
    .to_hex()
    .to_string())
}

fn output_contract(kind: &str) -> &'static str {
    match kind {
        "text" => "Return only the complete proposed lesson body in Markdown.",
        "essay" => {
            "Return only valid JSON with this shape: {\"prompt\":string,\"rubric\":string,\"min_words\":number|null}."
        }
        _ => {
            "Return only valid JSON with this shape: {\"questions\":[{\"id\":string,\"question\":string,\"options\":[string,...],\"correct_index\":number}]}. For questions with multiple correct answers use correct_indices instead of correct_index."
        }
    }
}

fn validate_output(kind: &str, output: &str) -> Result<()> {
    if kind == "text" {
        return Ok(());
    }
    let value: serde_json::Value = serde_json::from_str(output)
        .map_err(|_| Error::Invalid("Assessment proposals must be valid JSON".into()))?;
    match kind {
        "essay"
            if value
                .get("prompt")
                .and_then(serde_json::Value::as_str)
                .is_some() =>
        {
            Ok(())
        }
        "essay" => Err(Error::Invalid("Essay proposals need a prompt".into())),
        _ if value
            .get("questions")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|questions| !questions.is_empty()) =>
        {
            Ok(())
        }
        _ => Err(Error::Invalid(
            "Quiz proposals need a non-empty questions array".into(),
        )),
    }
}

pub fn prepare_run(
    conn: &Connection,
    course_id: &str,
    element_id: &str,
    workflow_id: &str,
    prompt: &str,
) -> Result<StudioRun> {
    validate_prompt(prompt)?;
    let brief = course(conn, course_id)?;
    let settings = settings(conn)?;
    let workflow: StudioDocument<StudioWorkflow> = get(conn, "workflow", workflow_id)?
        .ok_or_else(|| Error::Invalid("Choose a saved workflow".into()))?;
    validate_workflow(&workflow.value)?;
    let target = authoring_target(conn, course_id, element_id)?;
    let mut steps = Vec::new();
    for step in &workflow.value.steps {
        let role = settings
            .value
            .roles
            .iter()
            .find(|r| r.role == step.role)
            .ok_or_else(|| Error::Invalid("Unknown role".into()))?;
        let id = step
            .connection_id
            .as_ref()
            .or(role.connection_id.as_ref())
            .ok_or_else(|| Error::Invalid(format!("Assign a model to {}", role.role)))?;
        let model = connection(conn, id)?.value;
        if !model.enabled {
            return Err(Error::Unavailable(format!("{} is disabled", model.name)));
        }
        if model.capability != "text" {
            return Err(Error::Unavailable("This workflow requires text-capable models. Media generation adapters are not installed".into()));
        }
        crate::provider::validate_connection(&model)?;
        let mut effective = effective_prompt(role, &workflow.value, step, &brief.value, prompt);
        effective.push_str("\n\nOUTPUT CONTRACT\n");
        effective.push_str(output_contract(&target.kind));
        steps.push(StudioRunStep {
            role: step.role.clone(),
            connection: model,
            effective_prompt: effective,
            output: None,
        });
    }
    let feedback = lesson_feedback(conn, course_id, Some(element_id))?;
    let context = serde_json::to_string(&serde_json::json!({
        "element_title":target.title,"element_type":target.kind,"current_draft":target.content,"audience":brief.value.audience,
        "prerequisites":brief.value.prerequisites,"learning_outcome":brief.value.outcome,
        "sources":brief.value.sources.iter().filter(|s|s.selected).collect::<Vec<_>>(),
        "learner_feedback":feedback
    }))?;
    bounded(&context, 128_000, "Selected source context")?;
    Ok(StudioRun {
        id: uuid::Uuid::new_v4().to_string(),
        course_id: course_id.into(),
        element_id: element_id.into(),
        workflow_name: workflow.value.name,
        workflow_revision: workflow.revision,
        target_fingerprint: fingerprint(&target)?,
        original_content: target.content,
        context,
        status: "prepared".into(),
        steps,
        error: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        applied_fingerprint: None,
    })
}

pub fn run(conn: &Connection, id: &str) -> Result<StudioDocument<StudioRun>> {
    let doc: StudioDocument<StudioRun> =
        get(conn, "run", id)?.ok_or_else(|| Error::Invalid("Run not found".into()))?;
    owns_course(conn, &doc.value.course_id)?;
    Ok(doc)
}

pub fn apply_run(
    conn: &Connection,
    id: &str,
    expected: i64,
    text: &str,
) -> Result<StudioDocument<StudioRun>> {
    bounded(text, 128_000, "Draft")?;
    if text.trim().is_empty() {
        return Err(Error::Invalid("The draft is empty".into()));
    }
    let tx = conn.unchecked_transaction()?;
    let mut doc = run(&tx, id)?;
    if doc.revision != expected || doc.value.status != "review" {
        return Err(Error::Conflict);
    }
    let target = authoring_target(&tx, &doc.value.course_id, &doc.value.element_id)?;
    if fingerprint(&target)? != doc.value.target_fingerprint {
        return Err(Error::Conflict);
    }
    validate_output(&target.kind, text)?;
    tx.execute(
        "UPDATE course_elements SET content_inline=?1,content_cid=NULL WHERE id=?2",
        params![text, doc.value.element_id],
    )?;
    tx.execute(
        "UPDATE courses SET status='draft',updated_at=datetime('now') WHERE id=?1",
        [&doc.value.course_id],
    )?;
    doc.value.status = "applied".into();
    doc.value.applied_fingerprint = Some(fingerprint(&AuthoringTarget {
        title: target.title,
        content: Some(text.into()),
        kind: target.kind,
    })?);
    let saved = put(&tx, "run", id, expected, &doc.value)?;
    tx.commit()?;
    Ok(saved)
}

pub fn undo_run(conn: &Connection, id: &str, expected: i64) -> Result<StudioDocument<StudioRun>> {
    let tx = conn.unchecked_transaction()?;
    let mut doc = run(&tx, id)?;
    if doc.revision != expected || doc.value.status != "applied" {
        return Err(Error::Conflict);
    }
    let target = authoring_target(&tx, &doc.value.course_id, &doc.value.element_id)?;
    if Some(fingerprint(&target)?) != doc.value.applied_fingerprint {
        return Err(Error::Conflict);
    }
    tx.execute(
        "UPDATE course_elements SET content_inline=?1,content_cid=NULL WHERE id=?2",
        params![doc.value.original_content, doc.value.element_id],
    )?;
    doc.value.status = "undone".into();
    let saved = put(&tx, "run", id, expected, &doc.value)?;
    tx.commit()?;
    Ok(saved)
}

pub fn read_draft(
    conn: &Connection,
    course_id: &str,
    element_id: &str,
) -> Result<serde_json::Value> {
    let target = authoring_target(conn, course_id, element_id)?;
    if target.kind != "text" {
        return Err(Error::Permission);
    }
    let brief = course(conn, course_id)?.value;
    Ok(
        serde_json::json!({"course_id":course_id,"element_id":element_id,"title":target.title,"text":target.content,"fingerprint":fingerprint(&target)?,"audience":brief.audience,"outcome":brief.outcome,"initial_prompt":brief.initial_prompt,"sources":brief.sources.into_iter().filter(|s|s.selected).collect::<Vec<_>>()}),
    )
}

pub fn propose_draft(
    conn: &Connection,
    course_id: &str,
    element_id: &str,
    expected: &str,
    text: &str,
    client_name: &str,
    request_id: &str,
) -> Result<StudioDocument<StudioRun>> {
    bounded(text, 128_000, "Proposed lesson")?;
    bounded(request_id, 200, "Request identifier")?;
    if text.trim().is_empty() || request_id.is_empty() {
        return Err(Error::Invalid(
            "Proposal text and request identifier are required".into(),
        ));
    }
    let target = authoring_target(conn, course_id, element_id)?;
    if target.kind != "text" {
        return Err(Error::Permission);
    }
    let id = blake3::hash(
        serde_json::to_string(&(client_name, request_id, course_id, element_id))?.as_bytes(),
    )
    .to_hex()
    .to_string();
    if let Some(previous) = get::<StudioRun>(conn, "run", &id)? {
        if previous.value.target_fingerprint != expected
            || previous
                .value
                .steps
                .first()
                .and_then(|s| s.output.as_deref())
                != Some(text)
        {
            return Err(Error::Conflict);
        }
        return Ok(previous);
    }
    if fingerprint(&target)? != expected {
        return Err(Error::Conflict);
    }
    let run=StudioRun{id:id.clone(),course_id:course_id.into(),element_id:element_id.into(),workflow_name:format!("Proposal from {client_name}"),workflow_revision:0,target_fingerprint:expected.into(),original_content:target.content,context:target.title,status:"review".into(),steps:vec![StudioRunStep{role:"draft".into(),connection:StudioConnection{id:"external-assistant".into(),name:client_name.into(),endpoint:String::new(),model:"External assistant".into(),location:"external".into(),capability:"text".into(),enabled:false,has_key:false},effective_prompt:"Provided by the external assistant; its prompt history is not available to Alexandria.".into(),output:Some(text.into())}],error:None,created_at:chrono::Utc::now().to_rfc3339(),applied_fingerprint:None};
    put(conn, "run", &id, 0, &run)
}
