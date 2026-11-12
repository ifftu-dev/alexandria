//! Seam-1 tests for response-process signals: a session over an in-memory
//! database receives raw response timing in its windows, and the committed
//! snapshots, fusion summary and session outcome are what a sponsor sees.

use super::*;

use crate::db::Database;
use crate::sentinel::scoring::{
    BaselineItem, BaselineSource, CameraFeatures, InputKind, ItemResponse, ProfileRatios,
    RatioManifest, ResponseBaseline, ResponseFeatures, WindowFeatures, DEFAULT_PROFILE,
};
use crate::sentinel::snapshots::{persist_snapshot, persist_snapshot_with};

/// Three earlier items, each answered about 0.8 s past its reading floor.
fn history() -> ResponseBaseline {
    ResponseBaseline {
        source: BaselineSource::History,
        items: vec![
            BaselineItem {
                chars: 100,
                difficulty: Some(2),
                first_input_ms: 4_000.0,
            };
            3
        ],
    }
}

fn session_status(db: &Database) -> String {
    read_session(db.conn(), "rp-session").unwrap().status
}

fn session_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().unwrap();
    db.conn()
        .execute(
            "INSERT INTO integrity_sessions (id, status) VALUES ('rp-session', 'active')",
            [],
        )
        .unwrap();
    db
}

fn submit(
    db: &Database,
    response: ResponseFeatures,
) -> crate::sentinel::snapshots::IntegritySnapshot {
    let features = WindowFeatures {
        response: Some(response),
        ..Default::default()
    };
    persist_snapshot(
        db.conn(),
        &SubmitSnapshotRequest::from_features("rp-session", "assessment", features),
    )
    .unwrap()
}

/// A choice item of `chars` characters to read, answered once after
/// `first_input_ms`.
fn choice(id: &str, chars: u32, difficulty: u8, first_input_ms: f64) -> ItemResponse {
    ItemResponse {
        item_id: id.into(),
        input: InputKind::Choice,
        chars,
        difficulty: Some(difficulty),
        first_input_ms,
        commit_ms: first_input_ms,
        ..Default::default()
    }
}

#[test]
fn an_answer_before_the_item_could_be_read_is_an_advisory_signal_not_a_verdict() {
    let db = session_db();
    // About 300 characters at difficulty 4 cannot be read in 1.2 seconds.
    let snapshot = submit(
        &db,
        ResponseFeatures {
            items: vec![choice("q1", 300, 4, 1_200.0)],
            ..Default::default()
        },
    );
    assert_eq!(snapshot.anomaly_flags, ["response_below_reading_floor"]);
    assert_eq!(snapshot.session_status.as_deref(), Some("active"));
    let fusion = snapshot.fusion.unwrap();
    assert_eq!(fusion.advisory, ["response_below_reading_floor"]);

    // The same item answered after a plausible reading time raises nothing.
    let unhurried = submit(
        &db,
        ResponseFeatures {
            items: vec![choice("q2", 300, 4, 25_000.0)],
            ..Default::default()
        },
    );
    assert!(unhurried.anomaly_flags.is_empty());
}

#[test]
fn the_session_records_how_many_items_had_response_timing() {
    let db = session_db();
    assert_eq!(
        read_session(db.conn(), "rp-session")
            .unwrap()
            .response_items,
        0
    );
    submit(
        &db,
        ResponseFeatures {
            items: vec![choice("q1", 120, 2, 9_000.0), choice("q2", 90, 2, 7_000.0)],
            ..Default::default()
        },
    );
    // Returning to q1 to change the answer is not a second item.
    let mut back = choice("q1", 120, 2, 400.0);
    back.revisit = true;
    back.revisions = 1;
    let snapshot = submit(
        &db,
        ResponseFeatures {
            items: vec![back, choice("q3", 100, 2, 8_000.0)],
            ..Default::default()
        },
    );
    assert!(
        snapshot.anomaly_flags.is_empty(),
        "a revisit is never judged against the reading floor"
    );
    // A window without response features leaves the count alone.
    persist_snapshot(
        db.conn(),
        &SubmitSnapshotRequest::from_features(
            "rp-session",
            "assessment",
            WindowFeatures::default(),
        ),
    )
    .unwrap();
    assert_eq!(
        read_session(db.conn(), "rp-session")
            .unwrap()
            .response_items,
        3
    );
}

