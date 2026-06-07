//! Seed a single demo course that exercises both first-party plugins
//! (Music Reviews + IRL Review). Runs after `builtins::install_all` so
//! the plugin CIDs are resolvable, and is idempotent against
//! re-invocation (uses `INSERT OR IGNORE` against deterministic ids).

use rusqlite::{params, Connection};

/// Demo course id; deterministic so re-seeding is a no-op.
const COURSE_ID: &str = "course_plugin_demo";
const CHAPTER_MUSIC: &str = "ch_plugin_demo_music";
const CHAPTER_IRL: &str = "ch_plugin_demo_irl";
const EL_MUSIC: &str = "el_plugin_demo_music_reviews";
const EL_IRL: &str = "el_plugin_demo_irl_review";
const ENROLLMENT_ID: &str = "enroll_plugin_demo";

/// Resolve a builtin plugin's CID by matching the `id` field embedded in
/// its `manifest_json`. The full id is `did:key:<author>#<slug>`, so a
/// `LIKE '%#<slug>"%'` match is enough to disambiguate.
fn find_plugin_cid(conn: &Connection, slug: &str) -> Option<String> {
    conn.query_row(
        "SELECT plugin_cid FROM plugin_installed \
         WHERE source = 'builtin' AND manifest_json LIKE ?1 \
         ORDER BY installed_at DESC LIMIT 1",
        params![format!("%#{slug}\"%")],
        |row| row.get::<_, String>(0),
    )
    .ok()
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
            "A short course that demonstrates the two first-party plugins: Music Reviews (live pitch-matched note review) and IRL Review (human-instructor review of uploaded work).",
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
