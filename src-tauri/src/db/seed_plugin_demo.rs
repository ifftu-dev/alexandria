//! Seed a single demo course that exercises the first-party plugins
//! (Music Reviews, IRL Review, and the codejudge language plugins). Runs
//! after `builtins::install_all` so the plugin CIDs are resolvable, and is
//! idempotent against re-invocation (uses `INSERT OR IGNORE` against
//! deterministic ids).

use rusqlite::{params, Connection};

/// Demo course id; deterministic so re-seeding is a no-op.
const COURSE_ID: &str = "course_plugin_demo";
const CHAPTER_MUSIC: &str = "ch_plugin_demo_music";
const CHAPTER_IRL: &str = "ch_plugin_demo_irl";
const CHAPTER_CODE: &str = "ch_plugin_demo_code";
const CHAPTER_EDITOR: &str = "ch_plugin_demo_editor";
const EL_MUSIC: &str = "el_plugin_demo_music_reviews";
const EL_IRL: &str = "el_plugin_demo_irl_review";
const EL_CODE_JS_TWOSUM: &str = "el_plugin_demo_code_js_twosum";
const EL_CODE_LUA_REVERSE: &str = "el_plugin_demo_code_lua_reverse";
const EL_CODE_JS_PRIMES: &str = "el_plugin_demo_code_js_primes";
const EL_EDITOR_JS: &str = "el_plugin_demo_editor_js_double";
const EL_EDITOR_TS: &str = "el_plugin_demo_editor_ts_sum";
const EL_EDITOR_CPP: &str = "el_plugin_demo_editor_cpp_double";
const EL_EDITOR_PYTHON: &str = "el_plugin_demo_editor_python_double";
const ENROLLMENT_ID: &str = "enroll_plugin_demo";

/// Resolve a builtin plugin's CID by its manifest `id` slug. The full id is
/// `did:key:<author>#<slug>`, so we match on the parsed `id` field ending in
/// `#<slug>` — NOT a substring of the whole manifest. A substring match would
/// also hit a plugin that merely *lists* this slug in its `dependencies`
/// array (e.g. the `codejudge-multilang` umbrella lists the language plugins),
/// which previously caused code elements to be pointed at the umbrella.
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

/// Seed (and refresh) one codejudge element: a coding problem pinned to a
/// specific language plugin. The content is `{ problem_id }` — the plugin
/// resolves it against its own bundled problem bank at render time.
fn seed_code_element(
    conn: &Connection,
    id: &str,
    chapter_id: &str,
    title: &str,
    position: i64,
    plugin_cid: &str,
    problem_id: &str,
) -> Result<(), rusqlite::Error> {
    let content = serde_json::json!({ "version": "1", "problem_id": problem_id }).to_string();
    conn.execute(
        "INSERT OR IGNORE INTO course_elements \
         (id, chapter_id, title, element_type, position, duration_seconds, plugin_cid, plugin_version, content_inline) \
         VALUES (?1, ?2, ?3, 'plugin', ?4, NULL, ?5, '0.1.0', ?6)",
        params![id, chapter_id, title, position, plugin_cid, content],
    )?;
    // Refresh so edits to the pinned problem id propagate without a DB reset.
    conn.execute(
        "UPDATE course_elements SET content_inline = ?1, plugin_cid = ?2 WHERE id = ?3",
        params![content, plugin_cid, id],
    )?;
    Ok(())
}

