//! Seed a single demo course that exercises the first-party plugins
//! (Music Reviews, IRL Review, and the graded code editors). Runs
//! after `builtins::install_all` and is idempotent against re-invocation
//! (uses `INSERT OR IGNORE` against deterministic ids). The global Music/IRL
//! plugins are installed at startup; the graded code editors are course-scoped
//! and install lazily on enrollment, so the editor elements reference them by
//! their bundle CID — enrolling in this course then surfaces the plugin
//! install/consent pre-flight for them.

use rusqlite::{params, Connection};

/// Demo course id; deterministic so re-seeding is a no-op.
const COURSE_ID: &str = "course_plugin_demo";
const CHAPTER_MUSIC: &str = "ch_plugin_demo_music";
const CHAPTER_IRL: &str = "ch_plugin_demo_irl";
const CHAPTER_EDITOR: &str = "ch_plugin_demo_editor";
const EL_MUSIC: &str = "el_plugin_demo_music_reviews";
const EL_IRL: &str = "el_plugin_demo_irl_review";
const EL_EDITOR_JS: &str = "el_plugin_demo_editor_js_double";
const EL_EDITOR_TS: &str = "el_plugin_demo_editor_ts_sum";
const EL_EDITOR_CPP: &str = "el_plugin_demo_editor_cpp_double";
const EL_EDITOR_PYTHON: &str = "el_plugin_demo_editor_python_double";

/// Resolve a builtin plugin's CID by its manifest `id` slug. The full id is
/// `did:key:<author>#<slug>`, so we match on the parsed `id` field ending in
/// `#<slug>` — NOT a substring of the whole manifest. A substring match would
/// also hit a plugin that merely *lists* this slug in its `dependencies`
/// array (e.g. the `editors` collection lists the language editors), which
/// would otherwise cause editor elements to be pointed at the collection.
fn find_plugin_cid(conn: &Connection, slug: &str) -> Option<String> {
    let suffix = format!("#{slug}");
    let mut stmt = conn
        .prepare("SELECT plugin_cid, manifest_json FROM plugin_installed WHERE source = 'builtin'")
        .ok()?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .ok()?;
    for (cid, manifest_json) in rows.flatten() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&manifest_json) else {
            continue;
        };
        if value
            .get("id")
            .and_then(|v| v.as_str())
            .is_some_and(|id| id.ends_with(&suffix))
        {
            return Some(cid);
        }
    }
    None
}

/// CID of a builtin plugin computed directly from its embedded bundle —
/// resolvable even before the plugin is installed. The graded code editors are
/// course-scoped and install lazily on enrollment, so the demo course must
/// reference them by their bundle CID (not their installed row, which doesn't
/// exist yet). Enrolling then surfaces the install/consent pre-flight for them.
fn builtin_cid(slug: &str) -> Option<String> {
    crate::plugins::builtins::BUILTIN_PLUGINS
        .iter()
        .find(|b| b.slug == slug)
        .map(|b| crate::plugins::verifier::compute_plugin_cid(b.manifest_json))
}

/// Seed (and refresh) one graded code-editor element. The editor plugins carry
/// their problem inline: `content` holds the prompt,
/// starter code, visible `tests`, and a `grader_private.tests` array of hidden
/// cases. The host strips `grader_private` before the iframe sees the content,
/// so hidden expectations never reach the learner, but the deterministic grader
/// (run on submit) scores against every case.
#[allow(clippy::too_many_arguments)]
fn seed_editor_element(
    conn: &Connection,
    id: &str,
    chapter_id: &str,
    title: &str,
    position: i64,
    plugin_cid: &str,
    content: &serde_json::Value,
    skill_id: &str,
) -> Result<(), rusqlite::Error> {
    let content = content.to_string();
    conn.execute(
        "INSERT OR IGNORE INTO course_elements \
         (id, chapter_id, title, element_type, position, duration_seconds, plugin_cid, plugin_version, content_inline) \
         VALUES (?1, ?2, ?3, 'plugin', ?4, NULL, ?5, '0.1.0', ?6)",
        params![id, chapter_id, title, position, plugin_cid, content],
    )?;
    conn.execute(
        "UPDATE course_elements SET content_inline = ?1, plugin_cid = ?2 WHERE id = ?3",
        params![content, plugin_cid, id],
    )?;
    // Skill tag drives the graded-submission credential path: on a passing
    // grade, plugin_submit_and_grade issues an AssessmentCredential per tag.
    // Conditional on the skill existing — the taxonomy is `dev-seed`-only, but
    // this demo seed runs unconditionally, so skip the tag (and thus the
    // credential) rather than trip the skills FK when the taxonomy is absent.
    conn.execute(
        "INSERT OR IGNORE INTO element_skill_tags (element_id, skill_id, weight) \
         SELECT ?1, ?2, 1.0 WHERE EXISTS (SELECT 1 FROM skills WHERE id = ?2)",
        params![id, skill_id],
    )?;
    Ok(())
}

