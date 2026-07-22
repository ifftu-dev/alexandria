//! Tests for schema migrations.
//!
//! These live outside `schema.rs` because the `alex` CLI `#[path]`-includes
//! that file into a crate with no `crate::db`, so a test module referencing
//! it there would break the CLI build. Migration SQL is reached through the
//! public `MIGRATIONS` table by version rather than by private const, which
//! also means these tests exercise exactly what the migrator runs.

use super::schema::MIGRATIONS;
use crate::db::Database;

/// Apply migration 072 again over rows inserted afterwards.
///
/// The migration is written to be idempotent — `CREATE TABLE IF NOT
/// EXISTS` plus `INSERT OR IGNORE` on preserved ids — so re-running it
/// exercises the real SQL rather than a copy that could drift from it.
fn rerun_migration_072(db: &Database) {
    let (_, _, sql) = MIGRATIONS
        .iter()
        .find(|(v, _, _)| *v == 72)
        .expect("migration 072 exists");
    db.conn()
        .execute_batch(sql)
        .expect("migration 072 re-runs cleanly");
}

fn seed_bank(db: &Database) {
    db.conn()
        .execute_batch(
            r#"
            INSERT INTO question_banks (id, skill_id, label, taxonomy_version, ratified)
            VALUES ('bank_1', 'skill_rust', 'Rust basics', 'genesis', 1);

            INSERT INTO bank_questions
                (id, bank_id, prompt, options, correct_indices, difficulty, points)
            VALUES
                ('q_single', 'bank_1', 'Which is a keyword?',
                 '["fn","func","def","lambda"]', '[0]', 2, 1.0),
                ('q_multi', 'bank_1', 'Which are integer types?',
                 '["i32","u8","str","bool"]', '[0,1]', 4, 2.0);
            "#,
        )
        .expect("seed bank");
}

fn item_field(db: &Database, id: &str, sql: &str) -> String {
    db.conn()
        .query_row(sql, [id], |r| r.get::<_, String>(0))
        .expect("item field")
}

#[test]
fn backfill_preserves_question_ids() {
    // `assessment_attempts.question_ids` stores bank-question ids, so a
    // remapped id would orphan every historical attempt.
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    seed_bank(&db);
    rerun_migration_072(&db);

    let count: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM assessment_items WHERE id IN ('q_single','q_multi')",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(count, 2, "both questions became items under their own ids");
}

#[test]
fn backfill_derives_kind_from_key_cardinality() {
    // The wasm grader dispatches on `kind`; getting this wrong would
    // silently score every multi-select question as single-answer.
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    seed_bank(&db);
    rerun_migration_072(&db);

    let kind_of = |id: &str| {
        item_field(
            &db,
            id,
            "SELECT json_extract(content_public, '$.kind') FROM assessment_items WHERE id = ?1",
        )
    };
    assert_eq!(kind_of("q_single"), "single");
    assert_eq!(kind_of("q_multi"), "multi");
}

#[test]
fn backfill_moves_the_key_into_grader_private() {
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    seed_bank(&db);
    rerun_migration_072(&db);

    let key = item_field(
        &db,
        "q_multi",
        "SELECT json_extract(grader_private, '$.correct_indices') FROM assessment_items \
         WHERE id = ?1",
    );
    assert_eq!(key, "[0,1]");

    // And it is absent from the half that may reach a client.
    let public = item_field(
        &db,
        "q_multi",
        "SELECT content_public FROM assessment_items WHERE id = ?1",
    );
    assert!(
        !public.contains("correct_indices"),
        "content_public leaks the answer key: {public}"
    );
}