/// Seed (and refresh) one graded code-editor element. Unlike codejudge, the
/// editor plugins carry their problem inline: `content` holds the prompt,
/// starter code, visible `tests`, and a `grader_private.tests` array of hidden
/// cases. The host strips `grader_private` before the iframe sees the content,
/// so hidden expectations never reach the learner, but the deterministic grader
/// (run on submit) scores against every case.
fn seed_editor_element(
    conn: &Connection,
    id: &str,
    chapter_id: &str,
    title: &str,
    position: i64,
    plugin_cid: &str,
    content: &serde_json::Value,
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
            "A short course that demonstrates the first-party plugins: Music Reviews (live pitch-matched note review), IRL Review (human-instructor review of uploaded work), codejudge (solve coding challenges in JavaScript and Lua, run locally against test cases), and the graded code editors (write JavaScript, TypeScript, C++, or Python with live evaluation and submit for a deterministic score).",
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

    // Chapter 3 — Code challenges (codejudge language plugins). Additive:
    // only seeded when the codejudge language plugins are installed, so the
    // music/IRL demo still works if they were skipped. Each element pins a
    // shared problem (by id) at a specific language plugin.
    match (
        find_plugin_cid(conn, "codejudge-javascript"),
        find_plugin_cid(conn, "codejudge-lua"),
    ) {
        (Some(js_cid), Some(lua_cid)) => {
            conn.execute(
                "INSERT OR IGNORE INTO course_chapters (id, course_id, title, description, position) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    CHAPTER_CODE,
                    COURSE_ID,
                    "Code challenges",
                    "Solve coding problems in different languages. Each solution runs locally in a sandboxed in-browser engine against hidden test cases.",
                    2,
                ],
            )?;
            seed_code_element(
                conn,
                EL_CODE_JS_TWOSUM,
                CHAPTER_CODE,
                "Two Sum (JavaScript)",
                0,
                &js_cid,
                "two-sum",
            )?;
            seed_code_element(
                conn,
                EL_CODE_LUA_REVERSE,
                CHAPTER_CODE,
                "Reverse a String (Lua)",
                1,
                &lua_cid,
                "reverse-string",
            )?;
            seed_code_element(
                conn,
                EL_CODE_JS_PRIMES,
                CHAPTER_CODE,
                "Count Primes (JavaScript)",
                2,
                &js_cid,
                "count-primes",
            )?;
            log::info!("plugin demo course: codejudge chapter seeded (js + lua)");
        }
        _ => {
            log::debug!("plugin demo seed: codejudge language plugins not installed — skipping code chapter");
        }
    }

    // Chapter 4 — Graded code editors. The editor plugins are `kind: graded`:
    // the learner writes code with live eval + visible tests, then submits for a
    // credential-bearing score computed by the host's deterministic grader.
    // Additive: only seeded when both editor plugins are installed.
    match (
        find_plugin_cid(conn, "editor-javascript"),
        find_plugin_cid(conn, "editor-typescript"),
    ) {
        (Some(ejs_cid), Some(ets_cid)) => {
            conn.execute(
                "INSERT OR IGNORE INTO course_chapters (id, course_id, title, description, position) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    CHAPTER_EDITOR,
                    COURSE_ID,
                    "Graded code editors",
                    "Write JavaScript or TypeScript with syntax highlighting and live evaluation, run the visible tests, then submit for a graded score. Hidden tests are checked by the host's deterministic grader.",
                    3,
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
            )?;

            // C++ (JSCPP) — added only when the C++ editor is installed.
            if let Some(ecpp_cid) = find_plugin_cid(conn, "editor-cpp") {
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
                )?;
            }

            // Python (RustPython) — added only when the Python editor is installed.
            if let Some(epy_cid) = find_plugin_cid(conn, "editor-python") {
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
                )?;
            }
            log::info!("plugin demo course: graded editor chapter seeded (js + ts + cpp + python)");
        }
        _ => {
            log::debug!("plugin demo seed: editor plugins not installed — skipping editor chapter");
        }
    }

    // Auto-enroll the demo learner so the course appears on the dashboard
    // immediately. Single-user app: enrollments are not scoped to a
    // stake address column in the schema.
    conn.execute(
        "INSERT OR IGNORE INTO enrollments (id, course_id, enrolled_at, status) \
         VALUES (?1, ?2, datetime('now'), 'active')",
        params![ENROLLMENT_ID, COURSE_ID],
    )?;

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
    use crate::plugins::builtins;
    use tempfile::TempDir;

    #[test]
    fn seeds_codejudge_code_chapter() {
        let db = Database::open_in_memory().expect("db");
        db.run_migrations().expect("migrations");
        let dir = TempDir::new().unwrap();
        builtins::install_all(&db, dir.path());

        seed_plugin_demo_course(db.conn()).expect("seed");

        // Three codejudge plugin elements land in the code chapter.
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM course_elements \
                 WHERE chapter_id = ?1 AND element_type = 'plugin'",
                params![CHAPTER_CODE],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);

        // The three CIDs must be distinct. The umbrella lists the language
        // plugins in its `dependencies`, so a substring-based resolver would
        // alias them — pointing code elements at the umbrella (no editor).
        // Regression guard for exactly that bug.
        let js_cid = find_plugin_cid(db.conn(), "codejudge-javascript").unwrap();
        let lua_cid = find_plugin_cid(db.conn(), "codejudge-lua").unwrap();
        let umbrella_cid = find_plugin_cid(db.conn(), "codejudge-multilang").unwrap();
        assert_ne!(js_cid, lua_cid);
        assert_ne!(js_cid, umbrella_cid);
        assert_ne!(lua_cid, umbrella_cid);

        // The JS element pins the JS plugin (not the umbrella) + a problem_id.
        let (cid, content): (String, String) = db
            .conn()
            .query_row(
                "SELECT plugin_cid, content_inline FROM course_elements WHERE id = ?1",
                params![EL_CODE_JS_TWOSUM],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(cid, js_cid);
        assert_ne!(cid, umbrella_cid);
        assert!(content.contains("\"problem_id\":\"two-sum\""));

        // The Lua element pins the Lua plugin.
        let lua_el_cid: String = db
            .conn()
            .query_row(
                "SELECT plugin_cid FROM course_elements WHERE id = ?1",
                params![EL_CODE_LUA_REVERSE],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(lua_el_cid, lua_cid);

        // Idempotent: a second seed doesn't duplicate.
        seed_plugin_demo_course(db.conn()).expect("reseed");
        let count2: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM course_elements WHERE chapter_id = ?1",
                params![CHAPTER_CODE],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count2, 3);
    }

    #[test]
    fn seeds_graded_editor_chapter() {
        let db = Database::open_in_memory().expect("db");
        db.run_migrations().expect("migrations");
        let dir = TempDir::new().unwrap();
        builtins::install_all(&db, dir.path());

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

        // Each element pins its own editor plugin (not the codejudge ones), and
        // carries inline visible tests plus hidden `grader_private` tests.
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
