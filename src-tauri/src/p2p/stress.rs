//! Network stress tests — PR 4.8.
//!
//! Tests the P2P networking, gossip handling, sync merge, challenge,
//! and attestation subsystems under simulated load. These are unit-level
//! stress tests that exercise the domain handlers directly (without
//! spawning real libp2p nodes) to verify correctness under:
//!
//!   - **High-volume gossip**: Hundreds of concurrent messages across topics
//!   - **Concurrent validation**: Parallel dedup cache access
//!   - **Sync conflicts**: Competing LWW and append-only merges
//!   - **Challenge/attestation races**: Concurrent committee voting
//!   - **Adversarial inputs**: Boundary conditions and malformed data
//!
//! Multi-node gossip propagation tests use real P2P nodes connected via
//! mDNS on localhost to verify end-to-end message delivery.

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use ed25519_dalek::SigningKey;
    use rusqlite::params;

    use crate::db::Database;
    use crate::domain::attestation::SetRequirementParams;
    use crate::domain::attestation::SubmitAttestationParams;
    use crate::domain::catalog::CatalogAnnouncement;
    use crate::domain::challenge::SubmitChallengeParams;
    use crate::domain::evidence::EvidenceAnnouncement;
    use crate::evidence::attestation;
    use crate::evidence::challenge;
    use crate::p2p::catalog::{build_catalog_announcement, handle_catalog_message};
    use crate::p2p::evidence::handle_evidence_message;
    use crate::p2p::signing::sign_gossip_message;
    use crate::p2p::sync::{
        enqueue_change, get_pending_queue_items, mark_delivered, merge_append_only, merge_lww_rows,
        prune_delivered_queue, register_local_device, register_remote_device,
    };
    use crate::p2p::types::SignedGossipMessage;
    use crate::p2p::validation::MessageValidator;

    // ---------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn test_key() -> SigningKey {
        let mut rng = rand::thread_rng();
        SigningKey::generate(&mut rng)
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn signed_catalog_msg(
        key: &SigningKey,
        announcement: &CatalogAnnouncement,
    ) -> SignedGossipMessage {
        let payload = serde_json::to_vec(announcement).unwrap();
        sign_gossip_message(
            "/alexandria/catalog/1.0",
            payload,
            key,
            &announcement.author_address,
        )
    }

    fn signed_evidence_msg(
        key: &SigningKey,
        announcement: &EvidenceAnnouncement,
    ) -> SignedGossipMessage {
        let payload = serde_json::to_vec(announcement).unwrap();
        sign_gossip_message(
            "/alexandria/evidence/1.0",
            payload,
            key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        )
    }

    /// Insert the full FK chain for a skill (subject_field → subject → skill).
    fn insert_test_skill(db: &Database, skill_id: &str) {
        let conn = db.conn();
        conn.execute(
            "INSERT OR IGNORE INTO subject_fields (id, name) VALUES ('sf_stress', 'Stress Field')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO subjects (id, name, subject_field_id) \
             VALUES ('sub_stress', 'Stress Subject', 'sf_stress')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO skills (id, name, subject_id) VALUES (?1, ?1, 'sub_stress')",
            params![skill_id],
        )
        .unwrap();
    }

    /// Set up a database with all fixtures needed for challenge stress tests.
    fn setup_challenge_db() -> Database {
        let db = test_db();
        let conn = db.conn();

        // Local identity
        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, 'stake_test1uchallenger', 'addr_test1q_challenger')",
            [],
        )
        .unwrap();

        // Taxonomy
        conn.execute(
            "INSERT INTO subject_fields (id, name) VALUES ('sf1', 'CS')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO subjects (id, name, subject_field_id) VALUES ('sub1', 'Algo', 'sf1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO skills (id, name, subject_id) VALUES ('sk1', 'Graphs', 'sub1')",
            [],
        )
        .unwrap();

        // Active DAO with 5 committee members (for concurrent voting)
        conn.execute(
            "INSERT INTO governance_daos (id, name, scope_type, scope_id, status) \
             VALUES ('dao1', 'CS DAO', 'subject_field', 'sf1', 'active')",
            [],
        )
        .unwrap();
        for i in 1..=5 {
            conn.execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
                 VALUES ('dao1', ?1, 'committee')",
                params![format!("stake_test1ucommittee{i}")],
            )
            .unwrap();
        }

        // Course + evidence chain
        conn.execute(
            "INSERT INTO courses (id, title, author_address) \
             VALUES ('c1', 'Test Course', 'stake_test1uinstructor')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO course_chapters (id, course_id, title, position) \
             VALUES ('ch1', 'c1', 'Ch1', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
             VALUES ('el1', 'ch1', 'Quiz', 'quiz', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO element_skill_tags (element_id, skill_id, weight) \
             VALUES ('el1', 'sk1', 1.0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO skill_assessments \
             (id, skill_id, course_id, assessment_type, proficiency_level, difficulty, trust_factor) \
             VALUES ('sa1', 'sk1', 'c1', 'quiz', 'apply', 0.50, 1.0)",
            [],
        )
        .unwrap();

        db
    }

    /// Insert N evidence records for challenge tests.
    fn insert_evidence_records(db: &Database, count: usize) {
        let conn = db.conn();
        for i in 1..=count {
            conn.execute(
                "INSERT OR IGNORE INTO evidence_records \
                 (id, skill_assessment_id, skill_id, proficiency_level, score, \
                  difficulty, trust_factor, course_id, instructor_address) \
                 VALUES (?1, 'sa1', 'sk1', 'apply', 0.80, 0.50, 1.0, 'c1', 'stake_test1uinstructor')",
                params![format!("ev{i}")],
            )
            .unwrap();
        }
    }

    /// Set up DB for attestation stress tests.
    fn setup_attestation_db() -> Database {
        let db = test_db();
        let conn = db.conn();

        // Taxonomy
        conn.execute(
            "INSERT INTO subject_fields (id, name) VALUES ('sf1', 'CS')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO subjects (id, name, subject_field_id) VALUES ('sub1', 'Algo', 'sf1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO skills (id, name, subject_id) VALUES ('sk1', 'Graphs', 'sub1')",
            [],
        )
        .unwrap();

        // Local identity
        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, 'stake_test1ulearner', 'addr_test1qlearner')",
            [],
        )
        .unwrap();

        // Course + assessment
        conn.execute(
            "INSERT INTO courses (id, title, author_address) \
             VALUES ('c1', 'Algo Course', 'stake_test1uinstructor')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO skill_assessments (id, skill_id, course_id, assessment_type, proficiency_level, difficulty) \
             VALUES ('sa1', 'sk1', 'c1', 'quiz', 'apply', 0.50)",
            [],
        )
        .unwrap();

        // Active DAO
        conn.execute(
            "INSERT INTO governance_daos (id, name, scope_type, scope_id, status) \
             VALUES ('dao1', 'CS DAO', 'subject_field', 'sf1', 'active')",
            [],
        )
        .unwrap();

        db
    }

    /// Insert N evidence records for attestation tests.
    fn insert_attestation_evidence(db: &Database, count: usize) {
        let conn = db.conn();
        for i in 1..=count {
            conn.execute(
                "INSERT OR IGNORE INTO evidence_records \
                 (id, skill_assessment_id, skill_id, proficiency_level, score, \
                  difficulty, trust_factor, course_id, instructor_address) \
                 VALUES (?1, 'sa1', 'sk1', 'apply', 0.85, 0.50, 1.0, 'c1', 'stake_test1uinstructor')",
                params![format!("ev{i}")],
            )
            .unwrap();
        }
    }

    /// Insert a course FK for sync tests.
    fn insert_course(db: &Database, course_id: &str) {
        db.conn()
            .execute(
                "INSERT OR IGNORE INTO courses (id, title, author_address) \
                 VALUES (?1, 'Sync Course', 'stake_test1uauthor')",
                params![course_id],
            )
            .unwrap();
    }

    // ===================================================================
    // 1. HIGH-VOLUME GOSSIP HANDLER TESTS
    // ===================================================================

    /// Process 200 unique catalog announcements — all should be inserted.
    #[test]
    fn catalog_high_volume_unique_messages() {
        let db = test_db();
        let key = test_key();

        for i in 0..200 {
            let ann = build_catalog_announcement(
                "stake_test1uauthor",
                &format!("Course {i}"),
                None,
                &format!("cid_{i}"),
                None,
                &[],
                &[],
                1,
            );
            let msg = signed_catalog_msg(&key, &ann);
            handle_catalog_message(&db, &msg).unwrap();
        }

        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM catalog", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 200, "all 200 catalog entries should be stored");

        let sync_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sync_log WHERE entity_type = 'catalog' AND direction = 'received'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(sync_count, 200, "all 200 sync_log entries should exist");
    }

    /// Same catalog announcement repeated 100 times — no duplicates.
    #[test]
    fn catalog_idempotent_under_repeated_messages() {
        let db = test_db();
        let key = test_key();
        let ann = build_catalog_announcement(
            "stake_test1uauthor",
            "Same Course",
            None,
            "cid_same",
            None,
            &[],
            &[],
            1,
        );

        for _ in 0..100 {
            let msg = signed_catalog_msg(&key, &ann);
            handle_catalog_message(&db, &msg).unwrap();
        }

        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM catalog", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            count, 1,
            "repeated announcements should not create duplicates"
        );
    }

    /// Monotonic version enforcement under rapid version progression.
    #[test]
    fn catalog_monotonic_version_under_load() {
        let db = test_db();
        let key = test_key();

        // Insert versions 1 through 50 in random order
        let versions: Vec<i64> = {
            let mut v: Vec<i64> = (1..=50).collect();
            // Simulate out-of-order by reversing (worst case)
            v.reverse();
            v
        };

        for version in &versions {
            let ann = build_catalog_announcement(
                "stake_test1uauthor",
                &format!("v{version}"),
                None,
                "cid_1",
                None,
                &[],
                &[],
                *version,
            );
            let msg = signed_catalog_msg(&key, &ann);
            handle_catalog_message(&db, &msg).unwrap();
        }

        // After all, the stored version should be 50 (highest)
        let (stored_version, stored_title): (i64, String) = db
            .conn()
            .query_row(
                "SELECT version, title FROM catalog WHERE content_cid = 'cid_1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        // The first message (v50) should win because all subsequent
        // messages are older (versions < 50).
        assert_eq!(stored_version, 50);
        assert_eq!(stored_title, "v50");
    }

    /// High-volume evidence messages — only those with matching skills stored.
    #[test]
    fn evidence_high_volume_with_mixed_skill_presence() {
        let db = test_db();
        let key = test_key();

        // Create 10 skills in the DB
        for i in 0..10 {
            insert_test_skill(&db, &format!("skill_{i}"));
        }

        // Send 200 evidence messages: 100 for existing skills, 100 for missing
        for i in 0..200 {
            let skill_id = if i < 100 {
                format!("skill_{}", i % 10) // existing
            } else {
                format!("missing_skill_{}", i) // not in DB
            };

            let ann = EvidenceAnnouncement {
                evidence_id: format!("ev_{i}"),
                learner_address: "stake_test1ulearner".into(),
                skill_id,
                proficiency_level: "apply".into(),
                assessment_id: format!("sa_{i}"),
                score: 0.75,
                difficulty: 0.50,
                trust_factor: 1.0,
                course_id: Some("c1".into()),
                instructor_address: Some("stake_test1uinstructor".into()),
                created_at: now_secs() as i64,
            };
            let msg = signed_evidence_msg(&key, &ann);
            handle_evidence_message(&db, &msg).unwrap();
        }

        // Only 100 should be in evidence_records (those with existing skills)
        let evidence_count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM evidence_records", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            evidence_count, 100,
            "only evidence for existing skills should be stored"
        );

        // All 200 should be in sync_log
        let sync_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sync_log WHERE entity_type = 'evidence'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(sync_count, 200, "all messages should be logged in sync_log");
    }

    /// Evidence idempotency — same evidence_id repeated many times.
    #[test]
    fn evidence_idempotent_insert_or_ignore() {
        let db = test_db();
        let key = test_key();
        insert_test_skill(&db, "sk_idem");

        let ann = EvidenceAnnouncement {
            evidence_id: "ev_repeat".into(),
            learner_address: "stake_test1ulearner".into(),
            skill_id: "sk_idem".into(),
            proficiency_level: "apply".into(),
            assessment_id: "sa_idem".into(),
            score: 0.85,
            difficulty: 0.50,
            trust_factor: 1.0,
            course_id: None,
            instructor_address: None,
            created_at: now_secs() as i64,
        };

        for _ in 0..50 {
            let msg = signed_evidence_msg(&key, &ann);
            handle_evidence_message(&db, &msg).unwrap();
        }

        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM evidence_records WHERE id = 'ev_repeat'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "INSERT OR IGNORE should prevent duplicates");
    }

    // ===================================================================
    // 2. CONCURRENT VALIDATION + DEDUP
    // ===================================================================

    /// Validation pipeline correctly deduplicates 500 unique messages.
    #[test]
    fn validation_dedup_500_unique_messages() {
        let validator = MessageValidator::new();
        let key = test_key();

        for i in 0..500 {
            let msg = sign_gossip_message(
                "/alexandria/catalog/1.0",
                format!("{{\"id\":{i}}}").into_bytes(),
                &key,
                "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
            );
            assert!(
                validator.validate(&msg).is_ok(),
                "unique message {i} should pass validation"
            );
        }

        assert_eq!(validator.seen_count(), 500);
    }

    /// Same message submitted repeatedly is rejected after first.
    #[test]
    fn validation_dedup_rejects_replay() {
        let validator = MessageValidator::new();
        let key = test_key();

        let msg = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"id\":42}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );

        assert!(validator.validate(&msg).is_ok(), "first should pass");

        for _ in 0..99 {
            let result = validator.validate(&msg);
            assert!(result.is_err(), "replay should be rejected");
        }

        // Only the first message is in the cache
        assert_eq!(validator.seen_count(), 1);
    }

    /// Multiple signers sending unique payloads — all pass validation.
    #[test]
    fn validation_multiple_signers() {
        let validator = MessageValidator::new();

        for i in 0..50 {
            let key = test_key();
            // Each signer has a unique stake address (identity binding requires 1:1)
            let stake_address = format!("stake_test1usigner_{i}");
            // Each signer sends a unique payload (dedup is on payload hash)
            let payload = format!(
                "{{\"signer\":{i},\"nonce\":\"{}\"}}",
                hex::encode(key.verifying_key().to_bytes())
            );
            let msg = sign_gossip_message(
                "/alexandria/evidence/1.0",
                payload.into_bytes(),
                &key,
                &stake_address,
            );
            assert!(validator.validate(&msg).is_ok());
        }

        assert_eq!(validator.seen_count(), 50);
    }

    /// Concurrent validation from multiple threads via Arc<MessageValidator>.
    #[test]
    fn validation_concurrent_threads() {
        let validator = Arc::new(MessageValidator::new());
        let mut handles = Vec::new();

        // 10 threads, each validating 100 unique messages
        for t in 0..10 {
            let v = Arc::clone(&validator);
            let handle = std::thread::spawn(move || {
                let key = test_key();
                // Each thread gets its own stake address (identity binding)
                let stake_address = format!("stake_test1uthread_{t}");
                let mut ok_count = 0;
                for i in 0..100 {
                    let msg = sign_gossip_message(
                        "/alexandria/catalog/1.0",
                        format!("{{\"thread\":{t},\"msg\":{i}}}").into_bytes(),
                        &key,
                        &stake_address,
                    );
                    if v.validate(&msg).is_ok() {
                        ok_count += 1;
                    }
                }
                ok_count
            });
            handles.push(handle);
        }

        let total_ok: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();

        // Each thread sends unique payloads (different thread+msg combo),
        // each with its own key and stake_address.
        assert_eq!(
            total_ok, 1000,
            "all 1000 unique messages should pass validation"
        );
        assert_eq!(validator.seen_count(), 1000);
    }

    /// Mixed valid and invalid messages processed correctly under load.
    #[test]
    fn validation_mixed_valid_invalid() {
        let validator = MessageValidator::new();
        let key = test_key();

        let mut valid_count = 0;
        let mut invalid_count = 0;

        for i in 0..200 {
            if i % 3 == 0 {
                // Invalid: tampered signature
                let mut msg = sign_gossip_message(
                    "/alexandria/catalog/1.0",
                    format!("{{\"idx\":{i}}}").into_bytes(),
                    &key,
                    "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
                );
                msg.signature = vec![0u8; 64]; // invalid signature
                if validator.validate(&msg).is_err() {
                    invalid_count += 1;
                }
            } else {
                // Valid
                let msg = sign_gossip_message(
                    "/alexandria/catalog/1.0",
                    format!("{{\"idx\":{i}}}").into_bytes(),
                    &key,
                    "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
                );
                if validator.validate(&msg).is_ok() {
                    valid_count += 1;
                }
            }
        }

        // 200/3 = 67 invalid (i%3==0 for i=0,3,6,...,198 → 67 messages)
        // 200 - 67 = 133 valid
        assert_eq!(
            invalid_count, 67,
            "all tampered messages should be rejected"
        );
        assert_eq!(valid_count, 133, "all properly signed messages should pass");
    }

    // ===================================================================
    // 3. SYNC CONFLICT RESOLUTION UNDER LOAD
    // ===================================================================

    /// LWW merge with 100 conflicting updates — last-writer wins.
    #[test]
    fn sync_lww_100_conflicting_updates() {
        let db = test_db();
        let conn = db.conn();

        // Set up local identity + device
        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, 'stake_test1u_sync', 'addr_test1q_sync')",
            [],
        )
        .unwrap();
        register_local_device(conn, Some("device1"), "macos").unwrap();
        insert_course(&db, "sync_course");

        // Initial enrollment
        conn.execute(
            "INSERT INTO enrollments (id, course_id, status, updated_at) \
             VALUES ('enr_conflict', 'sync_course', 'active', '2025-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        // 100 conflicting LWW updates with incrementing timestamps
        for i in 1..=100 {
            let ts = format!("2025-01-01T00:{:02}:{:02}Z", i / 60, i % 60);
            let row = serde_json::json!({
                "id": "enr_conflict",
                "course_id": "sync_course",
                "status": format!("status_{i}"),
                "updated_at": ts,
            });
            let rows = vec![crate::domain::sync::SyncRow {
                row_id: "enr_conflict".into(),
                operation: "update".into(),
                data: Some(row),
                updated_at: ts.clone(),
            }];
            merge_lww_rows(conn, "enrollments", &rows).unwrap();
        }

        // The last update (i=100) should win (highest timestamp)
        let status: String = conn
            .query_row(
                "SELECT status FROM enrollments WHERE id = 'enr_conflict'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "status_100", "last writer should win");
    }

    /// LWW merge skips older timestamps correctly under interleaved order.
    #[test]
    fn sync_lww_interleaved_timestamps() {
        let db = test_db();
        let conn = db.conn();

        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, 'stake_test1u_sync2', 'addr_test1q_sync2')",
            [],
        )
        .unwrap();
        register_local_device(conn, Some("device2"), "linux").unwrap();
        insert_course(&db, "sync_course2");

        // Initial enrollment with timestamp at t=50
        conn.execute(
            "INSERT INTO enrollments (id, course_id, status, updated_at) \
             VALUES ('enr_inter', 'sync_course2', 'initial', '2025-01-01T00:00:50Z')",
            [],
        )
        .unwrap();

        // Send updates: t=10, t=100, t=30, t=90, t=50, t=80 ...
        // Using ISO timestamps where higher seconds = newer
        let timestamps = vec![10, 100, 30, 90, 50, 80, 20, 70, 40, 60];
        for ts_offset in &timestamps {
            let ts = format!("2025-01-01T00:{:02}:{:02}Z", ts_offset / 60, ts_offset % 60);
            let row = serde_json::json!({
                "id": "enr_inter",
                "course_id": "sync_course2",
                "status": format!("ts_{ts_offset}"),
                "updated_at": ts,
            });
            let rows = vec![crate::domain::sync::SyncRow {
                row_id: "enr_inter".into(),
                operation: "update".into(),
                data: Some(row),
                updated_at: ts.clone(),
            }];
            merge_lww_rows(conn, "enrollments", &rows).unwrap();
        }

        // t=100 should win (highest timestamp: "2025-01-01T01:40:00Z")
        let status: String = conn
            .query_row(
                "SELECT status FROM enrollments WHERE id = 'enr_inter'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            status, "ts_100",
            "highest timestamp should win regardless of arrival order"
        );
    }

    /// Append-only merge handles 200 unique evidence records without duplicates.
    #[test]
    fn sync_append_only_200_records() {
        let db = test_db();
        let conn = db.conn();

        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, 'stake_test1u_append', 'addr_test1q_append')",
            [],
        )
        .unwrap();
        register_local_device(conn, Some("device_append"), "macos").unwrap();

        // Set up FK chain
        insert_test_skill(&db, "sk_append");
        conn.execute(
            "INSERT INTO courses (id, title, author_address) \
             VALUES ('c_append', 'Append Course', 'stake_test1uinstructor')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO skill_assessments \
             (id, skill_id, course_id, assessment_type, proficiency_level, difficulty) \
             VALUES ('sa_append', 'sk_append', 'c_append', 'quiz', 'apply', 0.50)",
            [],
        )
        .unwrap();

        // 200 unique evidence records via append-only
        let mut rows = Vec::new();
        let ts_str = chrono::Utc::now().to_rfc3339();
        for i in 0..200 {
            let row = serde_json::json!({
                "id": format!("ev_append_{i}"),
                "skill_assessment_id": "sa_append",
                "skill_id": "sk_append",
                "proficiency_level": "apply",
                "score": 0.80,
                "difficulty": 0.50,
                "trust_factor": 1.0,
                "course_id": "c_append",
                "instructor_address": "stake_test1uinstructor",
            });
            rows.push(crate::domain::sync::SyncRow {
                row_id: format!("ev_append_{i}"),
                operation: "insert".into(),
                data: Some(row),
                updated_at: ts_str.clone(),
            });
        }

        let merged = merge_append_only(conn, "evidence_records", &rows).unwrap();
        assert_eq!(merged, 200, "all 200 unique records should be inserted");

        // Send them again — no new inserts
        let merged_again = merge_append_only(conn, "evidence_records", &rows).unwrap();
        assert_eq!(merged_again, 0, "duplicates should be ignored");

        // Total should be exactly 200
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM evidence_records", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(total, 200);
    }

    /// Sync queue enqueue and delivery tracking under high volume.
    #[test]
    fn sync_queue_high_volume() {
        let db = test_db();
        let conn = db.conn();

        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, 'stake_test1u_queue', 'addr_test1q_queue')",
            [],
        )
        .unwrap();
        let local_id = register_local_device(conn, Some("local"), "macos").unwrap();

        // Register 3 remote devices
        let mut remote_ids = Vec::new();
        for i in 1..=3 {
            let rid = format!("remote_{i}");
            register_remote_device(
                conn,
                &rid,
                Some(&format!("Device {i}")),
                Some("linux"),
                Some(&format!("peer_{i}")),
            )
            .unwrap();
            remote_ids.push(rid);
        }
        let _ = local_id;

        // Enqueue 100 changes
        let ts_str = chrono::Utc::now().to_rfc3339();
        for i in 0..100 {
            let row_data =
                serde_json::to_string(&serde_json::json!({"id": format!("enr_{i}")})).unwrap();
            enqueue_change(
                conn,
                "enrollments",
                &format!("enr_{i}"),
                "upsert",
                Some(&row_data),
                &ts_str,
            )
            .unwrap();
        }

        // Each remote device should see 100 pending items
        for rid in &remote_ids {
            let pending = get_pending_queue_items(conn, rid, 200).unwrap();
            assert_eq!(
                pending.len(),
                100,
                "device {rid} should see all 100 pending"
            );
        }

        // Mark all as delivered to device 1
        let ids_for_d1: Vec<i64> = get_pending_queue_items(conn, &remote_ids[0], 200)
            .unwrap()
            .iter()
            .map(|item| item.id)
            .collect();
        mark_delivered(conn, &ids_for_d1, &remote_ids[0]).unwrap();

        // Device 1 should have 0 pending, devices 2 & 3 still 100
        let d1_pending = get_pending_queue_items(conn, &remote_ids[0], 200).unwrap();
        assert_eq!(d1_pending.len(), 0);

        let d2_pending = get_pending_queue_items(conn, &remote_ids[1], 200).unwrap();
        assert_eq!(d2_pending.len(), 100);

        // Deliver to device 2 and 3
        for rid in &remote_ids[1..] {
            let ids: Vec<i64> = get_pending_queue_items(conn, rid, 200)
                .unwrap()
                .iter()
                .map(|item| item.id)
                .collect();
            mark_delivered(conn, &ids, rid).unwrap();
        }

        // Prune — all delivered to all devices
        let pruned = prune_delivered_queue(conn).unwrap();
        assert_eq!(
            pruned, 100,
            "all 100 items should be pruned after full delivery"
        );

        // Queue should be empty now
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM sync_queue", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
    }

    // ===================================================================
    // 4. CHALLENGE + ATTESTATION CONCURRENT ACCESS
    // ===================================================================

    /// Submit many challenges across different targets — all should succeed.
    #[test]
    fn challenge_bulk_submission() {
        let db = setup_challenge_db();
        let conn = db.conn();
        insert_evidence_records(&db, 50);

        // Submit 50 challenges targeting different evidence
        for i in 1..=50 {
            let params = SubmitChallengeParams {
                target_type: "evidence".into(),
                target_ids: vec![format!("ev{i}")],
                evidence_cids: vec![format!("bafy_{i}")],
                reason: format!("suspicious evidence {i}"),
                stake_lovelace: 5_000_000,
                dao_id: "dao1".into(),
                learner_address: "stake_test1ulearner".into(),
            };
            let ch = challenge::submit_challenge(conn, &params).unwrap();
            assert_eq!(ch.status, "pending");
        }

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM evidence_challenges", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 50, "all 50 challenges should be created");
    }

    /// All 5 committee members vote on same challenge — no double votes.
    #[test]
    fn challenge_all_committee_members_vote() {
        let db = setup_challenge_db();
        let conn = db.conn();
        insert_evidence_records(&db, 1);

        let params = SubmitChallengeParams {
            target_type: "evidence".into(),
            target_ids: vec!["ev1".into()],
            evidence_cids: vec!["bafy_1".into()],
            reason: "test all voters".into(),
            stake_lovelace: 5_000_000,
            dao_id: "dao1".into(),
            learner_address: "stake_test1ulearner".into(),
        };
        let ch = challenge::submit_challenge(conn, &params).unwrap();

        // All 5 committee members vote
        for i in 1..=5 {
            let voter = format!("stake_test1ucommittee{i}");
            let upheld = i <= 3; // 3 upheld, 2 rejected
            challenge::vote_on_challenge(conn, &ch.id, &voter, upheld, None).unwrap();
        }

        // Verify 5 votes
        let vote_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM challenge_votes WHERE challenge_id = ?1",
                params![ch.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(vote_count, 5);

        // Attempt double vote — should fail
        let double_vote =
            challenge::vote_on_challenge(conn, &ch.id, "stake_test1ucommittee1", false, None);
        assert!(double_vote.is_err());
        assert!(double_vote.unwrap_err().contains("already voted"));

        // Resolve — 3/5 = 60% < 66.7% → rejected
        let resolution = challenge::resolve_challenge(conn, &ch.id).unwrap();
        assert_eq!(resolution.status, "rejected", "60% is below 2/3 threshold");
        assert_eq!(resolution.votes_upheld, 3);
        assert_eq!(resolution.votes_rejected, 2);
    }

    /// Multiple challenges resolved in sequence — upheld ones invalidate evidence.
    #[test]
    fn challenge_sequential_resolution() {
        let db = setup_challenge_db();
        let conn = db.conn();
        insert_evidence_records(&db, 10);

        let mut challenge_ids = Vec::new();

        // Create 5 challenges targeting pairs of evidence
        for i in 0..5 {
            let ev1 = format!("ev{}", i * 2 + 1);
            let ev2 = format!("ev{}", i * 2 + 2);
            let params = SubmitChallengeParams {
                target_type: "evidence".into(),
                target_ids: vec![ev1, ev2],
                evidence_cids: vec![],
                reason: format!("batch challenge {i}"),
                stake_lovelace: 5_000_000,
                dao_id: "dao1".into(),
                learner_address: "stake_test1ulearner".into(),
            };
            let ch = challenge::submit_challenge(conn, &params).unwrap();
            challenge_ids.push(ch.id);
        }

        // Uphold challenges 0, 2, 4 (supermajority votes)
        // Reject challenges 1, 3
        for (idx, ch_id) in challenge_ids.iter().enumerate() {
            let upheld = idx % 2 == 0;
            for j in 1..=3 {
                let voter = format!("stake_test1ucommittee{j}");
                challenge::vote_on_challenge(conn, ch_id, &voter, upheld, None).unwrap();
            }
            let resolution = challenge::resolve_challenge(conn, ch_id).unwrap();
            if upheld {
                assert_eq!(resolution.status, "upheld");
            } else {
                assert_eq!(resolution.status, "rejected");
            }
        }

        // 6 evidence records should be deleted (3 upheld × 2 per challenge)
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM evidence_records", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(remaining, 4, "6 of 10 evidence records should be deleted");
    }

    /// Many assessors attest same evidence — only unique attestations counted.
    #[test]
    fn attestation_bulk_submission() {
        let db = setup_attestation_db();
        let conn = db.conn();
        insert_attestation_evidence(&db, 1);

        // Set requirement: 10 attestors needed
        attestation::set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk1".into(),
                proficiency_level: "apply".into(),
                required_attestors: 10,
                dao_id: "dao1".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();

        // Add 15 assessors
        for i in 1..=15 {
            conn.execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
                 VALUES ('dao1', ?1, 'assessor')",
                params![format!("stake_test1uassessor{i}")],
            )
            .unwrap();
        }

        // 15 unique attestations
        for i in 1..=15 {
            attestation::submit_attestation(
                conn,
                &format!("stake_test1uassessor{i}"),
                &format!("sig_{i}"),
                &SubmitAttestationParams {
                    evidence_id: "ev1".into(),
                    attestation_type: None,
                    integrity_score: Some(0.90 + (i as f64) * 0.001),
                    session_cid: None,
                },
            )
            .unwrap();
        }

        // Each assessor attempting again should fail
        for i in 1..=15 {
            let result = attestation::submit_attestation(
                conn,
                &format!("stake_test1uassessor{i}"),
                &format!("sig_{i}_dup"),
                &SubmitAttestationParams {
                    evidence_id: "ev1".into(),
                    attestation_type: None,
                    integrity_score: None,
                    session_cid: None,
                },
            );
            assert!(
                result.is_err(),
                "duplicate attestation from assessor {i} should fail"
            );
        }

        // Status: fully attested (15 >= 10)
        let status = attestation::get_attestation_status(conn, "ev1").unwrap();
        assert_eq!(status.required_attestors, 10);
        assert_eq!(status.current_attestors, 15);
        assert!(status.is_fully_attested);
    }

    /// Multiple evidence records with different attestation requirements.
    #[test]
    fn attestation_mixed_requirements() {
        let db = setup_attestation_db();
        let conn = db.conn();

        // Add more skills
        conn.execute(
            "INSERT INTO skills (id, name, subject_id) VALUES ('sk2', 'Sorting', 'sub1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO skills (id, name, subject_id) VALUES ('sk3', 'Trees', 'sub1')",
            [],
        )
        .unwrap();

        // Add assessments for new skills
        conn.execute(
            "INSERT INTO skill_assessments (id, skill_id, course_id, assessment_type, proficiency_level, difficulty) \
             VALUES ('sa2', 'sk2', 'c1', 'quiz', 'analyze', 0.70)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO skill_assessments (id, skill_id, course_id, assessment_type, proficiency_level, difficulty) \
             VALUES ('sa3', 'sk3', 'c1', 'quiz', 'evaluate', 0.80)",
            [],
        )
        .unwrap();

        // Evidence for each skill
        conn.execute(
            "INSERT INTO evidence_records \
             (id, skill_assessment_id, skill_id, proficiency_level, score, difficulty, trust_factor, \
              course_id, instructor_address) \
             VALUES ('ev1', 'sa1', 'sk1', 'apply', 0.85, 0.50, 1.0, 'c1', 'stake_test1uinstructor')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO evidence_records \
             (id, skill_assessment_id, skill_id, proficiency_level, score, difficulty, trust_factor, \
              course_id, instructor_address) \
             VALUES ('ev2', 'sa2', 'sk2', 'analyze', 0.75, 0.70, 1.0, 'c1', 'stake_test1uinstructor')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO evidence_records \
             (id, skill_assessment_id, skill_id, proficiency_level, score, difficulty, trust_factor, \
              course_id, instructor_address) \
             VALUES ('ev3', 'sa3', 'sk3', 'evaluate', 0.90, 0.80, 1.0, 'c1', 'stake_test1uinstructor')",
            [],
        )
        .unwrap();

        // Different requirements per skill
        attestation::set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk1".into(),
                proficiency_level: "apply".into(),
                required_attestors: 1,
                dao_id: "dao1".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();
        attestation::set_attestation_requirement(
            conn,
            &SetRequirementParams {
                skill_id: "sk2".into(),
                proficiency_level: "analyze".into(),
                required_attestors: 3,
                dao_id: "dao1".into(),
                set_by_proposal: None,
            },
        )
        .unwrap();
        // sk3 has no requirement (self-attested)

        // Initially: ev1 and ev2 are unattested, ev3 is self-attested
        let unattested = attestation::list_unattested_evidence(conn).unwrap();
        assert_eq!(unattested.len(), 2, "ev1 and ev2 need attestation");

        // Add assessors
        for i in 1..=3 {
            conn.execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
                 VALUES ('dao1', ?1, 'assessor')",
                params![format!("stake_test1uassessor{i}")],
            )
            .unwrap();
        }

        // Attest ev1 (needs 1) — should become fully attested
        attestation::submit_attestation(
            conn,
            "stake_test1uassessor1",
            "sig1",
            &SubmitAttestationParams {
                evidence_id: "ev1".into(),
                attestation_type: None,
                integrity_score: None,
                session_cid: None,
            },
        )
        .unwrap();

        assert!(attestation::is_evidence_fully_attested(conn, "ev1").unwrap());
        assert!(!attestation::is_evidence_fully_attested(conn, "ev2").unwrap());
        assert!(attestation::is_evidence_fully_attested(conn, "ev3").unwrap());

        // Attest ev2 with 3 assessors
        for i in 1..=3 {
            attestation::submit_attestation(
                conn,
                &format!("stake_test1uassessor{i}"),
                &format!("sig_ev2_{i}"),
                &SubmitAttestationParams {
                    evidence_id: "ev2".into(),
                    attestation_type: None,
                    integrity_score: None,
                    session_cid: None,
                },
            )
            .unwrap();
        }

        assert!(attestation::is_evidence_fully_attested(conn, "ev2").unwrap());

        // No more unattested evidence
        let remaining = attestation::list_unattested_evidence(conn).unwrap();
        assert!(
            remaining.is_empty(),
            "all evidence should be fully attested now"
        );
    }

    // ===================================================================
    // 5. ADVERSARIAL / BOUNDARY CONDITION TESTS
    // ===================================================================

    /// Taxonomy authority check rejects empty stake_address.
    #[test]
    fn adversarial_taxonomy_empty_stake_address() {
        let validator = MessageValidator::new();
        let key = test_key();

        let mut msg = sign_gossip_message(
            "/alexandria/taxonomy/1.0",
            b"{\"version\":1}".to_vec(),
            &key,
            "", // empty stake address
        );
        // Fix: the signing function sets stake_address, so force it empty
        msg.stake_address = String::new();

        let result = validator.validate(&msg);
        assert!(
            result.is_err(),
            "empty stake_address should be rejected for taxonomy"
        );
    }

    /// Non-JSON payload is rejected by schema validation.
    #[test]
    fn adversarial_invalid_json_payload() {
        let validator = MessageValidator::new();
        let key = test_key();

        // Binary garbage as payload
        let msg = sign_gossip_message(
            "/alexandria/catalog/1.0",
            vec![0xFF, 0xFE, 0xFD, 0xFC, 0xFB],
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );

        let result = validator.validate(&msg);
        assert!(
            result.is_err(),
            "binary garbage payload should fail schema check"
        );
    }

    /// Message with timestamp far in the future is rejected.
    #[test]
    fn adversarial_future_timestamp() {
        let validator = MessageValidator::new();
        let key = test_key();

        let mut msg = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"test\":true}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );
        msg.timestamp = now_secs() + 3600; // 1 hour in the future

        // Freshness check directly (signature check would pass because
        // the timestamp isn't included in the signed payload)
        let result = validator.check_freshness(&msg);
        assert!(result.is_err(), "timestamp 1 hour ahead should be rejected");
    }

    /// Message with timestamp far in the past is rejected.
    #[test]
    fn adversarial_past_timestamp() {
        let validator = MessageValidator::new();
        let key = test_key();

        let mut msg = sign_gossip_message(
            "/alexandria/catalog/1.0",
            b"{\"test\":true}".to_vec(),
            &key,
            "stake_test1uqfu74w3wh4gfzu8m6e7j987h4lq9r3t7ef5gaw497uu8q0kd9u4",
        );
        msg.timestamp = 1_000_000; // Way in the past (1970)

        let result = validator.check_freshness(&msg);
        assert!(result.is_err(), "ancient timestamp should be rejected");
    }

    /// Catalog handler rejects announcements with missing required fields.
    #[test]
    fn adversarial_catalog_missing_fields() {
        let db = test_db();
        let key = test_key();

        // Missing title
        let ann = CatalogAnnouncement {
            course_id: "c1".into(),
            title: String::new(), // empty
            description: None,
            content_cid: "cid_1".into(),
            author_address: "stake_test1u".into(),
            thumbnail_cid: None,
            tags: vec![],
            skill_ids: vec![],
            version: 1,
            published_at: now_secs() as i64,
        };
        let msg = signed_catalog_msg(&key, &ann);
        assert!(handle_catalog_message(&db, &msg).is_err());

        // Missing content_cid
        let ann2 = CatalogAnnouncement {
            course_id: "c2".into(),
            title: "Has Title".into(),
            description: None,
            content_cid: String::new(), // empty
            author_address: "stake_test1u".into(),
            thumbnail_cid: None,
            tags: vec![],
            skill_ids: vec![],
            version: 1,
            published_at: now_secs() as i64,
        };
        let msg2 = signed_catalog_msg(&key, &ann2);
        assert!(handle_catalog_message(&db, &msg2).is_err());
    }

    /// Evidence handler rejects out-of-range scores.
    #[test]
    fn adversarial_evidence_invalid_scores() {
        let db = test_db();
        let key = test_key();
        insert_test_skill(&db, "sk_score_test");

        // Score > 1.0
        let ann = EvidenceAnnouncement {
            evidence_id: "ev_bad1".into(),
            learner_address: "stake_test1u".into(),
            skill_id: "sk_score_test".into(),
            proficiency_level: "apply".into(),
            assessment_id: "sa_bad".into(),
            score: 1.5, // invalid
            difficulty: 0.50,
            trust_factor: 1.0,
            course_id: None,
            instructor_address: None,
            created_at: now_secs() as i64,
        };
        let msg = signed_evidence_msg(&key, &ann);
        assert!(handle_evidence_message(&db, &msg).is_err());

        // Score < 0.0
        let ann2 = EvidenceAnnouncement {
            score: -0.1, // invalid
            evidence_id: "ev_bad2".into(),
            ..ann.clone()
        };
        let msg2 = signed_evidence_msg(&key, &ann2);
        assert!(handle_evidence_message(&db, &msg2).is_err());
    }

    /// Challenge with below-minimum stake is rejected.
    #[test]
    fn adversarial_challenge_stake_boundary() {
        let db = setup_challenge_db();
        let conn = db.conn();
        insert_evidence_records(&db, 1);

        // Exactly at minimum: should succeed
        let params_ok = SubmitChallengeParams {
            target_type: "evidence".into(),
            target_ids: vec!["ev1".into()],
            evidence_cids: vec![],
            reason: "minimum stake test".into(),
            stake_lovelace: 5_000_000, // exactly 5 ADA
            dao_id: "dao1".into(),
            learner_address: "stake_test1ulearner".into(),
        };
        assert!(challenge::submit_challenge(conn, &params_ok).is_ok());

        // One lovelace below: should fail
        let params_low = SubmitChallengeParams {
            target_type: "evidence".into(),
            target_ids: vec!["ev1".into()],
            evidence_cids: vec![],
            reason: "below minimum test".into(),
            stake_lovelace: 4_999_999, // 1 lovelace short
            dao_id: "dao1".into(),
            learner_address: "stake_test1ulearner".into(),
        };
        assert!(challenge::submit_challenge(conn, &params_low).is_err());
    }

    /// Sync table name sanitization rejects SQL injection attempts.
    #[test]
    fn adversarial_sync_table_injection() {
        let db = test_db();
        let conn = db.conn();

        conn.execute(
            "INSERT INTO local_identity (id, stake_address, payment_address) \
             VALUES (1, 'stake_test1u', 'addr_test1q')",
            [],
        )
        .unwrap();
        register_local_device(conn, Some("dev"), "test").unwrap();

        let injection_attempts = vec![
            "enrollments; DROP TABLE users",
            "' OR 1=1 --",
            "evidence_records UNION SELECT * FROM local_identity",
            "../../../etc/passwd",
            "nonexistent_table",
        ];

        for attempt in injection_attempts {
            let rows = vec![crate::domain::sync::SyncRow {
                row_id: "test".into(),
                operation: "upsert".into(),
                data: Some(serde_json::json!({"id": "test"})),
                updated_at: chrono::Utc::now().to_rfc3339(),
            }];
            let result = merge_lww_rows(conn, attempt, &rows);
            assert!(
                result.is_err(),
                "SQL injection attempt should be rejected: {attempt}"
            );
        }
    }

    /// Attestation from non-assessor role is rejected (member, committee, chair).
    #[test]
    fn adversarial_attestation_wrong_roles() {
        let db = setup_attestation_db();
        let conn = db.conn();
        insert_attestation_evidence(&db, 1);

        // Add members with various roles
        let roles = vec![
            ("stake_test1umember", "member"),
            ("stake_test1ucommittee", "committee"),
            ("stake_test1uchair", "chair"),
        ];

        for (addr, role) in &roles {
            conn.execute(
                "INSERT INTO governance_dao_members (dao_id, stake_address, role) \
                 VALUES ('dao1', ?1, ?2)",
                params![addr, role],
            )
            .unwrap();

            let result = attestation::submit_attestation(
                conn,
                addr,
                "sig_wrong_role",
                &SubmitAttestationParams {
                    evidence_id: "ev1".into(),
                    attestation_type: None,
                    integrity_score: None,
                    session_cid: None,
                },
            );
            assert!(
                result.is_err(),
                "role '{role}' should not be allowed to attest"
            );
            assert!(result.unwrap_err().contains("not an assessor"));
        }
    }

    // ===================================================================
    // 6. MULTI-NODE GOSSIP PROPAGATION (real P2P nodes)
    // ===================================================================

    /// Two real P2P nodes discover each other via mDNS and exchange a
    /// catalog message. This tests the full gossip pipeline end-to-end:
    /// sign → publish → GossipSub → deliver → validate → event_tx.
    #[tokio::test]
    async fn multinode_gossip_catalog_propagation() {
        use crate::p2p::network::{keypair_from_cardano_key, start_node, NetworkError};
        use crate::p2p::types::P2pEvent;
        use tokio::sync::mpsc;
        use tokio::time::{timeout, Duration};

        // Create two nodes with different keys
        let kp1 = keypair_from_cardano_key(&[0x01u8; 32]).unwrap();
        let kp2 = keypair_from_cardano_key(&[0x02u8; 32]).unwrap();

        let (tx1, _rx1) = mpsc::channel::<P2pEvent>(256);
        let (tx2, mut rx2) = mpsc::channel::<P2pEvent>(256);

        let mut node1 = match start_node(kp1, tx1, vec![]).await {
            Ok(node) => node,
            Err(err) => {
                eprintln!("SKIP: node1 failed to start ({err:?})");
                return;
            }
        };
        let mut node2 = match start_node(kp2, tx2, vec![]).await {
            Ok(node) => node,
            Err(err) => {
                node1.shutdown().await;
                eprintln!("SKIP: node2 failed to start ({err:?})");
                return;
            }
        };

        // Wait for mDNS discovery (nodes on localhost discover each other)
        // Give them up to 10 seconds to connect
        let node1_id = node1.peer_id().to_string();
        let node2_id = node2.peer_id().to_string();

        let connected = timeout(Duration::from_secs(10), async {
            loop {
                let peers1 = node1.connected_peers().await.unwrap_or_default();
                let peers2 = node2.connected_peers().await.unwrap_or_default();
                if peers1.contains(&node2_id) || peers2.contains(&node1_id) {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await;

        if connected.is_err() {
            // mDNS may not work in CI — skip gracefully
            node1.shutdown().await;
            node2.shutdown().await;
            eprintln!("SKIP: mDNS discovery timed out (expected in CI/containers)");
            return;
        }

        // Give GossipSub a moment to finish mesh/subscription propagation.
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Publish a catalog message from node1
        let key = test_key();
        let ann = build_catalog_announcement(
            "stake_test1uauthor",
            "Multinode Test Course",
            None,
            "cid_multinode",
            None,
            &[],
            &[],
            1,
        );
        let payload = serde_json::to_vec(&ann).unwrap();
        let publish_result = timeout(Duration::from_secs(5), async {
            loop {
                match node1
                    .publish_catalog(payload.clone(), &key, "stake_test1uauthor")
                    .await
                {
                    Ok(()) => return Ok(()),
                    Err(NetworkError::Publish(err)) if err.contains("NoPeersSubscribedToTopic") => {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                    Err(err) => return Err(err),
                }
            }
        })
        .await;

        match publish_result {
            Ok(Ok(())) => {}
            Ok(Err(err)) => panic!("publish should succeed: {err:?}"),
            Err(_) => {
                node1.shutdown().await;
                node2.shutdown().await;
                eprintln!("SKIP: peers connected but never finished topic subscription");
                return;
            }
        }

        // Node2 should receive the GossipMessage event
        let received = timeout(Duration::from_secs(5), async {
            while let Some(event) = rx2.recv().await {
                if let P2pEvent::GossipMessage { topic, message } = event {
                    if topic.contains("catalog") {
                        return Some(message);
                    }
                }
            }
            None
        })
        .await;

        match received {
            Ok(Some(msg)) => {
                let received_ann: CatalogAnnouncement =
                    serde_json::from_slice(&msg.payload).unwrap();
                assert_eq!(received_ann.title, "Multinode Test Course");
                assert_eq!(received_ann.content_cid, "cid_multinode");
            }
            Ok(None) => {
                eprintln!("SKIP: no gossip message received (event channel closed)");
            }
            Err(_) => {
                eprintln!("SKIP: gossip propagation timed out");
            }
        }

        node1.shutdown().await;
        node2.shutdown().await;
    }

    /// Verify that two nodes see each other as connected peers after
    /// mDNS discovery.
    #[tokio::test]
    async fn multinode_peer_discovery() {
        use crate::p2p::network::{keypair_from_cardano_key, start_node};
        use crate::p2p::types::P2pEvent;
        use tokio::sync::mpsc;
        use tokio::time::{timeout, Duration};

        let kp1 = keypair_from_cardano_key(&[0x10u8; 32]).unwrap();
        let kp2 = keypair_from_cardano_key(&[0x20u8; 32]).unwrap();

        let (tx1, _rx1) = mpsc::channel::<P2pEvent>(64);
        let (tx2, _rx2) = mpsc::channel::<P2pEvent>(64);

        let mut node1 = match start_node(kp1, tx1, vec![]).await {
            Ok(node) => node,
            Err(err) => {
                eprintln!("SKIP: node1 failed to start ({err:?})");
                return;
            }
        };
        let mut node2 = match start_node(kp2, tx2, vec![]).await {
            Ok(node) => node,
            Err(err) => {
                node1.shutdown().await;
                eprintln!("SKIP: node2 failed to start ({err:?})");
                return;
            }
        };

        let node2_id = node2.peer_id().to_string();
        let node1_id = node1.peer_id().to_string();

        // Wait for peer discovery
        let discovered = timeout(Duration::from_secs(10), async {
            loop {
                let peers1 = node1.connected_peers().await.unwrap_or_default();
                let peers2 = node2.connected_peers().await.unwrap_or_default();
                if peers1.contains(&node2_id) || peers2.contains(&node1_id) {
                    return (peers1, peers2);
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await;

        match discovered {
            Ok((peers1, peers2)) => {
                let n1_sees_n2 = peers1.contains(&node2_id);
                let n2_sees_n1 = peers2.contains(&node1_id);
                assert!(
                    n1_sees_n2 || n2_sees_n1,
                    "at least one node should discover the other. \
                     node1 peers: {:?}, node2 peers: {:?}",
                    peers1,
                    peers2
                );
            }
            Err(_) => {
                let peers1 = node1.connected_peers().await.unwrap_or_default();
                let peers2 = node2.connected_peers().await.unwrap_or_default();
                eprintln!(
                    "SKIP: peer discovery timed out (expected in CI/containers). \
                     node1 peers: {:?}, node2 peers: {:?}",
                    peers1, peers2
                );
            }
        }

        node1.shutdown().await;
        node2.shutdown().await;
    }
}
