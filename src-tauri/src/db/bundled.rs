//! Bundled built-in data installed into every profile database.
//!
//! The skill taxonomy, skill synonyms, goal templates and question banks ship
//! with the app as labelled built-in content. No committee ratified them:
//! `taxonomy_version = 'bundled'` identifies the goal templates and question
//! banks, and `ratified = 1` only marks those rows usable until the baseline
//! schema replaces that column. No personas, credentials, opinions, courses,
//! classrooms or governance rows are installed; the demo course corpus lives
//! in `demo-world/content/` as non-authoritative source material.
//!
//! Installation is one transaction and idempotent: taxonomy rows are written
//! only into an empty taxonomy, and the rest use `INSERT OR IGNORE` and keyed
//! updates.

use rusqlite::{params, Connection};
use serde::Deserialize;

const PUBLIC_TAXONOMY_JSON: &str = include_str!("../../../bootstrap/public_taxonomy.json");

#[derive(Debug, Deserialize)]
struct BootstrapTaxonomyPayload {
    subject_fields: Vec<BootstrapSubjectField>,
    subjects: Vec<BootstrapSubject>,
    skills: Vec<BootstrapSkill>,
    skill_prerequisites: Vec<BootstrapSkillPrerequisite>,
    skill_relations: Vec<BootstrapSkillRelation>,
}