/// Insert (or refresh) the demo plugin course. Skips silently if either
/// builtin plugin is not yet installed — the next call after a successful
/// install will fill it in.
pub fn seed_plugin_demo_course(conn: &Connection) -> Result<(), rusqlite::Error> {
    let Some(music_cid) = find_plugin_cid(conn, "music-reviews") else {
        log::debug!("plugin demo seed: music-reviews not installed yet — skipping");
        return Ok(());
    };
    let Some(irl_cid) = find_plugin_cid(conn, "irl-review") else {
        log::debug!("plugin demo seed: irl-review not installed yet — skipping");
        return Ok(());
    };

    // Course shell.
    conn.execute(
        "INSERT OR IGNORE INTO courses (id, title, description, author_address, tags, skill_ids, status, published_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'published', datetime('now'))",
        params![
            COURSE_ID,
            "Plugins Showcase",
            "A short course that demonstrates the first-party plugins: Music Reviews (live pitch-matched note review), IRL Review (human-instructor review of uploaded work), and the graded code editors (write JavaScript, TypeScript, C++, or Python with live evaluation and submit for a deterministic score).",
            "addr_demo_learner",
            "[\"demo\",\"plugins\"]",
            "[]",
        ],
    )?;

    // Chapters.
    conn.execute(
        "INSERT OR IGNORE INTO course_chapters (id, course_id, title, description, position) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            CHAPTER_MUSIC,
            COURSE_ID,
            "Music Reviews",
            "Play a target sequence of notes; live pitch detection marks each note correct or wrong.",
            0,
        ],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO course_chapters (id, course_id, title, description, position) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            CHAPTER_IRL,
            COURSE_ID,
            "IRL Review",
            "Upload work in any format and request a review from a human instructor on this device.",
            1,
        ],
    )?;

    // Element 1 — Music Reviews. Content payload picks a beginner C-major
    // scale at a calm tempo and loose pitch tolerance so the demo is
    // forgiving for a first-time learner.
    let music_content = serde_json::json!({
        "title": "C major — ascend and return",
        "notes": [
            { "name": "C4", "duration": 1 },
            { "name": "D4", "duration": 1 },
            { "name": "E4", "duration": 1 },
            { "name": "F4", "duration": 1 },
            { "name": "G4", "duration": 2 },
            { "name": "A4", "duration": 1 },
            { "name": "G4", "duration": 2 },
            { "name": "F4", "duration": 1 },
            { "name": "E4", "duration": 2 },
            { "name": "D4", "duration": 1 },
            { "name": "C4", "duration": 4 }
        ],
        "tolerance_cents": 45,
        "bpm": 60
    })
    .to_string();
    conn.execute(
        "INSERT OR IGNORE INTO course_elements \
         (id, chapter_id, title, element_type, position, duration_seconds, plugin_cid, plugin_version, content_inline) \
         VALUES (?1, ?2, ?3, 'plugin', 0, NULL, ?4, '0.1.0', ?5)",
        params![EL_MUSIC, CHAPTER_MUSIC, "Play the C-major scale", music_cid, music_content],
    )?;
    // Refresh content_inline every startup so changes to the embedded
    // demo payload propagate without requiring a full reset of the DB.
    conn.execute(
        "UPDATE course_elements SET content_inline = ?1, plugin_cid = ?2 WHERE id = ?3",
        params![music_content, music_cid, EL_MUSIC],
    )?;

    // Element 2 — IRL Review.
    let irl_content = serde_json::json!({
        "prompt": "Record yourself playing one full chorus of any song, or upload sheet music with annotated phrasing. The instructor will rate your timing, intonation, and expression.",
        "default_skills": ["timing", "intonation", "expression"],
        "accept": "audio/*,video/*,image/*,application/pdf"
    })
    .to_string();
    conn.execute(
        "INSERT OR IGNORE INTO course_elements \
         (id, chapter_id, title, element_type, position, duration_seconds, plugin_cid, plugin_version, content_inline) \
         VALUES (?1, ?2, ?3, 'plugin', 0, NULL, ?4, '0.1.0', ?5)",
        params![EL_IRL, CHAPTER_IRL, "Submit a performance for review", irl_cid, irl_content],
    )?;
    conn.execute(
        "UPDATE course_elements SET content_inline = ?1, plugin_cid = ?2 WHERE id = ?3",
        params![irl_content, irl_cid, EL_IRL],
    )?;

    // Chapter 3 — Graded code editors. The editor plugins are `kind: graded`:
    // the learner writes code with live eval + visible tests, then submits for a
    // credential-bearing score computed by the host's deterministic grader.
    // The editors are course-scoped (install lazily on enrollment), so reference
    // them by their bundle CID rather than an installed row — this is what makes
    // enrolling in this course surface the plugin install/consent pre-flight.
    match (
        builtin_cid("editor-javascript"),
        builtin_cid("editor-typescript"),
    ) {
        (Some(ejs_cid), Some(ets_cid)) => {
            conn.execute(
                "INSERT OR IGNORE INTO course_chapters (id, course_id, title, description, position) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    CHAPTER_EDITOR,
                    COURSE_ID,
                    "Graded code editors",
                    "Write JavaScript, TypeScript, C++, or Python with syntax highlighting and live evaluation, run the visible tests, then submit for a graded score. Hidden tests are checked by the host's deterministic grader.",
                    2,
                ],
            )?;

            let js_content = serde_json::json!({
                "title": "Double the number",
                "prompt": "Read an integer **n** from input (one line) and print **n × 2**.",
                "starter_code": "const n = Number(readLine());\n// print n doubled\n",
                "tests": [
                    { "name": "example", "stdin": "4", "expected_stdout": "8" }
                ],
                "grader_private": {
                    "tests": [
                        { "name": "zero", "stdin": "0", "expected_stdout": "0" },
                        { "name": "negative", "stdin": "-5", "expected_stdout": "-10" },
                        { "name": "large", "stdin": "1000000", "expected_stdout": "2000000" }
                    ]
                }
            });
            seed_editor_element(
                conn,
                EL_EDITOR_JS,
                CHAPTER_EDITOR,
                "Double the number (JavaScript)",
                0,
                &ejs_cid,
                &js_content,
                "skill_javascript",
            )?;

            let ts_content = serde_json::json!({
                "title": "Sum to n",
                "prompt": "Read an integer **n** and print the sum **1 + 2 + … + n**. Type annotations are allowed — they're stripped before running.",
                "starter_code": "const n: number = Number(readLine());\n// print the sum 1..n\n",
                "tests": [
                    { "name": "example", "stdin": "10", "expected_stdout": "55" }
                ],
                "grader_private": {
                    "tests": [
                        { "name": "one", "stdin": "1", "expected_stdout": "1" },
                        { "name": "hundred", "stdin": "100", "expected_stdout": "5050" }
                    ]
                }
            });
            seed_editor_element(
                conn,
                EL_EDITOR_TS,
                CHAPTER_EDITOR,
                "Sum to n (TypeScript)",
                1,
                &ets_cid,
                &ts_content,
                "skill_typescript",
            )?;

            // C++ (JSCPP) — added only when the C++ editor is installed.
            if let Some(ecpp_cid) = builtin_cid("editor-cpp") {
                let cpp_content = serde_json::json!({
                    "title": "Double the number",
                    "prompt": "Read an integer **n** from input and print **n × 2**.",
                    "starter_code": "#include <iostream>\nusing namespace std;\n\nint main() {\n    int n;\n    cin >> n;\n    // print n doubled\n    return 0;\n}\n",
                    "tests": [
                        { "name": "example", "stdin": "4", "expected_stdout": "8" }
                    ],
                    "grader_private": {
                        "tests": [
                            { "name": "zero", "stdin": "0", "expected_stdout": "0" },
                            { "name": "large", "stdin": "1000", "expected_stdout": "2000" }
                        ]
                    }
                });
                seed_editor_element(
                    conn,
                    EL_EDITOR_CPP,
                    CHAPTER_EDITOR,
                    "Double the number (C++)",
                    2,
                    &ecpp_cid,
                    &cpp_content,
                    "skill_cpp",
                )?;
            }

            // Python (RustPython) — added only when the Python editor is installed.
            if let Some(epy_cid) = builtin_cid("editor-python") {
                let py_content = serde_json::json!({
                    "title": "Double the number",
                    "prompt": "Read an integer **n** from input and print **n × 2**.",
                    "starter_code": "n = int(input())\n# print n doubled\n",
                    "tests": [
                        { "name": "example", "stdin": "4", "expected_stdout": "8" }
                    ],
                    "grader_private": {
                        "tests": [
                            { "name": "zero", "stdin": "0", "expected_stdout": "0" },
                            { "name": "large", "stdin": "1000000", "expected_stdout": "2000000" }
                        ]
                    }
                });
                seed_editor_element(
                    conn,
                    EL_EDITOR_PYTHON,
                    CHAPTER_EDITOR,
                    "Double the number (Python)",
                    3,
                    &epy_cid,
                    &py_content,
                    "skill_python",
                )?;
            }
            log::info!("plugin demo course: graded editor chapter seeded (js + ts + cpp + python)");
        }
        _ => {
            log::debug!("plugin demo seed: editor plugins not installed — skipping editor chapter");
        }
    }

    // Intentionally NOT auto-enrolled: a fresh user discovers this course in
    // the catalog and enrolls themselves, which triggers the plugin
    // install/consent pre-flight (course_required_plugins + install_course_plugins).

    log::info!(
        "plugin demo course seeded (music_cid={} irl_cid={})",
        &music_cid[..music_cid.len().min(12)],
        &irl_cid[..irl_cid.len().min(12)]
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::plugins::registry;
    use tempfile::TempDir;

    /// Install every embedded builtin, including the course-scoped code editors
    /// and the `editors` collection, regardless of the `dev-seed` feature.
    /// `install_all` installs only global builtins at startup; the seed demo's
    /// editor chapter needs the editors present, so install the full set
    /// directly (in `BUILTIN_PLUGINS` order, so the collection's deps precede it).
    fn install_all_builtins(db: &Database, dir: &std::path::Path) {
        for bundle in crate::plugins::builtins::BUILTIN_PLUGINS {
            registry::install_builtin(db, dir, bundle).expect("install builtin");
        }
    }

    #[test]
    fn seeds_graded_editor_chapter() {
        let db = Database::open_in_memory().expect("db");
        db.run_migrations().expect("migrations");
        let dir = TempDir::new().unwrap();
        install_all_builtins(&db, dir.path());

        seed_plugin_demo_course(db.conn()).expect("seed");

        // Four graded editor elements land in the editor chapter (JS, TS, C++, Python).
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM course_elements \
                 WHERE chapter_id = ?1 AND element_type = 'plugin'",
                params![CHAPTER_EDITOR],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 4);

        // Each element pins its own editor plugin (not the `editors`
        // collection), and carries inline visible tests plus hidden
        // `grader_private` tests.
        let ejs_cid = find_plugin_cid(db.conn(), "editor-javascript").unwrap();
        let ets_cid = find_plugin_cid(db.conn(), "editor-typescript").unwrap();
        assert_ne!(ejs_cid, ets_cid);

        let (js_cid, js_content): (String, String) = db
            .conn()
            .query_row(
                "SELECT plugin_cid, content_inline FROM course_elements WHERE id = ?1",
                params![EL_EDITOR_JS],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(js_cid, ejs_cid);
        assert!(js_content.contains("\"grader_private\""));
        assert!(js_content.contains("\"tests\""));

        let ts_cid: String = db
            .conn()
            .query_row(
                "SELECT plugin_cid FROM course_elements WHERE id = ?1",
                params![EL_EDITOR_TS],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ts_cid, ets_cid);

        let cpp_cid: String = db
            .conn()
            .query_row(
                "SELECT plugin_cid FROM course_elements WHERE id = ?1",
                params![EL_EDITOR_CPP],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cpp_cid, find_plugin_cid(db.conn(), "editor-cpp").unwrap());

        let py_cid: String = db
            .conn()
            .query_row(
                "SELECT plugin_cid FROM course_elements WHERE id = ?1",
                params![EL_EDITOR_PYTHON],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(py_cid, find_plugin_cid(db.conn(), "editor-python").unwrap());

        // Idempotent.
        seed_plugin_demo_course(db.conn()).expect("reseed");
        let count2: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM course_elements WHERE chapter_id = ?1",
                params![CHAPTER_EDITOR],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count2, 4);
    }
}