#[test]
fn a_uniform_lag_over_the_learners_own_baseline_is_a_relay_signal() {
    let db = session_db();
    // Both items wait about five seconds longer than this learner usually
    // does, as when someone else reads the question and relays the answer.
    let relayed = submit(
        &db,
        ResponseFeatures {
            items: vec![choice("q1", 100, 2, 9_000.0), choice("q2", 100, 2, 9_500.0)],
            baseline: Some(history()),
            ..Default::default()
        },
    );
    assert_eq!(relayed.anomaly_flags, ["response_relay_lag"]);
    assert_eq!(relayed.session_status.as_deref(), Some("active"));

    // One slow item and one quick one is thinking, not a relay.
    let uneven = submit(
        &db,
        ResponseFeatures {
            items: vec![
                choice("q3", 100, 2, 4_500.0),
                choice("q4", 100, 2, 12_000.0),
            ],
            baseline: Some(history()),
            ..Default::default()
        },
    );
    assert!(uneven.anomaly_flags.is_empty());

    // Without a baseline there is nothing to compare against.
    let unknown = submit(
        &db,
        ResponseFeatures {
            items: vec![choice("q5", 100, 2, 9_000.0), choice("q6", 100, 2, 9_500.0)],
            ..Default::default()
        },
    );
    assert!(unknown.anomaly_flags.is_empty());
}

#[test]
fn idle_then_burst_and_answer_change_after_idle_are_advisory_signals() {
    let db = session_db();
    let mut item = choice("q1", 100, 2, 6_000.0);
    item.commit_ms = 40_000.0;
    item.revisions = 1;
    item.idle_bursts = 1;
    item.changes_after_idle = 1;
    let snapshot = submit(
        &db,
        ResponseFeatures {
            items: vec![item],
            ..Default::default()
        },
    );
    assert_eq!(
        snapshot.anomaly_flags,
        ["response_idle_burst", "answer_change_after_idle"]
    );
    assert_eq!(session_status(&db), "active");
}

/// A manifest that has measured the relay lag, the cross-modal gaze feature
/// and the device glance, each over the threshold on its own.
fn measured() -> RatioManifest {
    let mut manifest = RatioManifest::uncertified();
    manifest.version = "test-certified".into();
    manifest.profiles.insert(
        DEFAULT_PROFILE.into(),
        ProfileRatios {
            threshold: 2.0,
            ratios: [
                ("response_relay_lag".to_string(), 2.5),
                ("gaze_down_then_response".to_string(), 2.5),
                ("device_glance".to_string(), 2.5),
            ]
            .into(),
        },
    );
    manifest
}

