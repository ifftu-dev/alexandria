use alexandria_studio::{model::*, provider, store, Error};
use rusqlite::Connection;

#[path = "../../../src-tauri/src/db/schema.rs"]
mod schema;

fn db() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    for (_, _, sql) in schema::MIGRATIONS {
        db.execute_batch(sql).unwrap();
    }
    db.execute_batch("INSERT INTO local_identity(id,stake_address,payment_address) VALUES (1,'me','payment'); INSERT INTO courses(id,title,author_address) VALUES ('mine','My course','me'),('foreign','Other course','other'); INSERT INTO course_chapters(id,course_id,title,position) VALUES ('chapter','mine','Chapter',0); INSERT INTO course_elements(id,chapter_id,title,element_type,content_inline,position) VALUES ('lesson','chapter','Lesson','text','Original text',0),('assessment','chapter','Exam','assessment','SECRET ANSWERS',1);").unwrap();
    db
}
fn setup(db: &Connection) -> StudioDocument<StudioWorkflow> {
    let c = StudioConnection {
        id: "local".into(),
        name: "Local writer".into(),
        endpoint: "http://127.0.0.1:11434/v1".into(),
        model: "writer".into(),
        location: "local".into(),
        capability: "text".into(),
        enabled: true,
        has_key: false,
    };
    store::save_connection(
        db,
        &StudioDocument {
            id: c.id.clone(),
            revision: 0,
            value: c,
        },
        Some("private-api-key"),
    )
    .unwrap();
    let mut settings = store::settings(db).unwrap();
    for role in &mut settings.value.roles {
        role.connection_id = Some("local".into());
        role.initial_prompt = format!("My custom {} instructions", role.role);
    }
    store::save_settings(db, &settings).unwrap();
    let w = StudioWorkflow {
        id: "workflow".into(),
        name: "Draft and review".into(),
        initial_prompt: "Use British English".into(),
        steps: vec![StudioStep {
            id: "step".into(),
            role: "draft".into(),
            connection_id: None,
            initial_prompt: "Use short paragraphs".into(),
        }],
    };
    store::save_workflow(
        db,
        &StudioDocument {
            id: w.id.clone(),
            revision: 0,
            value: w,
        },
    )
    .unwrap()
}
#[test]
fn saved_brief_survives_reload_and_stale_writes_are_rejected() {
    let db = db();
    let mut doc = store::course(&db, "mine").unwrap();
    doc.value.initial_prompt = "Teach using everyday examples".into();
    let saved = store::save_course(&db, &doc).unwrap();
    assert_eq!(store::course(&db, "mine").unwrap().value, saved.value);
    assert!(matches!(
        store::save_course(&db, &doc),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        store::course(&db, "foreign"),
        Err(Error::Permission)
    ));
    assert!(matches!(
        store::course(&db, "missing"),
        Err(Error::Permission)
    ));
}
#[test]
fn prepared_runs_freeze_prompts_models_and_exclude_unselected_sources() {
    let db = db();
    setup(&db);
    let mut course = store::course(&db, "mine").unwrap();
    course.value.initial_prompt = "Assume no prior Python".into();
    course.value.sources = vec![
        StudioSource {
            id: "a".into(),
            title: "Included".into(),
            text: "Allowed reference".into(),
            selected: true,
        },
        StudioSource {
            id: "b".into(),
            title: "Private".into(),
            text: "PRIVATE EXCLUDED".into(),
            selected: false,
        },
    ];
    store::save_course(&db, &course).unwrap();
    let run = store::prepare_run(&db, "mine", "lesson", "workflow", "Focus on loops").unwrap();
    let prompt = &run.steps[0].effective_prompt;
    for expected in [
        "My custom draft instructions",
        "Use British English",
        "Use short paragraphs",
        "Assume no prior Python",
        "Focus on loops",
    ] {
        assert!(prompt.contains(expected));
    }
    assert!(!run.context.contains("PRIVATE EXCLUDED"));
    assert!(!run.context.contains("SECRET ANSWERS"));
    let serialized = serde_json::to_string(&run).unwrap();
    assert!(!serialized.contains("private-api-key"));
    let mut settings = store::settings(&db).unwrap();
    settings.value.roles[1].initial_prompt = "New default".into();
    store::save_settings(&db, &settings).unwrap();
    assert!(run.steps[0]
        .effective_prompt
        .contains("My custom draft instructions"));
    let mut assessment = store::prepare_run(
        &db,
        "mine",
        "assessment",
        "workflow",
        "Create two questions",
    )
    .unwrap();
    assert!(assessment.context.contains("SECRET ANSWERS"));
    assert!(assessment.steps[0]
        .effective_prompt
        .contains("correct_indices"));
    assessment.status = "review".into();
    assessment.steps[0].output = Some(
        r#"{"questions":[{"id":"q1","question":"Two plus two?","options":["3","4"],"correct_index":1}]}"#
            .into(),
    );
    let assessment = store::put(&db, "run", &assessment.id, 0, &assessment).unwrap();
    assert!(store::apply_run(&db, &assessment.id, assessment.revision, "not json").is_err());
    store::apply_run(
        &db,
        &assessment.id,
        assessment.revision,
        assessment.value.steps[0].output.as_deref().unwrap(),
    )
    .unwrap();
    assert!(store::read_draft(&db, "mine", "assessment").is_err());
}
#[test]
fn approval_is_explicit_conflict_checked_and_reversible() {
    let db = db();
    setup(&db);
    let run = store::prepare_run(&db, "mine", "lesson", "workflow", "").unwrap();
    let prepared = store::put(&db, "run", &run.id, 0, &run).unwrap();
    assert!(matches!(
        store::apply_run(&db, &run.id, prepared.revision, "Changed"),
        Err(Error::Conflict)
    ));
    let mut value = prepared.value;
    value.status = "review".into();
    value.steps[0].output = Some("Draft".into());
    let ready = store::put(&db, "run", &run.id, prepared.revision, &value).unwrap();
    let applied = store::apply_run(&db, &run.id, ready.revision, "Instructor-edited text").unwrap();
    let content: String = db
        .query_row(
            "SELECT content_inline FROM course_elements WHERE id='lesson'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(content, "Instructor-edited text");
    assert!(matches!(
        store::apply_run(&db, &run.id, ready.revision, "Duplicate"),
        Err(Error::Conflict)
    ));
    store::undo_run(&db, &run.id, applied.revision).unwrap();
    let content: String = db
        .query_row(
            "SELECT content_inline FROM course_elements WHERE id='lesson'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(content, "Original text");
}
#[test]
fn newer_manual_edits_prevent_generated_overwrites() {
    let db = db();
    setup(&db);
    let mut run = store::prepare_run(&db, "mine", "lesson", "workflow", "").unwrap();
    run.status = "review".into();
    let doc = store::put(&db, "run", &run.id, 0, &run).unwrap();
    db.execute(
        "UPDATE course_elements SET content_inline='New manual text' WHERE id='lesson'",
        [],
    )
    .unwrap();
    assert!(matches!(
        store::apply_run(&db, &run.id, doc.revision, "Generated"),
        Err(Error::Conflict)
    ));
    assert_eq!(store::run(&db, &run.id).unwrap().value.status, "review");
}
#[test]
fn invalid_connection_update_does_not_replace_secret() {
    let db = db();
    setup(&db);
    let doc = store::connection(&db, "local").unwrap();
    assert!(store::save_connection(
        &db,
        &StudioDocument {
            revision: 0,
            ..doc.clone()
        },
        Some("bad-replacement")
    )
    .is_err());
    assert_eq!(
        store::secret(&db, "local").unwrap().as_deref(),
        Some("private-api-key")
    );
    for endpoint in [
        "http://example.com/v1",
        "http://127.0.0.1.evil.test/v1",
        "http://localhost:11434/v1?key=secret",
        "http://user:password@localhost/v1",
    ] {
        let mut model = doc.value.clone();
        model.endpoint = endpoint.into();
        assert!(provider::validate_connection(&model).is_err());
    }
}

#[test]
fn tutor_policy_is_enrollment_scoped_and_threads_are_conflict_checked() {
    let db = db();
    setup(&db);
    let mut course = store::course(&db, "mine").unwrap();
    course.value.tutor = TutorPolicy {
        enabled: true,
        guidance: "balanced".into(),
        initial_prompt: "Use course vocabulary and short examples.".into(),
    };
    store::save_course(&db, &course).unwrap();

    db.execute("UPDATE local_identity SET stake_address='learner'", [])
        .unwrap();
    db.execute(
        "INSERT INTO enrollments(id,course_id,status) VALUES ('enrollment','mine','active')",
        [],
    )
    .unwrap();
    let context = store::tutor_context(&db, "mine", "lesson", "local").unwrap();
    assert_eq!(context.policy.guidance, "balanced");
    assert_eq!(context.lesson_inline.as_deref(), Some("Original text"));
    assert_eq!(context.secret.as_deref(), Some("private-api-key"));
    assert!(context.thread.messages.is_empty());
    assert!(matches!(
        store::tutor_context(&db, "mine", "assessment", "local"),
        Err(Error::Permission)
    ));

    let saved = store::save_tutor_exchange(
        &db,
        context.thread.clone(),
        0,
        "Can I have a hint?",
        "Start by naming the first concept.",
    )
    .unwrap();
    assert_eq!(saved.messages.len(), 2);
    assert!(matches!(
        store::save_tutor_exchange(&db, context.thread, 0, "Again", "No"),
        Err(Error::Conflict)
    ));
    store::clear_tutor_thread(&db, "mine", "lesson").unwrap();
    assert!(store::tutor_context(&db, "mine", "lesson", "local")
        .unwrap()
        .thread
        .messages
        .is_empty());

    store::submit_lesson_feedback(&db, "mine", "lesson", 3, "The loop example was too fast")
        .unwrap();
    db.execute("UPDATE local_identity SET stake_address='me'", [])
        .unwrap();
    let feedback = store::lesson_feedback(&db, "mine", Some("lesson")).unwrap();
    assert_eq!(feedback.len(), 1);
    assert_eq!(feedback[0].rating, 3);
    let run = store::prepare_run(&db, "mine", "lesson", "workflow", "Improve clarity").unwrap();
    assert!(run.context.contains("The loop example was too fast"));

    db.execute("UPDATE local_identity SET stake_address='learner'", [])
        .unwrap();
    db.execute("UPDATE enrollments SET status='dropped'", [])
        .unwrap();
    assert!(matches!(
        store::tutor_context(&db, "mine", "lesson", "local"),
        Err(Error::Permission)
    ));
}
#[tokio::test]
async fn real_http_boundary_sends_effective_prompt_and_handles_provider_failure() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let db = db();
    setup(&db);
    let mut connection = store::connection(&db, "local").unwrap().value;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    connection.endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut data = vec![0; 8192];
        let n = socket.read(&mut data).await.unwrap();
        let request = String::from_utf8_lossy(&data[..n]);
        assert!(request.starts_with("POST /v1/chat/completions"));
        assert!(request.contains("Personal instructor prompt"));
        let body = r#"{"choices":[{"message":{"content":"A real HTTP response"}}]}"#;
        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
    });
    let result = provider::complete(
        &connection,
        None,
        "Personal instructor prompt",
        "Lesson context",
    )
    .await
    .unwrap();
    assert_eq!(result, "A real HTTP response");
    server.await.unwrap();
}