#[derive(Debug, Deserialize)]
struct BootstrapSubjectField {
    id: String,
    name: String,
    description: Option<String>,
    icon_emoji: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BootstrapSubject {
    id: String,
    name: String,
    description: Option<String>,
    subject_field_id: String,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BootstrapSkill {
    id: String,
    name: String,
    description: Option<String>,
    subject_id: String,
    bloom_level: String,
    created_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BootstrapSkillPrerequisite {
    skill_id: String,
    prerequisite_id: String,
}

#[derive(Debug, Deserialize)]
struct BootstrapSkillRelation {
    skill_id: String,
    related_skill_id: String,
    relation_type: String,
}

/// Install the bundled taxonomy, synonyms, goal templates and question banks.
/// Returns the number of taxonomy skills written (0 when a taxonomy was
/// already present).
pub fn install_bundled_data(conn: &Connection) -> Result<i64, String> {
    crate::db::with_transaction(conn, || {
        let skills = install_taxonomy(conn)?;
        conn.execute_batch(GOAL_TEMPLATES_SQL)
            .map_err(|e| format!("install bundled goal templates: {e}"))?;
        conn.execute_batch(QUESTION_BANKS_SQL)
            .map_err(|e| format!("install bundled question banks: {e}"))?;
        Ok(skills)
    })
}

fn install_taxonomy(conn: &Connection) -> Result<i64, String> {
    let existing_skills: i64 = conn
        .query_row("SELECT COUNT(*) FROM skills", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    if existing_skills > 0 {
        return Ok(0);
    }

    let payload: BootstrapTaxonomyPayload =
        serde_json::from_str(PUBLIC_TAXONOMY_JSON).map_err(|e| e.to_string())?;

    for f in &payload.subject_fields {
        conn.execute(
            "INSERT OR REPLACE INTO subject_fields
             (id, name, description, icon_emoji, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, COALESCE(?5, datetime('now')), COALESCE(?6, datetime('now')))",
            params![
                f.id,
                f.name,
                f.description,
                f.icon_emoji,
                f.created_at,
                f.updated_at
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for s in &payload.subjects {
        conn.execute(
            "INSERT OR REPLACE INTO subjects
             (id, name, description, subject_field_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, COALESCE(?5, datetime('now')), COALESCE(?6, datetime('now')))",
            params![
                s.id,
                s.name,
                s.description,
                s.subject_field_id,
                s.created_at,
                s.updated_at
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for sk in &payload.skills {
        conn.execute(
            "INSERT OR REPLACE INTO skills
             (id, name, description, subject_id, bloom_level, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, COALESCE(?6, datetime('now')), COALESCE(?7, datetime('now')))",
            params![
                sk.id,
                sk.name,
                sk.description,
                sk.subject_id,
                sk.bloom_level,
                sk.created_at,
                sk.updated_at
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for edge in &payload.skill_prerequisites {
        conn.execute(
            "INSERT OR IGNORE INTO skill_prerequisites (skill_id, prerequisite_id)
             VALUES (?1, ?2)",
            params![edge.skill_id, edge.prerequisite_id],
        )
        .map_err(|e| e.to_string())?;
    }

    for rel in &payload.skill_relations {
        conn.execute(
            "INSERT OR IGNORE INTO skill_relations (skill_id, related_skill_id, relation_type)
             VALUES (?1, ?2, ?3)",
            params![rel.skill_id, rel.related_skill_id, rel.relation_type],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(payload.skills.len() as i64)
}

/// Bundled question banks so assessments for these skills work offline.
///
/// Questions are written as `assessment_items`, which is what an attempt
/// actually draws from (`commands::assessment::start_attempt` selects
/// `WHERE bank_id = ?`). Seeding `bank_questions` instead leaves a bank whose
/// item pool is empty, and an empty pool produces an attempt with no
/// questions rather than an error, so the split matters: `content_public` is
/// the half that may reach a client and `grader_private` holds the key.
///
/// `correct_indices` are original-option indices; the runtime shuffles per
/// attempt and grades host-side.
const QUESTION_BANKS_SQL: &str = r#"
INSERT OR IGNORE INTO question_banks (id, skill_id, label, pass_threshold, draw_count, taxonomy_version, ratified) VALUES
  ('qb_js', 'skill_javascript', 'JavaScript fundamentals', 0.7, 3, 'bundled', 1),
  ('qb_bigo', 'skill_big_o', 'Big-O & complexity', 0.7, 3, 'bundled', 1);

WITH bundled(id, bank_id, prompt, options, correct_indices, difficulty, points) AS (VALUES
  ('bq_js1', 'qb_js', 'Which keyword declares a block-scoped variable?',
    '["var","let","function","global"]', '[1]', 1, 1.0),
  ('bq_js2', 'qb_js', 'What does `typeof null` return?',
    '["\"null\"","\"object\"","\"undefined\"","throws"]', '[1]', 2, 1.0),
  ('bq_js3', 'qb_js', 'Which are falsy in JavaScript? (select all)',
    '["0","\"\"","[]","NaN"]', '[0,1,3]', 3, 1.0),
  ('bq_js4', 'qb_js', 'Which method returns a new array?',
    '["push","map","sort","splice"]', '[1]', 2, 1.0),
  ('bq_bo1', 'qb_bigo', 'Time complexity of binary search?',
    '["O(1)","O(log n)","O(n)","O(n log n)"]', '[1]', 1, 1.0),
  ('bq_bo2', 'qb_bigo', 'Which is asymptotically fastest for large n?',
    '["O(n^2)","O(n log n)","O(n)","O(2^n)"]', '[2]', 2, 1.0),
  ('bq_bo3', 'qb_bigo', 'Average-case lookup in a hash table?',
    '["O(1)","O(log n)","O(n)","O(n log n)"]', '[0]', 2, 1.0),
  ('bq_bo4', 'qb_bigo', 'Which sorts are O(n log n) worst-case? (select all)',
    '["quicksort","mergesort","heapsort","bubblesort"]', '[1,2]', 3, 1.0)
)
INSERT OR IGNORE INTO assessment_items
  (id, item_kind, skill_id, content_public, grader_private,
   difficulty, points, bank_id, taxonomy_version, ratified)
SELECT
  q.id,
  'mcq',
  b.skill_id,
  json_object(
    'kind',    CASE WHEN json_array_length(q.correct_indices) = 1
                    THEN 'single' ELSE 'multi' END,
    'prompt',  q.prompt,
    'options', json(q.options)
  ),
  json_object('correct_indices', json(q.correct_indices)),
  q.difficulty,
  q.points,
  q.bank_id,
  'bundled',
  1
FROM bundled q
JOIN question_banks b ON b.id = q.bank_id;

-- The primary skill also lands in the multi-skill table, so one query shape
-- serves both the single- and multi-skill cases.
INSERT OR IGNORE INTO assessment_item_skills (item_id, skill_id, weight)
SELECT id, skill_id, 1.0 FROM assessment_items WHERE taxonomy_version = 'bundled';
"#;

/// Skill synonyms for on-device JD and document matching, and bundled goal
/// templates mapping exams, curricula and job roles onto bundled skills.
const GOAL_TEMPLATES_SQL: &str = r#"
-- Synonyms/aliases for on-device JD + document skill matching.
UPDATE skills SET synonyms = 'js,ecmascript,node.js,node' WHERE id = 'skill_javascript';
UPDATE skills SET synonyms = 'ts' WHERE id = 'skill_typescript';
UPDATE skills SET synonyms = 'vue.js,vuejs' WHERE id = 'skill_vue';
UPDATE skills SET synonyms = 'html,css,frontend' WHERE id = 'skill_html_css';
UPDATE skills SET synonyms = 'rest,restful,api,http api' WHERE id = 'skill_rest_api';
UPDATE skills SET synonyms = 'database design,data modeling,sql' WHERE id = 'skill_db_design';
UPDATE skills SET synonyms = 'algorithms,complexity analysis,data structures' WHERE id = 'skill_big_o';
UPDATE skills SET synonyms = 'linear regression,statistical modeling' WHERE id = 'skill_regression';
UPDATE skills SET synonyms = 'neural networks,deep learning,dnn' WHERE id = 'skill_neural_nets';
UPDATE skills SET synonyms = 'supervised learning' WHERE id = 'skill_supervised';
UPDATE skills SET synonyms = 'model evaluation,machine learning' WHERE id = 'skill_ml_eval';
UPDATE skills SET synonyms = 'authentication,oauth,authz' WHERE id = 'skill_auth';

-- Bundled goal templates, labelled taxonomy_version 'bundled'.
INSERT OR IGNORE INTO goal_templates (id, kind, key, label, board, grade, skill_ids, taxonomy_version, ratified) VALUES
  ('gt_role_em', 'job_role', 'engineering_manager', 'Engineering Manager', NULL, NULL,
   '["skill_big_o","skill_db_design","skill_rest_api","skill_graph_theory"]', 'bundled', 1),
  ('gt_role_fe', 'job_role', 'frontend_engineer', 'Frontend Engineer', NULL, NULL,
   '["skill_javascript","skill_typescript","skill_html_css","skill_vue"]', 'bundled', 1),
  ('gt_role_mle', 'job_role', 'ml_engineer', 'Machine Learning Engineer', NULL, NULL,
   '["skill_regression","skill_neural_nets","skill_supervised","skill_ml_eval","skill_probability"]', 'bundled', 1),
  ('gt_exam_jee', 'exam', 'jee_main', 'JEE Main (Engineering entrance)', NULL, NULL,
   '["skill_logic","skill_sets","skill_combinatorics","skill_probability"]', 'bundled', 1),
  ('gt_cur_cbse10', 'curriculum', 'cbse.grade10', 'CBSE — Grade 10', 'CBSE', '10',
   '["skill_logic","skill_sets","skill_probability"]', 'bundled', 1),
  ('gt_cur_icse10', 'curriculum', 'icse.grade10', 'ICSE — Grade 10', 'ICSE', '10',
   '["skill_logic","skill_sets","skill_probability"]', 'bundled', 1),

  -- Additional job roles
  ('gt_role_be', 'job_role', 'backend_engineer', 'Backend Engineer', NULL, NULL,
   '["skill_rest_api","skill_db_design","skill_sql","skill_docker","skill_ci_cd"]', 'bundled', 1),
  ('gt_role_fs', 'job_role', 'fullstack_engineer', 'Full-Stack Engineer', NULL, NULL,
   '["skill_javascript","skill_react","skill_rest_api","skill_db_design"]', 'bundled', 1),
  ('gt_role_ds', 'job_role', 'data_scientist', 'Data Scientist', NULL, NULL,
   '["skill_python","skill_regression","skill_probability","skill_distributions","skill_inference"]', 'bundled', 1),
  ('gt_role_de', 'job_role', 'data_engineer', 'Data Engineer', NULL, NULL,
   '["skill_etl","skill_sql","skill_streaming","skill_db_design"]', 'bundled', 1),
  ('gt_role_devops', 'job_role', 'devops_engineer', 'DevOps Engineer', NULL, NULL,
   '["skill_docker","skill_ci_cd","skill_dns","skill_concurrency"]', 'bundled', 1),
  ('gt_role_sec', 'job_role', 'security_engineer', 'Security Engineer', NULL, NULL,
   '["skill_symmetric","skill_asymmetric","skill_tls","skill_firewalls","skill_auth"]', 'bundled', 1),
  ('gt_role_pd', 'job_role', 'product_designer', 'Product Designer', NULL, NULL,
   '["skill_ia","skill_design_systems","skill_color_theory","skill_accessibility"]', 'bundled', 1),

  -- Additional exams
  ('gt_exam_gate', 'exam', 'gate_cse', 'GATE — Computer Science', NULL, NULL,
   '["skill_big_o","skill_arrays","skill_graphs","skill_dp","skill_logic"]', 'bundled', 1),
  ('gt_exam_gre', 'exam', 'gre', 'GRE (General)', NULL, NULL,
   '["skill_logic","skill_probability","skill_combinatorics"]', 'bundled', 1),
  ('gt_exam_upsc', 'exam', 'upsc_prelims', 'UPSC Civil Services (Prelims)', NULL, NULL,
   '["skill_constitutional_literacy","skill_federalism_vs_centralism","skill_public_finance_literacy","skill_media_literacy_political"]', 'bundled', 1),

  -- Additional curricula
  ('gt_cur_cbse12', 'curriculum', 'cbse.grade12', 'CBSE — Grade 12', 'CBSE', '12',
   '["skill_derivatives","skill_integrals","skill_probability","skill_logic"]', 'bundled', 1),
  ('gt_cur_icse12', 'curriculum', 'icse.grade12', 'ICSE — Grade 12', 'ICSE', '12',
   '["skill_derivatives","skill_integrals","skill_sets"]', 'bundled', 1),
  ('gt_cur_ib_math', 'curriculum', 'ib.dp.math', 'IB Diploma — Mathematics', 'IB', '12',
   '["skill_derivatives","skill_integrals","skill_matrices","skill_probability"]', 'bundled', 1),
  ('gt_cur_alevel_cs', 'curriculum', 'alevels.cs', 'A-Levels — Computer Science', 'A-Levels', '12',
   '["skill_big_o","skill_arrays","skill_python","skill_logic"]', 'bundled', 1);
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    const INSTALLED_TABLES: [&str; 8] = [
        "subject_fields",
        "subjects",
        "skills",
        "skill_prerequisites",
        "goal_templates",
        "question_banks",
        "assessment_items",
        "assessment_item_skills",
    ];

    fn migrated() -> Database {
        let db = Database::open_in_memory().expect("open database");
        db.run_migrations().expect("run migrations");
        db
    }

    fn count(db: &Database, sql: &str) -> i64 {
        db.conn()
            .query_row(sql, [], |row| row.get(0))
            .expect("count rows")
    }

    fn table_counts(db: &Database) -> Vec<i64> {
        INSTALLED_TABLES
            .iter()
            .map(|table| count(db, &format!("SELECT COUNT(*) FROM {table}")))
            .collect()
    }

    #[test]
    fn fresh_profile_gets_only_labelled_built_in_rows() {
        let db = migrated();
        install_bundled_data(db.conn()).expect("install bundled data");

        let payload: BootstrapTaxonomyPayload =
            serde_json::from_str(PUBLIC_TAXONOMY_JSON).expect("bundled taxonomy");
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM skills"),
            payload.skills.len() as i64
        );
        assert!(count(&db, "SELECT COUNT(*) FROM goal_templates") > 0);
        assert!(count(&db, "SELECT COUNT(*) FROM question_banks") > 0);
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM goal_templates WHERE taxonomy_version != 'bundled'"
            ),
            0
        );
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM question_banks WHERE taxonomy_version != 'bundled'"
            ),
            0
        );
        for table in [
            "local_identity",
            "courses",
            "course_elements",
            "enrollments",
            "credentials",
            "key_registry",
            "opinions",
            "classrooms",
            "classroom_members",
            "tutoring_sessions",
            "reputation_assertions",
            "derived_skill_states",
            "pinboard_observations",
            "completion_observations",
            "devices",
            "sync_log",
            "governance_dao_members",
            "governance_proposals",
        ] {
            assert_eq!(
                count(&db, &format!("SELECT COUNT(*) FROM {table}")),
                0,
                "fabricated rows in {table}"
            );
        }
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM governance_daos WHERE id != 'sentinel-dao'"
            ),
            0
        );
    }

    #[test]
    fn bundled_templates_and_banks_reference_bundled_skills() {
        let db = migrated();
        install_bundled_data(db.conn()).expect("install bundled data");

        let mut stmt = db
            .conn()
            .prepare("SELECT id, skill_ids FROM goal_templates")
            .expect("prepare templates");
        let templates: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query templates")
            .collect::<Result<_, _>>()
            .expect("read templates");
        for (id, skill_ids) in templates {
            let skill_ids: Vec<String> = serde_json::from_str(&skill_ids).expect("skill ids");
            for skill in skill_ids {
                let present: i64 = db
                    .conn()
                    .query_row(
                        "SELECT COUNT(*) FROM skills WHERE id = ?1",
                        params![skill],
                        |row| row.get(0),
                    )
                    .expect("look up skill");
                assert_eq!(present, 1, "{id} references missing skill {skill}");
            }
        }
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM question_banks WHERE skill_id NOT IN (SELECT id FROM skills)"
            ),
            0
        );
    }

    /// An attempt draws from `assessment_items WHERE bank_id = ?`, so a bank
    /// seeded only into `bank_questions` resolves against `skills` and still
    /// yields an empty draw — an attempt with no questions and no error.
    /// Counting bank rows cannot see that; this counts what the draw sees.
    #[test]
    fn every_bundled_bank_can_fill_a_draw() {
        let db = migrated();
        install_bundled_data(db.conn()).expect("install bundled data");

        let mut stmt = db
            .conn()
            .prepare("SELECT id, draw_count FROM question_banks")
            .expect("prepare banks");
        let banks: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query banks")
            .collect::<Result<_, _>>()
            .expect("read banks");
        assert!(!banks.is_empty(), "no bundled banks installed");

        for (bank_id, draw_count) in banks {
            let items: i64 = db
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM assessment_items WHERE bank_id = ?1",
                    params![bank_id],
                    |row| row.get(0),
                )
                .expect("count items");
            assert!(
                items >= draw_count,
                "bank {bank_id} draws {draw_count} but has {items} items"
            );
        }
    }

    #[test]
    fn a_bundled_item_serves_its_prompt_and_withholds_its_key() {
        let db = migrated();
        install_bundled_data(db.conn()).expect("install bundled data");

        let (public, private): (String, String) = db
            .conn()
            .query_row(
                "SELECT content_public, grader_private FROM assessment_items \
                 WHERE id = 'bq_js3'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read bundled item");

        let public: serde_json::Value = serde_json::from_str(&public).expect("public json");
        assert_eq!(public["kind"], "multi");
        assert!(public["prompt"].as_str().expect("prompt").contains("falsy"));
        assert_eq!(public["options"].as_array().expect("options").len(), 4);
        assert!(
            public.get("correct_indices").is_none(),
            "answer key reached the public half: {public}"
        );

        let private: serde_json::Value = serde_json::from_str(&private).expect("private json");
        assert_eq!(private["correct_indices"], serde_json::json!([0, 1, 3]));
    }

    #[test]
    fn reinstalling_changes_nothing() {
        let db = migrated();
        install_bundled_data(db.conn()).expect("install bundled data");
        let before = table_counts(&db);

        assert_eq!(install_bundled_data(db.conn()).expect("reinstall"), 0);

        assert_eq!(table_counts(&db), before);
    }

    #[test]
    fn failed_install_leaves_no_partial_rows_and_can_retry() {
        let db = migrated();
        db.conn()
            .execute_batch(
                "CREATE TEMP TRIGGER fail_bundled_bank BEFORE INSERT ON assessment_items \
                 BEGIN SELECT RAISE(ABORT, 'injected bank failure'); END;",
            )
            .expect("install fault");

        let error = install_bundled_data(db.conn()).unwrap_err();

        assert!(error.contains("injected bank failure"), "{error}");
        assert!(table_counts(&db).iter().all(|rows| *rows == 0));
        db.conn()
            .execute_batch("DROP TRIGGER fail_bundled_bank")
            .expect("remove fault");
        assert!(install_bundled_data(db.conn()).expect("retry install") > 0);
    }
}