#[test]
fn backfill_carries_prompt_options_difficulty_and_points() {
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    seed_bank(&db);
    rerun_migration_072(&db);

    let (difficulty, points, bank): (i64, f64, String) = db
        .conn()
        .query_row(
            "SELECT difficulty, points, bank_id FROM assessment_items WHERE id = 'q_multi'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("row");
    assert_eq!(difficulty, 4);
    assert_eq!(points, 2.0);
    assert_eq!(bank, "bank_1");

    let options = item_field(
        &db,
        "q_multi",
        "SELECT json_extract(content_public, '$.options') FROM assessment_items WHERE id = ?1",
    );
    assert_eq!(options, r#"["i32","u8","str","bool"]"#);

    let prompt = item_field(
        &db,
        "q_multi",
        "SELECT json_extract(content_public, '$.prompt') FROM assessment_items WHERE id = ?1",
    );
    assert_eq!(prompt, "Which are integer types?");
}

#[test]
fn backfill_inherits_skill_and_ratification_from_the_bank() {
    // An item must not become usable for credentials just by existing —
    // it inherits the bank's governance state.
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    seed_bank(&db);
    db.conn()
        .execute_batch(
            "INSERT INTO question_banks (id, skill_id, label, ratified)
             VALUES ('bank_draft', 'skill_go', 'Draft', 0);
             INSERT INTO bank_questions (id, bank_id, prompt, options, correct_indices)
             VALUES ('q_draft', 'bank_draft', 'p', '[\"a\",\"b\"]', '[0]');",
        )
        .expect("seed draft bank");
    rerun_migration_072(&db);

    let (skill, ratified): (String, i64) = db
        .conn()
        .query_row(
            "SELECT skill_id, ratified FROM assessment_items WHERE id = 'q_single'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("row");
    assert_eq!(skill, "skill_rust");
    assert_eq!(ratified, 1);

    let draft_ratified: i64 = db
        .conn()
        .query_row(
            "SELECT ratified FROM assessment_items WHERE id = 'q_draft'",
            [],
            |r| r.get(0),
        )
        .expect("row");
    assert_eq!(
        draft_ratified, 0,
        "unratified bank must not yield a ratified item"
    );
}

#[test]
fn backfill_populates_the_multi_skill_table() {
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    seed_bank(&db);
    rerun_migration_072(&db);

    let (skill, weight): (String, f64) = db
        .conn()
        .query_row(
            "SELECT skill_id, weight FROM assessment_item_skills WHERE item_id = 'q_single'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("row");
    assert_eq!(skill, "skill_rust");
    assert_eq!(weight, 1.0);
}

#[test]
fn backfill_is_idempotent() {
    // Migrations can be re-applied across profile restores; a second run
    // must not duplicate items or resurrect deleted ones.
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    seed_bank(&db);
    rerun_migration_072(&db);
    rerun_migration_072(&db);
    rerun_migration_072(&db);

    let count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM assessment_items", [], |r| r.get(0))
        .expect("count");
    assert_eq!(count, 2);
}

#[test]
fn item_kind_is_constrained() {
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    let err = db.conn().execute(
        "INSERT INTO assessment_items (id, item_kind, skill_id, content_public)
         VALUES ('bad', 'essay-ish', 'skill_x', '{}')",
        [],
    );
    assert!(err.is_err(), "CHECK constraint should reject unknown kinds");
}

// ---- migration 073: bloom levels -----------------------------------------

fn rerun_migration(db: &Database, version: i64) {
    let (_, _, sql) = MIGRATIONS
        .iter()
        .find(|(v, _, _)| *v == version)
        .unwrap_or_else(|| panic!("migration {version} exists"));
    db.conn()
        .execute_batch(sql)
        .unwrap_or_else(|e| panic!("migration {version} re-runs cleanly: {e}"));
}

/// `skills.subject_id` is a real foreign key, so a skill fixture needs a
/// subject and a subject field behind it.
fn seed_subject(db: &Database) {
    db.conn()
        .execute_batch(
            "INSERT OR IGNORE INTO subject_fields (id, name) VALUES ('sf', 'Field');
             INSERT OR IGNORE INTO subjects (id, name, subject_field_id)
             VALUES ('sub', 'Subject', 'sf');",
        )
        .expect("seed subject");
}

fn skill_level(db: &Database, id: &str) -> String {
    db.conn()
        .query_row("SELECT bloom_level FROM skills WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .expect("skill row")
}

#[test]
fn normalises_unknown_bloom_levels_to_the_column_default() {
    // A row the type would read as `apply` must not keep claiming to be
    // something else — the row and the type have to agree.
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    seed_subject(&db);
    db.conn()
        .execute_batch(
            "INSERT INTO skills (id, name, subject_id, bloom_level)
             VALUES ('s_bad', 'Bad', 'sub', 'synthesize'),
                    ('s_empty', 'Empty', 'sub', '');",
        )
        .expect("seed");
    rerun_migration(&db, 73);

    assert_eq!(skill_level(&db, "s_bad"), "apply");
    assert_eq!(skill_level(&db, "s_empty"), "apply");
}

#[test]
fn folds_case_and_whitespace_onto_the_canonical_token() {
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    seed_subject(&db);
    db.conn()
        .execute_batch(
            "INSERT INTO skills (id, name, subject_id, bloom_level)
             VALUES ('s_case', 'Case', 'sub', '  Analyze ');",
        )
        .expect("seed");
    rerun_migration(&db, 73);

    assert_eq!(skill_level(&db, "s_case"), "analyze");
}

#[test]
fn leaves_valid_bloom_levels_untouched() {
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    seed_subject(&db);
    for level in [
        "remember",
        "understand",
        "apply",
        "analyze",
        "evaluate",
        "create",
    ] {
        db.conn()
            .execute(
                "INSERT INTO skills (id, name, subject_id, bloom_level) VALUES (?1, 'S', 'sub', ?2)",
                rusqlite::params![format!("s_{level}"), level],
            )
            .expect("seed");
    }
    rerun_migration(&db, 73);

    for level in [
        "remember",
        "understand",
        "apply",
        "analyze",
        "evaluate",
        "create",
    ] {
        assert_eq!(skill_level(&db, &format!("s_{level}")), level);
    }
}

#[test]
fn backfills_item_bloom_level_from_the_parent_skill() {
    // Migration 072 left the column NULL; an item should start at its
    // skill's level rather than at a guess.
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    seed_subject(&db);
    db.conn()
        .execute_batch(
            "INSERT INTO skills (id, name, subject_id, bloom_level)
             VALUES ('s_eval', 'Eval', 'sub', 'evaluate');
             INSERT INTO assessment_items (id, item_kind, skill_id, content_public, bloom_level)
             VALUES ('i_1', 'mcq', 's_eval', '{}', NULL);",
        )
        .expect("seed");
    rerun_migration(&db, 73);

    let level: String = db
        .conn()
        .query_row(
            "SELECT bloom_level FROM assessment_items WHERE id = 'i_1'",
            [],
            |r| r.get(0),
        )
        .expect("item row");
    assert_eq!(level, "evaluate");
}

#[test]
fn item_backfill_does_not_overwrite_an_authored_level() {
    // The column exists on the item precisely so it can differ from the
    // skill's; a re-run must not flatten that back.
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    seed_subject(&db);
    db.conn()
        .execute_batch(
            "INSERT INTO skills (id, name, subject_id, bloom_level)
             VALUES ('s_x', 'X', 'sub', 'remember');
             INSERT INTO assessment_items (id, item_kind, skill_id, content_public, bloom_level)
             VALUES ('i_authored', 'mcq', 's_x', '{}', 'create');",
        )
        .expect("seed");
    rerun_migration(&db, 73);
    rerun_migration(&db, 73);

    let level: String = db
        .conn()
        .query_row(
            "SELECT bloom_level FROM assessment_items WHERE id = 'i_authored'",
            [],
            |r| r.get(0),
        )
        .expect("item row");
    assert_eq!(level, "create");
}

#[test]
fn item_with_a_missing_skill_still_gets_a_level() {
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");
    db.conn()
        .execute_batch(
            "INSERT INTO assessment_items (id, item_kind, skill_id, content_public, bloom_level)
             VALUES ('i_orphan', 'mcq', 'no_such_skill', '{}', NULL);",
        )
        .expect("seed");
    rerun_migration(&db, 73);

    let level: String = db
        .conn()
        .query_row(
            "SELECT bloom_level FROM assessment_items WHERE id = 'i_orphan'",
            [],
            |r| r.get(0),
        )
        .expect("item row");
    assert_eq!(level, "apply");
}

#[test]
fn seeded_taxonomy_carries_only_known_bloom_levels() {
    // The shipped seed is the largest real dataset here; if it contained a
    // stray level the normalisation above would silently rewrite content.
    let db = Database::open_in_memory().expect("db");
    db.run_migrations().expect("migrations");

    let bad: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM skills WHERE bloom_level NOT IN
             ('remember','understand','apply','analyze','evaluate','create')",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(bad, 0, "seeded skills must use canonical Bloom tokens");
}