#[test]
fn a_gaze_down_before_a_response_is_gaze_evidence_not_a_second_channel() {
    let db = session_db();
    let glance_then_answer = WindowFeatures {
        camera: Some(CameraFeatures {
            gaze_checks: 10,
            gaze_offscreen_checks: 3,
            gaze_down_glances: 3,
            ..Default::default()
        }),
        response: Some(ResponseFeatures {
            gaze_down_then_response: 2,
            ..Default::default()
        }),
        ..Default::default()
    };
    let snapshot = persist_snapshot_with(
        db.conn(),
        &SubmitSnapshotRequest::from_features("rp-session", "assessment", glance_then_answer),
        &measured(),
    )
    .unwrap();
    assert!(snapshot
        .anomaly_flags
        .contains(&"gaze_down_then_response".to_string()));
    // device_glance is critical on its own, so the session is in review,
    // but one channel is never flagged.
    assert_eq!(snapshot.session_status.as_deref(), Some("review"));
    let fusion = snapshot.fusion.unwrap();
    let gaze = fusion
        .channels
        .iter()
        .find(|c| c.channel == "gaze")
        .unwrap();
    assert_eq!(gaze.signals, ["device_glance", "gaze_down_then_response"]);

    // A measured relay lag is a second, independent channel.
    let relayed = persist_snapshot_with(
        db.conn(),
        &SubmitSnapshotRequest::from_features(
            "rp-session",
            "assessment",
            WindowFeatures {
                response: Some(ResponseFeatures {
                    items: vec![choice("q1", 100, 2, 9_000.0), choice("q2", 100, 2, 9_500.0)],
                    baseline: Some(history()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
        &measured(),
    )
    .unwrap();
    assert_eq!(relayed.session_status.as_deref(), Some("flagged"));
    let over: Vec<_> = relayed
        .fusion
        .unwrap()
        .channels
        .into_iter()
        .filter(|c| c.over_threshold)
        .map(|c| c.channel)
        .collect();
    assert_eq!(over, ["gaze", "response_process"]);
}

#[test]
fn a_response_after_a_second_voice_is_its_own_advisory_signal() {
    let db = session_db();
    let snapshot = submit(
        &db,
        ResponseFeatures {
            voice_then_response: 1,
            ..Default::default()
        },
    );
    assert_eq!(snapshot.anomaly_flags, ["voice_then_response"]);
    assert_eq!(snapshot.fusion.unwrap().advisory, ["voice_then_response"]);
}

#[test]
fn response_timing_no_honest_collector_produces_is_refused_before_writing() {
    let db = session_db();
    let mut committed_before_first_input = choice("q1", 100, 2, 5_000.0);
    committed_before_first_input.commit_ms = 1_000.0;
    let mut impossible_difficulty = choice("q1", 100, 2, 5_000.0);
    impossible_difficulty.difficulty = Some(7);
    let mut negative = choice("q1", 100, 2, 5_000.0);
    negative.first_input_ms = -1.0;
    let mut unnamed = choice("", 100, 2, 5_000.0);
    unnamed.commit_ms = 5_000.0;
    for item in [
        committed_before_first_input,
        impossible_difficulty,
        negative,
        unnamed,
    ] {
        let features = WindowFeatures {
            response: Some(ResponseFeatures {
                items: vec![item],
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(persist_snapshot(
            db.conn(),
            &SubmitSnapshotRequest::from_features("rp-session", "assessment", features),
        )
        .is_err());
    }
    let count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM integrity_snapshots", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
}

/// A spoken answer in an interview: nothing to read, timed from the end of
/// the interviewer's question to the candidate's first speech.
fn spoken(id: &str, first_input_ms: f64) -> ItemResponse {
    ItemResponse {
        item_id: id.into(),
        input: InputKind::Voice,
        chars: 0,
        difficulty: None,
        first_input_ms,
        commit_ms: first_input_ms,
        ..Default::default()
    }
}

#[test]
fn a_spoken_answer_that_starts_straight_after_the_question_is_turn_taking() {
    let db = session_db();
    // Conversational turns start within a few hundred milliseconds; a heard
    // question has no reading floor.
    let snapshot = submit(
        &db,
        ResponseFeatures {
            items: vec![spoken("q1", 250.0), spoken("q2", 400.0)],
            ..Default::default()
        },
    );
    assert!(
        snapshot.anomaly_flags.is_empty(),
        "{:?}",
        snapshot.anomaly_flags
    );
    assert_eq!(
        read_session(db.conn(), "rp-session")
            .unwrap()
            .response_items,
        2
    );
}

#[test]
fn spoken_answers_that_all_wait_for_a_relay_are_judged_against_the_candidates_own_turns() {
    let db = session_db();
    let own_turns = ResponseBaseline {
        source: BaselineSource::Attempt,
        items: vec![
            BaselineItem {
                chars: 0,
                difficulty: None,
                first_input_ms: 600.0,
            };
            3
        ],
    };
    let snapshot = submit(
        &db,
        ResponseFeatures {
            items: vec![spoken("q4", 5_200.0), spoken("q5", 5_600.0)],
            baseline: Some(own_turns),
            ..Default::default()
        },
    );
    assert_eq!(snapshot.anomaly_flags, ["response_relay_lag"]);
}