#[test]
fn external_proposals_are_idempotent_review_only_and_isolated() {
    let db = db();
    let draft = store::read_draft(&db, "mine", "lesson").unwrap();
    let fingerprint = draft["fingerprint"].as_str().unwrap();
    let proposal = store::propose_draft(
        &db,
        "mine",
        "lesson",
        fingerprint,
        "Proposed",
        "Assistant",
        "grant:request-1",
    )
    .unwrap();
    assert_eq!(proposal.value.status, "review");
    assert_eq!(
        store::read_draft(&db, "mine", "lesson").unwrap()["text"],
        "Original text"
    );
    let duplicate = store::propose_draft(
        &db,
        "mine",
        "lesson",
        fingerprint,
        "Proposed",
        "Assistant",
        "grant:request-1",
    )
    .unwrap();
    assert_eq!(duplicate.id, proposal.id);
    assert!(store::propose_draft(
        &db,
        "mine",
        "lesson",
        fingerprint,
        "Different",
        "Assistant",
        "grant:request-1"
    )
    .is_err());
    let applied = store::apply_run(&db, &proposal.id, proposal.revision, "Proposed").unwrap();
    let retry = store::propose_draft(
        &db,
        "mine",
        "lesson",
        fingerprint,
        "Proposed",
        "Assistant",
        "grant:request-1",
    )
    .unwrap();
    assert_eq!(retry.revision, applied.revision);
    assert_eq!(retry.value.status, "applied");
    assert!(store::propose_draft(
        &db,
        "mine",
        "lesson",
        fingerprint,
        "Stale",
        "Assistant",
        "grant:request-2"
    )
    .is_err());
    assert!(store::read_draft(&db, "mine", "assessment").is_err());
    assert!(store::read_draft(&db, "foreign", "lesson").is_err());
    db.execute("UPDATE local_identity SET stake_address='other'", [])
        .unwrap();
    assert!(store::propose_draft(
        &db,
        "mine",
        "lesson",
        fingerprint,
        "Proposed",
        "Assistant",
        "grant:request-1"
    )
    .is_err());
    assert!(store::run(&db, &proposal.id).is_err());
}

#[tokio::test]
async fn provider_failure_does_not_disclose_response_body() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let db = db();
    setup(&db);
    let mut connection = store::connection(&db, "local").unwrap().value;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    connection.endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 8192];
        let _ = socket.read(&mut bytes).await.unwrap();
        let body = "sensitive provider diagnostic";
        socket.write_all(format!("HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).as_bytes()).await.unwrap();
    });
    let error = provider::complete(&connection, Some("private-key"), "Prompt", "Context")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("401"));
    assert!(!error.contains("private-key"));
    assert!(!error.contains("sensitive"));
    server.await.unwrap();
}
