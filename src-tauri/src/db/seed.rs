//! Seed data for a fresh Alexandria database.
//!
//! Inserts a representative taxonomy (subject fields → subjects → skills),
//! prerequisite/relation edges, a governance DAO, and a sample course
//! with chapters, elements, and skill tags — giving new users something
//! to explore immediately.
//!
//! The seed function is idempotent: it only runs when the `subject_fields`
//! table is empty.

use rusqlite::Connection;

/// Seed the database with demo taxonomy, courses, and governance data.
/// Returns `Ok(true)` if seed data was inserted, `Ok(false)` if skipped.
pub fn seed_if_empty(conn: &Connection) -> Result<bool, rusqlite::Error> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM subject_fields", [], |row| row.get(0))?;

    if count > 0 {
        log::info!("Database already has taxonomy data — skipping seed");
        // Still backfill new demo data for existing databases
        backfill_demo_data(conn)?;
        return Ok(false);
    }

    log::info!("Seeding database with demo taxonomy, courses, and governance data…");

    conn.execute_batch(SEED_SQL)?;
    conn.execute_batch(BACKFILL_SQL)?;

    // Visual assets are applied via parameterized queries (not execute_batch)
    // because sqlite3_exec can silently fail on long SVG strings or emoji.
    seed_visual_assets(conn)?;

    // Inline content for all seed elements — stored directly in the database
    // so content is available on all platforms (including mobile without iroh).
    seed_inline_content(conn)?;

    log::info!("Seed data inserted successfully");

    // If the wallet is already populated (rare on a fresh seed, but
    // possible in tests and in the backfill-after-seed flow), try to
    // rebind the demo learner to the real wallet right away.
    let _ = bind_current_user_to_seed(conn);

    Ok(true)
}

/// Rewrite any demo-learner sentinel rows to the real wallet address.
///
/// Seed data references `addr_demo_learner` for learner-scoped rows
/// that are created before the user's wallet exists. Once
/// `local_identity` is populated (during `generate_wallet`,
/// `unlock_vault`, or `restore_wallet`), this function updates
/// reputation rows in place so the dashboards light up under the
/// user's real stake address.
///
/// Idempotent: after the first run the sentinel is gone and subsequent
/// invocations are no-ops. Returns the number of rows rewritten across
/// all targeted tables.
pub fn bind_current_user_to_seed(conn: &Connection) -> Result<usize, rusqlite::Error> {
    bind_current_user_to_seed_with_did(conn, None)
}

/// Like [`bind_current_user_to_seed`], but also rewrites the demo-learner
/// DID across `credentials`, `derived_skill_states`, `pinboard_observations`,
/// and adds the user as a member of seeded DAOs — so the SkillGraph and
/// governance views light up under the real DID after onboarding.
pub fn bind_current_user_to_seed_with_did(
    conn: &Connection,
    real_did: Option<&str>,
) -> Result<usize, rusqlite::Error> {
    const ADDRESS_SENTINEL: &str = "addr_demo_learner";
    const DID_SENTINEL: &str = "did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX";

    // Read the real wallet address. If none is set yet, skip silently.
    let real_address: Option<String> = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .ok();

    let Some(real_address) = real_address else {
        return Ok(0);
    };

    if real_address == ADDRESS_SENTINEL {
        return Ok(0);
    }

    let mut total: usize = 0;
    total += conn.execute(
        "UPDATE reputation_assertions SET actor_address = ?1 WHERE actor_address = ?2",
        rusqlite::params![&real_address, ADDRESS_SENTINEL],
    )?;
    // Post-migration 040: reputation_impact_deltas was dropped — the
    // per-learner impact table will be reintroduced (repointed at
    // credentials) when the reputation engine is rebuilt.

    if let Some(real_did) = real_did {
        if real_did != DID_SENTINEL {
            // Credentials: rewrite both the column and the embedded JSON
            // so frontend filters on `credential_subject.id` match.
            total += conn.execute(
                "UPDATE credentials \
                 SET subject_did = ?1, \
                     signed_vc_json = REPLACE(signed_vc_json, ?2, ?1) \
                 WHERE subject_did = ?2",
                rusqlite::params![real_did, DID_SENTINEL],
            )?;

            total += conn.execute(
                "UPDATE derived_skill_states SET subject_did = ?1 WHERE subject_did = ?2",
                rusqlite::params![real_did, DID_SENTINEL],
            )?;

            total += conn.execute(
                "UPDATE pinboard_observations SET pinner_did = ?1 WHERE pinner_did = ?2",
                rusqlite::params![real_did, DID_SENTINEL],
            )?;
        }

        // Make the user a member of two seeded DAOs so the governance
        // views show "joined" status, proposals are votable, and the
        // active election is participatable.
        for (dao_id, role) in [("dao_cs", "member"), ("dao_web", "member")] {
            total += conn.execute(
                "INSERT OR IGNORE INTO governance_dao_members (dao_id, stake_address, role) \
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![dao_id, &real_address, role],
            )?;
        }
    }

    if total > 0 {
        log::info!("Rebound {total} demo-learner rows to real wallet");
    }
    Ok(total)
}

/// Backfill demo data for tables added after the initial seed.
/// Checks each table independently so it's safe to run on any existing DB.
fn backfill_demo_data(conn: &Connection) -> Result<(), rusqlite::Error> {
    let needs_backfill = |table: &str| -> bool {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0)
            == 0
    };

    // Only backfill if any of the demo-data tables are empty. Add
    // new tables to this list when you extend BACKFILL_SQL so that
    // existing seeded DBs pick up the new content. The idempotent
    // `ON CONFLICT` clauses in BACKFILL_SQL handle the case where
    // some tables already have rows but others don't.
    if needs_backfill("enrollments")
        || needs_backfill("governance_dao_members")
        || needs_backfill("classrooms")
        || needs_backfill("video_chapters")
        || needs_backfill("opinions")
        || needs_backfill("credentials")
        || needs_backfill("pinboard_observations")
        || needs_backfill("completion_observations")
        || needs_backfill("completion_attestation_requirements")
    {
        log::info!("Backfilling demo data for new tables…");
        conn.execute_batch(BACKFILL_SQL)?;
        log::info!("Demo data backfill complete");
    }

    // Backfill thumbnails — runs every time so existing DBs that were
    // seeded before the thumbnail artwork was added pick them up. The
    // UPDATE is a no-op if the value is already correct.
    for (id, svg) in COURSE_THUMBNAILS {
        conn.execute(
            "UPDATE courses SET thumbnail_svg = ?1 WHERE id = ?2 AND (thumbnail_svg IS NULL OR thumbnail_svg = '')",
            rusqlite::params![svg, id],
        )?;
    }
    for (id, svg) in TUTORIAL_THUMBNAILS {
        conn.execute(
            "UPDATE courses SET thumbnail_svg = ?1 WHERE id = ?2 AND (thumbnail_svg IS NULL OR thumbnail_svg = '')",
            rusqlite::params![svg, id],
        )?;
    }

    // Always retry the bind after seed/backfill — cheap and idempotent.
    let _ = bind_current_user_to_seed(conn);

    Ok(())
}

/// Store element content directly in the `content_inline` column.
/// This makes content available on all platforms (including mobile without iroh).
fn seed_inline_content(conn: &Connection) -> Result<(), rusqlite::Error> {
    use super::seed_content::SEED_CONTENT;
    use rusqlite::params;

    for (element_id, body) in SEED_CONTENT {
        conn.execute(
            "UPDATE course_elements SET content_inline = ?1 WHERE id = ?2",
            params![body, element_id],
        )?;
    }
    Ok(())
}

/// Apply visual assets (emojis, author names, thumbnails) via parameterized queries.
fn seed_visual_assets(conn: &Connection) -> Result<(), rusqlite::Error> {
    use rusqlite::params;

    // Subject field emojis
    for (id, emoji) in [
        ("sf_cs", "\u{1F4BB}"),     // 💻
        ("sf_math", "\u{1F4D0}"),   // 📐
        ("sf_data", "\u{1F4CA}"),   // 📊
        ("sf_web", "\u{1F310}"),    // 🌐
        ("sf_cyber", "\u{1F510}"),  // 🔐
        ("sf_design", "\u{1F3A8}"), // 🎨
        ("sf_civics", "\u{1F5F3}"), // 🗳
    ] {
        conn.execute(
            "UPDATE subject_fields SET icon_emoji = ?1 WHERE id = ?2",
            params![emoji, id],
        )?;
    }

    // DAO emojis
    for (id, emoji) in [
        ("dao_cs", "\u{1F4BB}"),
        ("dao_math", "\u{1F4D0}"),
        ("dao_data", "\u{1F4CA}"),
        ("dao_web", "\u{1F310}"),
        ("dao_cyber", "\u{1F510}"),
        ("dao_design", "\u{1F3A8}"),
        ("dao_civics", "\u{1F5F3}"),
    ] {
        conn.execute(
            "UPDATE governance_daos SET icon_emoji = ?1 WHERE id = ?2",
            params![emoji, id],
        )?;
    }

    // Course author display names
    for (id, name) in [
        ("course_algo_101", "Dr. Elena Vasquez"),
        ("course_web_fullstack", "Dr. Elena Vasquez"),
        ("course_ml_foundations", "Marcus Chen"),
        ("course_crypto_101", "Marcus Chen"),
        ("course_ux_design", "Amara Osei"),
        ("course_math_discrete", "Prof. Imani Okafor"),
        ("course_civics_101", "Dr. Nomvula Dlamini"),
        ("course_tut_civ_constitution", "Dr. Nomvula Dlamini"),
        ("course_tut_civ_budget", "Dr. Nomvula Dlamini"),
    ] {
        conn.execute(
            "UPDATE courses SET author_name = ?1 WHERE id = ?2",
            params![name, id],
        )?;
    }

    // Course thumbnail SVGs
    for (id, svg) in COURSE_THUMBNAILS {
        conn.execute(
            "UPDATE courses SET thumbnail_svg = ?1 WHERE id = ?2",
            params![svg, id],
        )?;
    }

    // Tutorial thumbnail SVGs (distinct style: wider, play-button, cinematic)
    for (id, svg) in TUTORIAL_THUMBNAILS {
        conn.execute(
            "UPDATE courses SET thumbnail_svg = ?1 WHERE id = ?2",
            params![svg, id],
        )?;
    }

    Ok(())
}

// Tutorial thumbnails — wider aspect (2:1), play-button motif, bolder
// gradients with a cinematic feel. Visually distinct from course cards.
const TUTORIAL_THUMBNAILS: &[(&str, &str)] = &[
    (
        "course_tut_bigO",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 320"><rect width="640" height="320" rx="0" fill="#1e1b4b"/><circle cx="520" cy="160" r="120" fill="#4338ca" opacity="0.3"/><circle cx="120" cy="160" r="80" fill="#6366f1" opacity="0.15"/><polygon points="280,120 340,160 280,200" fill="#fff" opacity="0.9"/><text x="400" y="155" fill="#fff" font-family="system-ui,sans-serif" font-size="20" font-weight="700" opacity="0.9">Big-O in 8 Min</text><text x="400" y="180" fill="#a5b4fc" font-family="system-ui,sans-serif" font-size="12" opacity="0.7">Complexity fundamentals</text></svg>"##,
    ),
    (
        "course_tut_asyncawait",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 320"><rect width="640" height="320" fill="#0c4a6e"/><circle cx="500" cy="100" r="160" fill="#0284c7" opacity="0.2"/><circle cx="160" cy="240" r="100" fill="#0ea5e9" opacity="0.12"/><polygon points="280,120 340,160 280,200" fill="#fff" opacity="0.9"/><text x="400" y="155" fill="#fff" font-family="system-ui,sans-serif" font-size="20" font-weight="700" opacity="0.9">Async / Await</text><text x="400" y="180" fill="#7dd3fc" font-family="system-ui,sans-serif" font-size="12" opacity="0.7">Concurrency quick tour</text></svg>"##,
    ),
    (
        "course_tut_ml_regression",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 320"><rect width="640" height="320" fill="#431407"/><circle cx="540" cy="140" r="140" fill="#ea580c" opacity="0.15"/><circle cx="100" cy="200" r="100" fill="#f97316" opacity="0.1"/><polygon points="280,120 340,160 280,200" fill="#fff" opacity="0.9"/><text x="400" y="155" fill="#fff" font-family="system-ui,sans-serif" font-size="18" font-weight="700" opacity="0.9">Linear Regression</text><text x="400" y="180" fill="#fdba74" font-family="system-ui,sans-serif" font-size="12" opacity="0.7">From first principles</text></svg>"##,
    ),
    (
        "course_tut_aes",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 320"><rect width="640" height="320" fill="#052e16"/><circle cx="480" cy="120" r="150" fill="#16a34a" opacity="0.15"/><circle cx="140" cy="220" r="90" fill="#22c55e" opacity="0.1"/><polygon points="280,120 340,160 280,200" fill="#fff" opacity="0.9"/><text x="400" y="155" fill="#fff" font-family="system-ui,sans-serif" font-size="20" font-weight="700" opacity="0.9">AES Walkthrough</text><text x="400" y="180" fill="#86efac" font-family="system-ui,sans-serif" font-size="12" opacity="0.7">Block cipher step-by-step</text></svg>"##,
    ),
    (
        "course_tut_ux_interviews",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 320"><rect width="640" height="320" fill="#4a044e"/><circle cx="520" cy="160" r="130" fill="#c026d3" opacity="0.15"/><circle cx="120" cy="180" r="100" fill="#e879f9" opacity="0.08"/><polygon points="280,120 340,160 280,200" fill="#fff" opacity="0.9"/><text x="400" y="155" fill="#fff" font-family="system-ui,sans-serif" font-size="18" font-weight="700" opacity="0.9">User Interviews</text><text x="400" y="180" fill="#f0abfc" font-family="system-ui,sans-serif" font-size="12" opacity="0.7">Run great research sessions</text></svg>"##,
    ),
    (
        "course_tut_civ_constitution",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 320"><rect width="640" height="320" fill="#450a0a"/><circle cx="500" cy="140" r="140" fill="#dc2626" opacity="0.12"/><circle cx="140" cy="200" r="90" fill="#f59e0b" opacity="0.08"/><polygon points="280,120 340,160 280,200" fill="#fff" opacity="0.9"/><text x="400" y="155" fill="#fff" font-family="system-ui,sans-serif" font-size="18" font-weight="700" opacity="0.9">Constitutions 101</text><text x="400" y="180" fill="#fca5a5" font-family="system-ui,sans-serif" font-size="12" opacity="0.7">Reading a national charter</text></svg>"##,
    ),
    (
        "course_tut_civ_budget",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 320"><rect width="640" height="320" fill="#422006"/><circle cx="520" cy="150" r="130" fill="#d97706" opacity="0.15"/><circle cx="120" cy="190" r="100" fill="#fbbf24" opacity="0.08"/><polygon points="280,120 340,160 280,200" fill="#fff" opacity="0.9"/><text x="400" y="155" fill="#fff" font-family="system-ui,sans-serif" font-size="18" font-weight="700" opacity="0.9">Reading a Budget</text><text x="400" y="180" fill="#fde68a" font-family="system-ui,sans-serif" font-size="12" opacity="0.7">Where your taxes go</text></svg>"##,
    ),
];

const COURSE_THUMBNAILS: &[(&str, &str)] = &[
    (
        "course_algo_101",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#6366f1"/><stop offset="100%" stop-color="#8b5cf6"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.15" fill="none" stroke="#fff" stroke-width="2"><line x1="80" y1="280" x2="160" y2="200"/><line x1="160" y1="200" x2="240" y2="120"/><line x1="240" y1="120" x2="320" y2="180"/><line x1="320" y1="180" x2="400" y2="80"/><line x1="400" y1="80" x2="480" y2="160"/><line x1="480" y1="160" x2="560" y2="100"/><circle cx="80" cy="280" r="6" fill="#fff"/><circle cx="160" cy="200" r="6" fill="#fff"/><circle cx="240" cy="120" r="6" fill="#fff"/><circle cx="320" cy="180" r="6" fill="#fff"/><circle cx="400" cy="80" r="6" fill="#fff"/><circle cx="480" cy="160" r="6" fill="#fff"/><circle cx="560" cy="100" r="6" fill="#fff"/></g><g opacity="0.08" fill="#fff"><rect x="100" y="240" width="40" height="80" rx="4"/><rect x="160" y="200" width="40" height="120" rx="4"/><rect x="220" y="160" width="40" height="160" rx="4"/><rect x="280" y="180" width="40" height="140" rx="4"/><rect x="340" y="120" width="40" height="200" rx="4"/><rect x="400" y="100" width="40" height="220" rx="4"/><rect x="460" y="140" width="40" height="180" rx="4"/></g><text x="320" y="175" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="28" font-weight="700" opacity="0.9">O(n log n)</text><text x="320" y="210" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="14" opacity="0.6">Algorithms &amp; Data Structures</text></svg>"##,
    ),
    (
        "course_web_fullstack",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#0ea5e9"/><stop offset="100%" stop-color="#6366f1"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.1" fill="none" stroke="#fff" stroke-width="1.5"><rect x="60" y="40" width="200" height="280" rx="12"/><rect x="80" y="60" width="160" height="20" rx="4"/><rect x="80" y="90" width="120" height="12" rx="3"/><rect x="80" y="110" width="140" height="12" rx="3"/><rect x="80" y="130" width="100" height="12" rx="3"/><rect x="80" y="160" width="160" height="100" rx="6"/><rect x="80" y="270" width="70" height="28" rx="6"/><rect x="160" y="270" width="70" height="28" rx="6"/></g><g opacity="0.12" fill="#fff"><circle cx="440" cy="180" r="80"/><circle cx="440" cy="180" r="60" fill="none" stroke="#fff" stroke-width="2"/><path d="M420 160 L430 180 L460 180 L435 195 L445 215 L420 200 L395 215 L405 195 L380 180 L410 180Z"/></g><g opacity="0.07" fill="none" stroke="#fff" stroke-width="1"><line x1="340" y1="100" x2="540" y2="100"/><line x1="340" y1="130" x2="540" y2="130"/><line x1="340" y1="230" x2="540" y2="230"/><line x1="340" y1="260" x2="540" y2="260"/></g><text x="320" y="170" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="24" font-weight="700" opacity="0.9">&lt;Vue /&gt; + Rust</text><text x="320" y="205" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="14" opacity="0.6">Full-Stack Web Development</text></svg>"##,
    ),
    (
        "course_ml_foundations",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#f59e0b"/><stop offset="100%" stop-color="#ef4444"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.12" fill="none" stroke="#fff" stroke-width="2"><circle cx="200" cy="180" r="8"/><circle cx="280" cy="120" r="8"/><circle cx="360" cy="200" r="8"/><circle cx="440" cy="140" r="8"/><circle cx="320" cy="80" r="8"/><circle cx="160" cy="260" r="8"/><circle cx="480" cy="240" r="8"/><circle cx="400" cy="280" r="8"/><circle cx="240" cy="220" r="8"/><circle cx="520" cy="160" r="8"/><line x1="200" y1="180" x2="280" y2="120"/><line x1="280" y1="120" x2="360" y2="200"/><line x1="360" y1="200" x2="440" y2="140"/><line x1="280" y1="120" x2="320" y2="80"/><line x1="200" y1="180" x2="160" y2="260"/><line x1="440" y1="140" x2="480" y2="240"/><line x1="360" y1="200" x2="400" y2="280"/><line x1="200" y1="180" x2="240" y2="220"/><line x1="440" y1="140" x2="520" y2="160"/></g><g opacity="0.08" fill="#fff"><circle cx="320" cy="180" r="100"/><ellipse cx="320" cy="180" rx="140" ry="60" fill="none" stroke="#fff" stroke-width="1" stroke-dasharray="4,4"/></g><text x="320" y="170" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="26" font-weight="700" opacity="0.9">f(x) = wx + b</text><text x="320" y="205" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="14" opacity="0.6">Machine Learning Foundations</text></svg>"##,
    ),
    (
        "course_crypto_101",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#10b981"/><stop offset="100%" stop-color="#0ea5e9"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.1" fill="none" stroke="#fff" stroke-width="2"><rect x="180" y="80" width="280" height="200" rx="16"/><circle cx="320" cy="140" r="30"/><path d="M290 140 L320 110 L350 140" stroke-width="3"/><line x1="240" y1="200" x2="400" y2="200"/><rect x="240" y="220" width="60" height="8" rx="4"/><rect x="320" y="220" width="80" height="8" rx="4"/></g><g opacity="0.07" fill="#fff"><circle cx="120" cy="100" r="4"/><circle cx="520" cy="80" r="4"/><circle cx="100" cy="260" r="4"/><circle cx="540" cy="280" r="4"/><line x1="120" y1="100" x2="180" y2="80" stroke="#fff" stroke-width="1"/><line x1="460" y1="80" x2="520" y2="80" stroke="#fff" stroke-width="1"/><line x1="100" y1="260" x2="180" y2="280" stroke="#fff" stroke-width="1"/><line x1="460" y1="280" x2="540" y2="280" stroke="#fff" stroke-width="1"/></g><g opacity="0.06"><rect x="60" y="300" width="520" height="30" rx="4" fill="#fff"/><rect x="70" y="306" width="100" height="18" rx="3" fill="none" stroke="#fff" stroke-width="1"/><rect x="180" y="306" width="80" height="18" rx="3" fill="none" stroke="#fff" stroke-width="1"/></g><text x="320" y="170" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="24" font-weight="700" opacity="0.9">AES-256 + Ed25519</text><text x="320" y="205" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="14" opacity="0.6">Applied Cryptography</text></svg>"##,
    ),
    (
        "course_ux_design",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#ec4899"/><stop offset="100%" stop-color="#f59e0b"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.1" fill="none" stroke="#fff" stroke-width="1.5"><rect x="100" y="60" width="180" height="240" rx="12"/><circle cx="190" cy="120" r="24"/><rect x="130" y="160" width="120" height="8" rx="4"/><rect x="140" y="178" width="100" height="6" rx="3"/><rect x="130" y="200" width="120" height="40" rx="6"/><rect x="130" y="250" width="50" height="28" rx="14"/><rect x="200" y="250" width="50" height="28" rx="14"/></g><g opacity="0.1" fill="none" stroke="#fff" stroke-width="1.5"><rect x="360" y="60" width="180" height="240" rx="12"/><rect x="390" y="90" width="120" height="80" rx="8"/><rect x="390" y="185" width="80" height="8" rx="4"/><rect x="390" y="205" width="120" height="6" rx="3"/><rect x="390" y="225" width="100" height="6" rx="3"/><rect x="390" y="255" width="60" height="24" rx="12"/></g><g opacity="0.07" fill="#fff"><path d="M300 160 L320 140 L340 160" stroke="#fff" stroke-width="2" fill="none"/><line x1="320" y1="160" x2="320" y2="200" stroke="#fff" stroke-width="2"/><circle cx="320" cy="220" r="4"/></g><text x="320" y="170" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="26" font-weight="700" opacity="0.9">UX Design</text><text x="320" y="205" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="14" opacity="0.6">Research to Prototype</text></svg>"##,
    ),
    (
        "course_math_discrete",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#14b8a6"/><stop offset="100%" stop-color="#0ea5e9"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.12" fill="none" stroke="#fff" stroke-width="2"><circle cx="160" cy="90" r="6"/><circle cx="250" cy="140" r="6"/><circle cx="350" cy="100" r="6"/><circle cx="450" cy="170" r="6"/><circle cx="540" cy="120" r="6"/><line x1="160" y1="90" x2="250" y2="140"/><line x1="250" y1="140" x2="350" y2="100"/><line x1="350" y1="100" x2="450" y2="170"/><line x1="450" y1="170" x2="540" y2="120"/></g><g opacity="0.08" fill="#fff"><rect x="90" y="220" width="460" height="90" rx="12"/><rect x="120" y="245" width="120" height="16" rx="4"/><rect x="260" y="245" width="120" height="16" rx="4"/><rect x="400" y="245" width="120" height="16" rx="4"/></g><text x="320" y="165" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="26" font-weight="700" opacity="0.92">Discrete Math</text><text x="320" y="200" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="14" opacity="0.65">Logic, Sets, Graphs, Probability</text></svg>"##,
    ),
    (
        "course_civics_101",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#dc2626"/><stop offset="50%" stop-color="#f59e0b"/><stop offset="100%" stop-color="#059669"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><g opacity="0.10" fill="none" stroke="#fff" stroke-width="1.5"><rect x="80" y="60" width="200" height="240" rx="10"/><rect x="110" y="100" width="140" height="10" rx="3"/><rect x="110" y="125" width="120" height="8" rx="3"/><rect x="110" y="145" width="130" height="8" rx="3"/><rect x="110" y="180" width="140" height="70" rx="6"/><rect x="110" y="260" width="60" height="22" rx="4"/><rect x="180" y="260" width="60" height="22" rx="4"/></g><g opacity="0.12" fill="#fff"><circle cx="440" cy="140" r="52" fill="none" stroke="#fff" stroke-width="2"/><path d="M420 122 L430 140 L440 126 L450 142 L460 126" stroke="#fff" stroke-width="2" fill="none"/><rect x="388" y="185" width="104" height="10" rx="3"/><rect x="408" y="205" width="64" height="6" rx="2"/></g><g opacity="0.08" fill="none" stroke="#fff" stroke-width="1"><path d="M340 240 C 380 260 420 260 460 240 C 500 220 540 220 580 240"/><path d="M340 260 C 380 280 420 280 460 260 C 500 240 540 240 580 260"/></g><text x="320" y="175" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="24" font-weight="700" opacity="0.92">Civic Sense</text><text x="320" y="205" text-anchor="middle" fill="#fff" font-family="system-ui,sans-serif" font-size="13" opacity="0.7">For the Global South</text></svg>"##,
    ),
];

const SEED_SQL: &str = r##"
-- ============================================================
-- SUBJECT FIELDS (top-level knowledge domains)
-- ============================================================
INSERT INTO subject_fields (id, name, description) VALUES
    ('sf_cs',       'Computer Science',     'The study of computation, algorithms, data structures, and software systems'),
    ('sf_math',     'Mathematics',          'Pure and applied mathematics including algebra, calculus, and discrete math'),
    ('sf_data',     'Data Science',         'Statistics, machine learning, data engineering, and analytical methods'),
    ('sf_web',      'Web Development',      'Frontend, backend, and full-stack web application development'),
    ('sf_cyber',    'Cybersecurity',        'Information security, cryptography, and defensive/offensive techniques'),
    ('sf_design',   'Design',              'User experience, interface design, and visual communication');

-- ============================================================
-- SUBJECTS (mid-level topics within fields)
-- ============================================================
INSERT INTO subjects (id, name, description, subject_field_id) VALUES
    -- Computer Science
    ('sub_algo',        'Algorithms & Data Structures',     'Fundamental algorithms, complexity analysis, and core data structures',       'sf_cs'),
    ('sub_os',          'Operating Systems',                'Process management, memory, file systems, and concurrency',                  'sf_cs'),
    ('sub_lang',        'Programming Languages',            'Language design, type systems, compilers, and paradigms',                    'sf_cs'),
    ('sub_net',         'Computer Networks',                'Protocols, routing, transport layers, and network architecture',             'sf_cs'),
    -- Mathematics
    ('sub_calc',        'Calculus',                         'Limits, derivatives, integrals, and multivariable calculus',                 'sf_math'),
    ('sub_linalg',      'Linear Algebra',                   'Vectors, matrices, transformations, and eigenvalues',                       'sf_math'),
    ('sub_discrete',    'Discrete Mathematics',             'Logic, sets, combinatorics, graph theory, and proofs',                      'sf_math'),
    ('sub_prob',        'Probability & Statistics',         'Random variables, distributions, inference, and hypothesis testing',         'sf_math'),
    -- Data Science
    ('sub_ml',          'Machine Learning',                 'Supervised, unsupervised, and reinforcement learning algorithms',            'sf_data'),
    ('sub_nlp',         'Natural Language Processing',      'Text processing, language models, and sequence-to-sequence architectures',   'sf_data'),
    ('sub_dataeng',     'Data Engineering',                 'Pipelines, ETL, warehousing, and streaming architectures',                  'sf_data'),
    -- Web Development
    ('sub_frontend',    'Frontend Development',             'HTML, CSS, JavaScript, frameworks, and browser APIs',                       'sf_web'),
    ('sub_backend',     'Backend Development',              'Server-side logic, APIs, databases, and authentication',                    'sf_web'),
    ('sub_devops',      'DevOps & Deployment',              'CI/CD, containers, infrastructure as code, and monitoring',                 'sf_web'),
    -- Cybersecurity
    ('sub_crypto',      'Cryptography',                     'Symmetric/asymmetric encryption, hashing, digital signatures',              'sf_cyber'),
    ('sub_netsec',      'Network Security',                 'Firewalls, intrusion detection, TLS, and secure protocols',                 'sf_cyber'),
    -- Design
    ('sub_ux',          'UX Design',                        'User research, information architecture, and interaction design',           'sf_design'),
    ('sub_ui',          'UI Design',                        'Visual design, typography, color theory, and design systems',               'sf_design');

-- ============================================================
-- SKILLS (leaf-level assessable abilities)
-- ============================================================
INSERT INTO skills (id, name, description, subject_id, bloom_level) VALUES
    -- Algorithms & Data Structures
    ('skill_big_o',         'Big-O Analysis',                   'Analyze time and space complexity of algorithms',                    'sub_algo',     'analyze'),
    ('skill_arrays',        'Arrays & Strings',                 'Implement and manipulate array-based data structures',               'sub_algo',     'apply'),
    ('skill_linked_lists',  'Linked Lists',                     'Implement singly and doubly linked list operations',                 'sub_algo',     'apply'),
    ('skill_stacks_queues', 'Stacks & Queues',                  'Implement LIFO and FIFO data structures and their applications',    'sub_algo',     'apply'),
    ('skill_trees',         'Trees & BSTs',                     'Implement binary trees, BSTs, and tree traversal algorithms',        'sub_algo',     'apply'),
    ('skill_graphs',        'Graph Algorithms',                 'Implement BFS, DFS, shortest paths, and spanning trees',            'sub_algo',     'apply'),
    ('skill_sorting',       'Sorting Algorithms',               'Implement and compare comparison and non-comparison sorts',          'sub_algo',     'apply'),
    ('skill_hashing',       'Hash Tables',                      'Implement hash maps with collision resolution strategies',           'sub_algo',     'apply'),
    ('skill_dp',            'Dynamic Programming',              'Solve optimization problems using memoization and tabulation',       'sub_algo',     'analyze'),
    ('skill_greedy',        'Greedy Algorithms',                'Design greedy strategies and prove correctness',                     'sub_algo',     'analyze'),

    -- Operating Systems
    ('skill_processes',     'Processes & Threads',              'Manage process lifecycle, threading models, and scheduling',         'sub_os',       'understand'),
    ('skill_memory',        'Memory Management',                'Explain paging, virtual memory, and memory allocation',              'sub_os',       'understand'),
    ('skill_concurrency',   'Concurrency & Synchronization',    'Use locks, semaphores, and message passing to prevent data races',  'sub_os',       'apply'),
    ('skill_filesystems',   'File Systems',                     'Describe file system structures, journaling, and I/O scheduling',   'sub_os',       'understand'),

    -- Programming Languages
    ('skill_rust',          'Rust Programming',                 'Write safe, concurrent Rust code using ownership and lifetimes',     'sub_lang',     'apply'),
    ('skill_typescript',    'TypeScript',                       'Build type-safe applications with TypeScript and its type system',   'sub_lang',     'apply'),
    ('skill_python',        'Python',                           'Write idiomatic Python for scripting, data, and web development',   'sub_lang',     'apply'),
    ('skill_functional',    'Functional Programming',           'Apply FP concepts: immutability, higher-order functions, monads',   'sub_lang',     'understand'),

    -- Computer Networks
    ('skill_tcp_ip',        'TCP/IP Stack',                     'Explain the TCP/IP model layers and protocol interactions',          'sub_net',      'understand'),
    ('skill_http',          'HTTP & REST',                      'Design and consume RESTful APIs using HTTP methods and status codes','sub_net',      'apply'),
    ('skill_dns',           'DNS',                              'Explain DNS resolution, record types, and caching',                 'sub_net',      'remember'),

    -- Calculus
    ('skill_limits',        'Limits & Continuity',              'Evaluate limits and determine continuity of functions',              'sub_calc',     'apply'),
    ('skill_derivatives',   'Derivatives',                      'Compute derivatives and apply differentiation rules',               'sub_calc',     'apply'),
    ('skill_integrals',     'Integration',                      'Evaluate definite and indefinite integrals using standard techniques','sub_calc',    'apply'),
    ('skill_multivariable', 'Multivariable Calculus',           'Compute partial derivatives, gradients, and multiple integrals',    'sub_calc',     'apply'),

    -- Linear Algebra
    ('skill_vectors',       'Vectors & Spaces',                 'Perform vector operations and reason about vector spaces',          'sub_linalg',   'apply'),
    ('skill_matrices',      'Matrix Operations',                'Multiply, invert, and decompose matrices',                          'sub_linalg',   'apply'),
    ('skill_eigenvalues',   'Eigenvalues & Eigenvectors',       'Compute eigenvalues/eigenvectors and apply spectral decomposition', 'sub_linalg',   'analyze'),
    ('skill_svd',           'Singular Value Decomposition',     'Compute and interpret SVD for dimensionality reduction',            'sub_linalg',   'analyze'),

    -- Discrete Mathematics
    ('skill_logic',         'Propositional & Predicate Logic',  'Construct and evaluate logical propositions and proofs',            'sub_discrete', 'apply'),
    ('skill_sets',          'Set Theory',                       'Apply set operations, relations, and functions',                     'sub_discrete', 'apply'),
    ('skill_combinatorics', 'Combinatorics',                    'Solve counting problems using permutations and combinations',       'sub_discrete', 'apply'),
    ('skill_graph_theory',  'Graph Theory',                     'Prove properties of graphs, trees, and network flows',              'sub_discrete', 'analyze'),

    -- Probability & Statistics
    ('skill_probability',   'Probability Fundamentals',         'Compute probabilities using axioms, Bayes theorem, and distributions','sub_prob',   'apply'),
    ('skill_distributions', 'Probability Distributions',        'Work with common distributions (normal, binomial, Poisson, etc.)', 'sub_prob',     'apply'),
    ('skill_inference',     'Statistical Inference',            'Perform hypothesis testing, confidence intervals, and estimation',  'sub_prob',     'analyze'),
    ('skill_regression',    'Regression Analysis',              'Build and interpret linear and logistic regression models',          'sub_prob',     'apply'),

    -- Machine Learning
    ('skill_supervised',    'Supervised Learning',              'Train and evaluate classification and regression models',            'sub_ml',       'apply'),
    ('skill_unsupervised',  'Unsupervised Learning',            'Apply clustering, dimensionality reduction, and anomaly detection', 'sub_ml',       'apply'),
    ('skill_neural_nets',   'Neural Networks',                  'Build and train feedforward and convolutional neural networks',      'sub_ml',       'apply'),
    ('skill_deep_learning', 'Deep Learning',                    'Design deep architectures: RNNs, transformers, GANs',               'sub_ml',       'create'),
    ('skill_ml_eval',       'Model Evaluation',                 'Apply cross-validation, metrics, and bias-variance analysis',       'sub_ml',       'evaluate'),

    -- Natural Language Processing
    ('skill_tokenization',  'Text Preprocessing',               'Tokenize, normalize, and vectorize text data',                     'sub_nlp',      'apply'),
    ('skill_embeddings',    'Word Embeddings',                  'Use Word2Vec, GloVe, and contextual embeddings',                    'sub_nlp',      'apply'),
    ('skill_transformers',  'Transformer Architecture',         'Explain and implement self-attention and transformer models',       'sub_nlp',      'analyze'),

    -- Data Engineering
    ('skill_sql',           'SQL',                              'Write complex queries with joins, subqueries, and window functions','sub_dataeng',  'apply'),
    ('skill_etl',           'ETL Pipelines',                    'Design and implement extract-transform-load workflows',             'sub_dataeng',  'apply'),
    ('skill_streaming',     'Stream Processing',                'Build real-time data pipelines with event streaming platforms',      'sub_dataeng',  'apply'),

    -- Frontend Development
    ('skill_html_css',      'HTML & CSS',                       'Write semantic HTML and responsive CSS layouts',                    'sub_frontend', 'apply'),
    ('skill_javascript',    'JavaScript',                       'Write modern ES6+ JavaScript for browser and server environments', 'sub_frontend', 'apply'),
    ('skill_vue',           'Vue.js',                           'Build reactive UIs with Vue 3 Composition API and SFCs',           'sub_frontend', 'apply'),
    ('skill_react',         'React',                            'Build component-based UIs with React hooks and state management',  'sub_frontend', 'apply'),
    ('skill_accessibility', 'Web Accessibility',                'Implement WCAG guidelines and ARIA patterns',                       'sub_frontend', 'evaluate'),

    -- Backend Development
    ('skill_rest_api',      'REST API Design',                  'Design resource-oriented APIs with proper status codes and auth',   'sub_backend',  'create'),
    ('skill_auth',          'Authentication & Authorization',   'Implement JWT, OAuth 2.0, and role-based access control',           'sub_backend',  'apply'),
    ('skill_db_design',     'Database Design',                  'Normalize schemas, design indexes, and manage migrations',          'sub_backend',  'create'),
    ('skill_graphql',       'GraphQL',                          'Design and implement GraphQL schemas, resolvers, and subscriptions','sub_backend',  'apply'),

    -- DevOps & Deployment
    ('skill_docker',        'Docker & Containers',              'Build container images, compose services, and manage registries',   'sub_devops',   'apply'),
    ('skill_ci_cd',         'CI/CD Pipelines',                  'Configure automated build, test, and deploy pipelines',             'sub_devops',   'apply'),
    ('skill_k8s',           'Kubernetes',                       'Deploy and manage containerized apps on Kubernetes clusters',       'sub_devops',   'apply'),

    -- Cryptography
    ('skill_symmetric',     'Symmetric Encryption',             'Apply AES, ChaCha20, and block cipher modes of operation',          'sub_crypto',   'apply'),
    ('skill_asymmetric',    'Asymmetric Encryption',            'Use RSA, elliptic curves, and key exchange protocols',              'sub_crypto',   'apply'),
    ('skill_hash_crypto',   'Cryptographic Hashing',            'Apply SHA-256, BLAKE2, and hash-based data structures',            'sub_crypto',   'apply'),
    ('skill_signatures',    'Digital Signatures',               'Implement and verify Ed25519 and ECDSA signatures',                'sub_crypto',   'apply'),
    ('skill_zk',            'Zero-Knowledge Proofs',            'Explain ZK-SNARKs, ZK-STARKs, and their applications',             'sub_crypto',   'understand'),

    -- Network Security
    ('skill_tls',           'TLS & Certificate Management',     'Configure TLS, manage certificates, and diagnose handshake issues','sub_netsec',   'apply'),
    ('skill_firewalls',     'Firewalls & IDS',                  'Configure firewall rules and intrusion detection systems',          'sub_netsec',   'apply'),

    -- UX Design
    ('skill_user_research', 'User Research',                    'Plan and conduct user interviews, surveys, and usability tests',    'sub_ux',       'evaluate'),
    ('skill_ia',            'Information Architecture',          'Organize content structures, navigation, and labeling systems',     'sub_ux',       'create'),
    ('skill_wireframing',   'Wireframing & Prototyping',        'Create low and high-fidelity wireframes and interactive prototypes','sub_ux',       'create'),

    -- UI Design
    ('skill_color_theory',  'Color Theory',                     'Apply color harmony, contrast, and accessibility standards',        'sub_ui',       'apply'),
    ('skill_typography',    'Typography',                       'Select and pair typefaces, set scales, and manage readability',      'sub_ui',       'apply'),
    ('skill_design_systems','Design Systems',                   'Build and maintain reusable component libraries and tokens',        'sub_ui',       'create');

-- ============================================================
-- SKILL PREREQUISITES (directed edges in the DAG)
-- ============================================================
INSERT INTO skill_prerequisites (skill_id, prerequisite_id) VALUES
    -- Algorithms chain
    ('skill_sorting',       'skill_arrays'),
    ('skill_sorting',       'skill_big_o'),
    ('skill_linked_lists',  'skill_arrays'),
    ('skill_stacks_queues', 'skill_arrays'),
    ('skill_stacks_queues', 'skill_linked_lists'),
    ('skill_trees',         'skill_linked_lists'),
    ('skill_trees',         'skill_stacks_queues'),
    ('skill_graphs',        'skill_trees'),
    ('skill_graphs',        'skill_stacks_queues'),
    ('skill_hashing',       'skill_arrays'),
    ('skill_dp',            'skill_big_o'),
    ('skill_dp',            'skill_arrays'),
    ('skill_greedy',        'skill_big_o'),
    ('skill_greedy',        'skill_sorting'),

    -- OS chain
    ('skill_concurrency',   'skill_processes'),
    ('skill_memory',        'skill_processes'),
    ('skill_filesystems',   'skill_memory'),

    -- Programming chain
    ('skill_functional',    'skill_python'),

    -- Networks chain
    ('skill_http',          'skill_tcp_ip'),
    ('skill_dns',           'skill_tcp_ip'),

    -- Calculus chain
    ('skill_derivatives',   'skill_limits'),
    ('skill_integrals',     'skill_derivatives'),
    ('skill_multivariable', 'skill_integrals'),

    -- Linear Algebra chain
    ('skill_matrices',      'skill_vectors'),
    ('skill_eigenvalues',   'skill_matrices'),
    ('skill_svd',           'skill_eigenvalues'),

    -- Discrete chain
    ('skill_combinatorics', 'skill_sets'),
    ('skill_graph_theory',  'skill_logic'),
    ('skill_graph_theory',  'skill_sets'),

    -- Statistics chain
    ('skill_distributions', 'skill_probability'),
    ('skill_inference',     'skill_distributions'),
    ('skill_regression',    'skill_distributions'),

    -- ML chain
    ('skill_supervised',    'skill_regression'),
    ('skill_supervised',    'skill_matrices'),
    ('skill_unsupervised',  'skill_matrices'),
    ('skill_unsupervised',  'skill_probability'),
    ('skill_neural_nets',   'skill_supervised'),
    ('skill_neural_nets',   'skill_derivatives'),
    ('skill_deep_learning', 'skill_neural_nets'),
    ('skill_ml_eval',       'skill_supervised'),
    ('skill_ml_eval',       'skill_inference'),

    -- NLP chain
    ('skill_embeddings',    'skill_tokenization'),
    ('skill_embeddings',    'skill_vectors'),
    ('skill_transformers',  'skill_embeddings'),
    ('skill_transformers',  'skill_neural_nets'),

    -- Data Engineering chain
    ('skill_etl',           'skill_sql'),
    ('skill_streaming',     'skill_etl'),

    -- Frontend chain
    ('skill_javascript',    'skill_html_css'),
    ('skill_vue',           'skill_javascript'),
    ('skill_react',         'skill_javascript'),
    ('skill_accessibility', 'skill_html_css'),
    ('skill_typescript',    'skill_javascript'),

    -- Backend chain
    ('skill_rest_api',      'skill_http'),
    ('skill_auth',          'skill_rest_api'),
    ('skill_db_design',     'skill_sql'),
    ('skill_graphql',       'skill_rest_api'),

    -- DevOps chain
    ('skill_ci_cd',         'skill_docker'),
    ('skill_k8s',           'skill_docker'),

    -- Crypto chain
    ('skill_asymmetric',    'skill_symmetric'),
    ('skill_hash_crypto',   'skill_symmetric'),
    ('skill_signatures',    'skill_asymmetric'),
    ('skill_signatures',    'skill_hash_crypto'),
    ('skill_zk',            'skill_asymmetric'),

    -- Security chain
    ('skill_tls',           'skill_asymmetric'),
    ('skill_tls',           'skill_hash_crypto'),
    ('skill_firewalls',     'skill_tcp_ip'),

    -- UX chain
    ('skill_wireframing',   'skill_user_research'),
    ('skill_wireframing',   'skill_ia'),

    -- UI chain
    ('skill_design_systems','skill_color_theory'),
    ('skill_design_systems','skill_typography');

-- ============================================================
-- SKILL RELATIONS (undirected cross-topic links)
-- ============================================================
INSERT INTO skill_relations (skill_id, related_skill_id, relation_type) VALUES
    ('skill_graph_theory',  'skill_graphs',         'complementary'),
    ('skill_big_o',         'skill_logic',          'complementary'),
    ('skill_hashing',       'skill_hash_crypto',    'related'),
    ('skill_vue',           'skill_react',          'alternative'),
    ('skill_rest_api',      'skill_graphql',        'alternative'),
    ('skill_docker',        'skill_k8s',            'complementary'),
    ('skill_supervised',    'skill_unsupervised',   'complementary'),
    ('skill_sql',           'skill_db_design',      'complementary'),
    ('skill_color_theory',  'skill_accessibility',  'related'),
    ('skill_typography',    'skill_html_css',       'related'),
    ('skill_user_research', 'skill_accessibility',  'complementary'),
    ('skill_tls',           'skill_firewalls',      'complementary'),
    ('skill_eigenvalues',   'skill_deep_learning',  'complementary'),
    ('skill_probability',   'skill_ml_eval',        'complementary');

-- ============================================================
-- GOVERNANCE DAOs
-- ============================================================
INSERT INTO governance_daos (id, name, description, scope_type, scope_id, status, committee_size, election_interval_days) VALUES
    ('dao_cs',      'Computer Science DAO',     'Governs the Computer Science taxonomy and course quality standards',    'subject_field', 'sf_cs',    'active', 7, 180),
    ('dao_math',    'Mathematics DAO',          'Governs the Mathematics curriculum and proof requirements',             'subject_field', 'sf_math',  'active', 5, 365),
    ('dao_data',    'Data Science DAO',         'Governs Data Science skill standards and model evaluation criteria',    'subject_field', 'sf_data',  'active', 5, 180),
    ('dao_web',     'Web Development DAO',      'Governs web development best practices and technology standards',       'subject_field', 'sf_web',   'active', 5, 180),
    ('dao_cyber',   'Cybersecurity DAO',        'Governs security standards, ethical guidelines, and certification',     'subject_field', 'sf_cyber', 'active', 5, 365),
    ('dao_design',  'Design DAO',               'Governs design quality standards and accessibility requirements',       'subject_field', 'sf_design','active', 5, 365);

-- ============================================================
-- SAMPLE COURSES (author_address is placeholder — will show as "Unknown")
-- ============================================================
INSERT INTO courses (id, title, description, author_address, tags, skill_ids, status) VALUES
    ('course_algo_101',
     'Algorithms & Data Structures 101',
     'A comprehensive introduction to fundamental algorithms and data structures. Covers arrays, linked lists, trees, graphs, sorting, searching, and complexity analysis.',
     'addr_seed_author_1',
     '["algorithms","data-structures","computer-science","fundamentals"]',
     '["skill_big_o","skill_arrays","skill_linked_lists","skill_stacks_queues","skill_trees","skill_sorting","skill_hashing"]',
     'published'),

    ('course_web_fullstack',
     'Full-Stack Web Development with Vue & Rust',
     'Build modern web applications from scratch using Vue 3 on the frontend and Rust on the backend. Covers HTML/CSS, JavaScript, TypeScript, Vue Composition API, REST APIs, and database design.',
     'addr_seed_author_1',
     '["web","vue","rust","fullstack","api"]',
     '["skill_html_css","skill_javascript","skill_typescript","skill_vue","skill_rest_api","skill_db_design","skill_auth"]',
     'published'),

    ('course_ml_foundations',
     'Machine Learning Foundations',
     'From linear regression to neural networks — build a solid ML foundation. Requires basic linear algebra and probability.',
     'addr_seed_author_2',
     '["machine-learning","AI","data-science","neural-networks"]',
     '["skill_regression","skill_supervised","skill_unsupervised","skill_neural_nets","skill_ml_eval"]',
     'published'),

    ('course_crypto_101',
     'Applied Cryptography',
     'Understand the building blocks of modern cryptography: symmetric encryption, public-key systems, hashing, digital signatures, and zero-knowledge proofs.',
     'addr_seed_author_2',
     '["cryptography","security","encryption","blockchain"]',
     '["skill_symmetric","skill_asymmetric","skill_hash_crypto","skill_signatures","skill_zk"]',
     'published'),

    ('course_ux_design',
     'UX Design: Research to Prototype',
     'Learn the full UX design process from user research through wireframing and prototyping. Includes design systems and accessibility.',
     'addr_seed_author_3',
     '["design","UX","UI","accessibility","research"]',
     '["skill_user_research","skill_ia","skill_wireframing","skill_color_theory","skill_typography","skill_design_systems"]',
     'published'),

    ('course_math_discrete',
     'Discrete Mathematics for Computing',
     'A practical discrete math course for software and data workflows: logic, set theory, combinatorics, graph fundamentals, and probability intuition.',
     'addr_seed_author_4',
     '["mathematics","discrete-math","proofs","logic","probability"]',
     '["skill_logic","skill_sets","skill_combinatorics","skill_graph_theory","skill_probability"]',
     'published');

-- ============================================================
-- CHAPTERS (for the algorithms course)
-- ============================================================
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_algo_1', 'course_algo_101', 'Complexity Analysis',         'Big-O notation and asymptotic analysis',                       0),
    ('ch_algo_2', 'course_algo_101', 'Linear Data Structures',     'Arrays, linked lists, stacks, and queues',                      1),
    ('ch_algo_3', 'course_algo_101', 'Trees & Graphs',             'Binary trees, BSTs, graph representations, and traversals',     2),
    ('ch_algo_4', 'course_algo_101', 'Sorting & Searching',        'Comparison sorts, linear-time sorts, and binary search',        3),
    ('ch_algo_5', 'course_algo_101', 'Hash Tables',                'Hash functions, collision resolution, and applications',         4);

-- CHAPTERS (for the web development course)
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_web_1',  'course_web_fullstack', 'HTML & CSS Fundamentals',     'Semantic markup, flexbox, grid, and responsive design',     0),
    ('ch_web_2',  'course_web_fullstack', 'JavaScript Essentials',       'ES6+ features, DOM manipulation, and async patterns',      1),
    ('ch_web_3',  'course_web_fullstack', 'TypeScript Deep Dive',        'Type system, generics, and strict configuration',          2),
    ('ch_web_4',  'course_web_fullstack', 'Vue 3 & Composition API',    'Reactivity, composables, routing, and state management',   3),
    ('ch_web_5',  'course_web_fullstack', 'Backend with Rust',           'REST APIs, database layer, auth, and deployment',          4);

-- CHAPTERS (for the ML course)
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_ml_1',   'course_ml_foundations', 'Regression',                  'Linear and logistic regression from scratch',              0),
    ('ch_ml_2',   'course_ml_foundations', 'Classification & Clustering', 'SVMs, decision trees, k-means, and DBSCAN',               1),
    ('ch_ml_3',   'course_ml_foundations', 'Neural Networks',             'Perceptrons, backpropagation, and deep architectures',     2),
    ('ch_ml_4',   'course_ml_foundations', 'Model Evaluation',            'Cross-validation, metrics, and bias-variance tradeoff',    3);

-- CHAPTERS (for the crypto course)
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_cry_1',  'course_crypto_101', 'Symmetric Cryptography',     'Block ciphers, AES, modes of operation, and stream ciphers',   0),
    ('ch_cry_2',  'course_crypto_101', 'Public-Key Cryptography',    'RSA, Diffie-Hellman, elliptic curves',                         1),
    ('ch_cry_3',  'course_crypto_101', 'Hashing & Signatures',       'SHA-256, BLAKE2, Ed25519, and merkle trees',                   2),
    ('ch_cry_4',  'course_crypto_101', 'Zero-Knowledge Proofs',      'ZK-SNARKs, ZK-STARKs, and privacy applications',               3);

-- CHAPTERS (for the UX design course)
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_ux_1',   'course_ux_design', 'User Research Methods',       'Interviews, surveys, personas, and journey maps',              0),
    ('ch_ux_2',   'course_ux_design', 'Information Architecture',    'Sitemaps, card sorting, and navigation patterns',              1),
    ('ch_ux_3',   'course_ux_design', 'Wireframing & Prototyping',   'From sketches to interactive prototypes',                      2),
    ('ch_ux_4',   'course_ux_design', 'Visual Design Fundamentals',  'Color, typography, spacing, and design tokens',                3);

-- CHAPTERS (for the discrete math course)
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_math_1', 'course_math_discrete', 'Logic & Proof Basics',      'Propositions, implications, predicates, and proof patterns',      0),
    ('ch_math_2', 'course_math_discrete', 'Sets & Combinatorics',      'Set operations, counting principles, permutations, combinations',  1),
    ('ch_math_3', 'course_math_discrete', 'Graph Foundations',          'Graph terminology, traversals, and modeling real systems',         2),
    ('ch_math_4', 'course_math_discrete', 'Probability for Engineers',  'Random variables, expected value, and practical probability',      3);

-- ============================================================
-- ELEMENTS (fair representation of all element types)
-- Types: text, quiz, video, pdf, downloadable, assessment,
--        objective_single_mcq, objective_multi_mcq,
--        subjective_mcq, essay, interactive
-- ============================================================
INSERT INTO course_elements (id, chapter_id, title, element_type, position, duration_seconds) VALUES
    -- Algo course - Chapter 1: Complexity Analysis
    ('el_algo_1_1', 'ch_algo_1', 'What is Big-O?',                       'text',  0, NULL),
    ('el_algo_1_2', 'ch_algo_1', 'Analyzing Loops',                      'text',  1, NULL),
    ('el_algo_1_3', 'ch_algo_1', 'Complexity Quiz',                      'quiz',  2, NULL),
    ('el_algo_1_4', 'ch_algo_1', 'Complexity Lecture Video',              'video', 3, 720),
    -- Algo course - Chapter 2: Linear Data Structures
    ('el_algo_2_1', 'ch_algo_2', 'Array Operations',                     'text',  0, NULL),
    ('el_algo_2_2', 'ch_algo_2', 'Linked List Implementation',           'text',  1, NULL),
    ('el_algo_2_3', 'ch_algo_2', 'Stack & Queue Patterns',               'text',  2, NULL),
    ('el_algo_2_4', 'ch_algo_2', 'Data Structures Quiz',                 'quiz',  3, NULL),
    ('el_algo_2_5', 'ch_algo_2', 'Array vs Linked List Trade-offs',      'objective_single_mcq', 4, NULL),
    -- Algo course - Chapter 3: Trees & Graphs
    ('el_algo_3_1', 'ch_algo_3', 'Binary Trees Explained',               'text',  0, NULL),
    ('el_algo_3_2', 'ch_algo_3', 'Graph Representations',                'text',  1, NULL),
    ('el_algo_3_3', 'ch_algo_3', 'BFS vs DFS',                           'text',  2, NULL),
    ('el_algo_3_4', 'ch_algo_3', 'Trees & Graphs Quiz',                  'quiz',  3, NULL),
    ('el_algo_3_5', 'ch_algo_3', 'Graph Traversal Simulation',           'interactive', 4, NULL),
    -- Algo course - Chapter 4: Sorting & Searching
    ('el_algo_4_1', 'ch_algo_4', 'Bubble Sort & Selection Sort',         'text',  0, NULL),
    ('el_algo_4_2', 'ch_algo_4', 'Merge Sort & Quick Sort',              'text',  1, NULL),
    ('el_algo_4_3', 'ch_algo_4', 'Sorting Quiz',                         'quiz',  2, NULL),
    ('el_algo_4_4', 'ch_algo_4', 'Sorting Algorithm Comparison',         'objective_multi_mcq', 3, NULL),
    -- Algo course - Chapter 5: Hash Tables
    ('el_algo_5_1', 'ch_algo_5', 'Hash Functions',                       'text',  0, NULL),
    ('el_algo_5_2', 'ch_algo_5', 'Collision Resolution',                 'text',  1, NULL),
    ('el_algo_5_3', 'ch_algo_5', 'Hash Tables Quiz',                     'quiz',  2, NULL),
    ('el_algo_5_4', 'ch_algo_5', 'Algorithms Final Assessment',          'assessment', 3, NULL),

    -- Web course - Chapter 1: HTML & CSS Fundamentals
    ('el_web_1_1',  'ch_web_1', 'Semantic HTML',                         'text',  0, NULL),
    ('el_web_1_2',  'ch_web_1', 'Flexbox & Grid',                        'text',  1, NULL),
    ('el_web_1_3',  'ch_web_1', 'CSS Layout Workshop',                   'interactive', 2, NULL),
    ('el_web_1_4',  'ch_web_1', 'HTML & CSS Cheat Sheet',                'pdf',   3, NULL),
    -- Web course - Chapter 2: JavaScript Essentials
    ('el_web_2_1',  'ch_web_2', 'ES6+ Features',                         'text',  0, NULL),
    ('el_web_2_2',  'ch_web_2', 'Async/Await Patterns',                  'text',  1, NULL),
    ('el_web_2_3',  'ch_web_2', 'JavaScript Fundamentals Check',         'objective_single_mcq', 2, NULL),
    ('el_web_2_4',  'ch_web_2', 'Build a Todo App',                      'essay', 3, NULL),
    -- Web course - Chapter 3: TypeScript Deep Dive
    ('el_web_3_1',  'ch_web_3', 'TypeScript Type System',                'text',  0, NULL),
    ('el_web_3_2',  'ch_web_3', 'TypeScript Generics Video',             'video', 1, 540),
    -- Web course - Chapter 4: Vue 3 & Composition API
    ('el_web_4_1',  'ch_web_4', 'Vue Reactivity System',                 'text',  0, NULL),
    ('el_web_4_2',  'ch_web_4', 'Composables Pattern',                   'text',  1, NULL),
    ('el_web_4_3',  'ch_web_4', 'Vue Component Design',                  'subjective_mcq', 2, NULL),
    -- Web course - Chapter 5: Backend with Rust
    ('el_web_5_1',  'ch_web_5', 'Building REST APIs in Rust',            'text',  0, NULL),
    ('el_web_5_2',  'ch_web_5', 'Database Design & Migrations',          'text',  1, NULL),
    ('el_web_5_3',  'ch_web_5', 'Authentication with JWT',               'text',  2, NULL),
    ('el_web_5_4',  'ch_web_5', 'Starter Project Template',              'downloadable', 3, NULL),
    ('el_web_5_5',  'ch_web_5', 'Full-Stack Web Dev Final Assessment',   'assessment', 4, NULL),

    -- ML course - Chapter 1: Regression
    ('el_ml_1_1',   'ch_ml_1', 'Linear Regression from Scratch',         'text',  0, NULL),
    ('el_ml_1_2',   'ch_ml_1', 'Logistic Regression',                    'text',  1, NULL),
    ('el_ml_1_3',   'ch_ml_1', 'Regression Intuition Video',             'video', 2, 600),
    -- ML course - Chapter 2: Classification & Clustering
    ('el_ml_2_1',   'ch_ml_2', 'Decision Trees & Random Forests',        'text',  0, NULL),
    ('el_ml_2_2',   'ch_ml_2', 'K-Means Clustering',                     'text',  1, NULL),
    ('el_ml_2_3',   'ch_ml_2', 'Classify or Cluster?',                   'objective_multi_mcq', 2, NULL),
    ('el_ml_2_4',   'ch_ml_2', 'K-Means Interactive Visualization',      'interactive', 3, NULL),
    -- ML course - Chapter 3: Neural Networks
    ('el_ml_3_1',   'ch_ml_3', 'Neural Network Architecture',            'text',  0, NULL),
    ('el_ml_3_2',   'ch_ml_3', 'Backpropagation',                        'text',  1, NULL),
    ('el_ml_3_3',   'ch_ml_3', 'Design a Neural Network',                'essay', 2, NULL),
    -- ML course - Chapter 4: Model Evaluation
    ('el_ml_4_1',   'ch_ml_4', 'Cross-Validation Techniques',            'text',  0, NULL),
    ('el_ml_4_2',   'ch_ml_4', 'Evaluation Metrics Quiz',                'quiz',  1, NULL),
    ('el_ml_4_3',   'ch_ml_4', 'ML Foundations Final Assessment',        'assessment', 2, NULL),
    ('el_ml_4_4',   'ch_ml_4', 'ML Research Paper Collection',           'pdf',   3, NULL),

    -- Crypto course - Chapter 1: Symmetric Cryptography
    ('el_cry_1_1',  'ch_cry_1', 'Block Ciphers & AES',                   'text',  0, NULL),
    ('el_cry_1_2',  'ch_cry_1', 'AES Encryption Demo Video',             'video', 1, 480),
    ('el_cry_1_3',  'ch_cry_1', 'AES Mode Selection',                    'objective_single_mcq', 2, NULL),
    -- Crypto course - Chapter 2: Public-Key Cryptography
    ('el_cry_2_1',  'ch_cry_2', 'RSA Explained',                         'text',  0, NULL),
    ('el_cry_2_2',  'ch_cry_2', 'Elliptic Curve Cryptography',           'text',  1, NULL),
    ('el_cry_2_3',  'ch_cry_2', 'RSA vs ECC Trade-offs',                 'subjective_mcq', 2, NULL),
    -- Crypto course - Chapter 3: Hashing & Signatures
    ('el_cry_3_1',  'ch_cry_3', 'SHA-256 & BLAKE2',                      'text',  0, NULL),
    ('el_cry_3_2',  'ch_cry_3', 'Digital Signatures with Ed25519',       'text',  1, NULL),
    ('el_cry_3_3',  'ch_cry_3', 'Hash Function Properties',              'objective_multi_mcq', 2, NULL),
    ('el_cry_3_4',  'ch_cry_3', 'Crypto Toolkit Cheat Sheet',            'downloadable', 3, NULL),
    -- Crypto course - Chapter 4: Zero-Knowledge Proofs
    ('el_cry_4_1',  'ch_cry_4', 'Introduction to ZK Proofs',             'text',  0, NULL),
    ('el_cry_4_2',  'ch_cry_4', 'ZK Proof Interactive Demo',             'interactive', 1, NULL),
    ('el_cry_4_3',  'ch_cry_4', 'Cryptography Final Assessment',         'assessment', 2, NULL),

    -- UX course - Chapter 1: User Research Methods
    ('el_ux_1_1',   'ch_ux_1', 'Planning User Interviews',               'text',  0, NULL),
    ('el_ux_1_2',   'ch_ux_1', 'Creating Personas',                      'text',  1, NULL),
    ('el_ux_1_3',   'ch_ux_1', 'User Research Methods Video',            'video', 2, 660),
    ('el_ux_1_4',   'ch_ux_1', 'Research Plan Essay',                    'essay', 3, NULL),
    -- UX course - Chapter 2: Information Architecture
    ('el_ux_2_1',   'ch_ux_2', 'Card Sorting Workshop',                  'text',  0, NULL),
    ('el_ux_2_2',   'ch_ux_2', 'Navigation Patterns',                    'text',  1, NULL),
    ('el_ux_2_3',   'ch_ux_2', 'IA Best Practices',                      'objective_single_mcq', 2, NULL),
    -- UX course - Chapter 3: Wireframing & Prototyping
    ('el_ux_3_1',   'ch_ux_3', 'Low-Fidelity Wireframes',                'text',  0, NULL),
    ('el_ux_3_2',   'ch_ux_3', 'Interactive Prototyping',                 'text',  1, NULL),
    ('el_ux_3_3',   'ch_ux_3', 'Wireframe Templates',                    'downloadable', 2, NULL),
    ('el_ux_3_4',   'ch_ux_3', 'Prototype Fidelity Levels',              'subjective_mcq', 3, NULL),
    -- UX course - Chapter 4: Visual Design Fundamentals
    ('el_ux_4_1',   'ch_ux_4', 'Color Theory for Screens',               'text',  0, NULL),
    ('el_ux_4_2',   'ch_ux_4', 'Typography Best Practices',              'text',  1, NULL),
    ('el_ux_4_3',   'ch_ux_4', 'Design System Reference',                'pdf',   2, NULL),
    ('el_ux_4_4',   'ch_ux_4', 'UX Design Final Assessment',             'assessment', 3, NULL),

    -- Discrete Math course
    ('el_math_1_1', 'ch_math_1', 'Propositional Logic Essentials',       'text',  0, NULL),
    ('el_math_1_2', 'ch_math_1', 'Proof Strategies in Practice',         'text',  1, NULL),
    ('el_math_1_3', 'ch_math_1', 'Logic Foundations Quiz',               'quiz',  2, NULL),
    ('el_math_2_1', 'ch_math_2', 'Set Operations & Counting Rules',      'text',  0, NULL),
    ('el_math_2_2', 'ch_math_2', 'Choosing Counting Techniques',         'objective_single_mcq', 1, NULL),
    ('el_math_3_1', 'ch_math_3', 'Graph Models for Real Systems',        'text',  0, NULL),
    ('el_math_3_2', 'ch_math_3', 'Graph Thinking Interactive',           'interactive', 1, NULL),
    ('el_math_4_1', 'ch_math_4', 'Expected Value & Risk',                'text',  0, NULL),
    ('el_math_4_2', 'ch_math_4', 'Discrete Math Final Assessment',       'assessment', 1, NULL);

-- ============================================================
-- ELEMENT SKILL TAGS (link elements to skills for evidence)
-- ============================================================
INSERT INTO element_skill_tags (element_id, skill_id, weight) VALUES
    -- Algo course
    ('el_algo_1_1', 'skill_big_o',          1.0),
    ('el_algo_1_2', 'skill_big_o',          1.0),
    ('el_algo_1_3', 'skill_big_o',          1.0),
    ('el_algo_1_4', 'skill_big_o',          0.5),
    ('el_algo_2_1', 'skill_arrays',         1.0),
    ('el_algo_2_2', 'skill_linked_lists',   1.0),
    ('el_algo_2_3', 'skill_stacks_queues',  1.0),
    ('el_algo_2_4', 'skill_arrays',         0.5),
    ('el_algo_2_4', 'skill_linked_lists',   0.5),
    ('el_algo_2_4', 'skill_stacks_queues',  0.5),
    ('el_algo_2_5', 'skill_arrays',         1.0),
    ('el_algo_2_5', 'skill_linked_lists',   0.5),
    ('el_algo_3_1', 'skill_trees',          1.0),
    ('el_algo_3_2', 'skill_graphs',         1.0),
    ('el_algo_3_3', 'skill_graphs',         1.0),
    ('el_algo_3_4', 'skill_trees',          0.5),
    ('el_algo_3_4', 'skill_graphs',         0.5),
    ('el_algo_3_5', 'skill_graphs',         1.0),
    ('el_algo_3_5', 'skill_trees',          0.5),
    ('el_algo_4_1', 'skill_sorting',        1.0),
    ('el_algo_4_2', 'skill_sorting',        1.0),
    ('el_algo_4_3', 'skill_sorting',        1.0),
    ('el_algo_4_4', 'skill_sorting',        1.0),
    ('el_algo_5_1', 'skill_hashing',        1.0),
    ('el_algo_5_2', 'skill_hashing',        1.0),
    ('el_algo_5_3', 'skill_hashing',        1.0),
    ('el_algo_5_4', 'skill_big_o',          0.5),
    ('el_algo_5_4', 'skill_arrays',         0.5),
    ('el_algo_5_4', 'skill_trees',          0.5),
    ('el_algo_5_4', 'skill_sorting',        0.5),
    ('el_algo_5_4', 'skill_hashing',        0.5),

    -- Web course
    ('el_web_1_1',  'skill_html_css',       1.0),
    ('el_web_1_2',  'skill_html_css',       1.0),
    ('el_web_1_3',  'skill_html_css',       1.0),
    ('el_web_1_4',  'skill_html_css',       0.5),
    ('el_web_2_1',  'skill_javascript',     1.0),
    ('el_web_2_2',  'skill_javascript',     1.0),
    ('el_web_2_3',  'skill_javascript',     1.0),
    ('el_web_2_4',  'skill_javascript',     1.0),
    ('el_web_2_4',  'skill_html_css',       0.5),
    ('el_web_3_1',  'skill_typescript',     1.0),
    ('el_web_3_2',  'skill_typescript',     1.0),
    ('el_web_4_1',  'skill_vue',            1.0),
    ('el_web_4_2',  'skill_vue',            1.0),
    ('el_web_4_3',  'skill_vue',            1.0),
    ('el_web_5_1',  'skill_rest_api',       1.0),
    ('el_web_5_1',  'skill_rust',           0.5),
    ('el_web_5_2',  'skill_db_design',      1.0),
    ('el_web_5_3',  'skill_auth',           1.0),
    ('el_web_5_4',  'skill_rest_api',       0.5),
    ('el_web_5_4',  'skill_rust',           0.5),
    ('el_web_5_5',  'skill_html_css',       0.5),
    ('el_web_5_5',  'skill_javascript',     0.5),
    ('el_web_5_5',  'skill_vue',            0.5),
    ('el_web_5_5',  'skill_rest_api',       0.5),
    ('el_web_5_5',  'skill_db_design',      0.5),

    -- ML course
    ('el_ml_1_1',   'skill_regression',     1.0),
    ('el_ml_1_2',   'skill_regression',     1.0),
    ('el_ml_1_3',   'skill_regression',     0.5),
    ('el_ml_2_1',   'skill_supervised',     1.0),
    ('el_ml_2_2',   'skill_unsupervised',   1.0),
    ('el_ml_2_3',   'skill_supervised',     0.5),
    ('el_ml_2_3',   'skill_unsupervised',   0.5),
    ('el_ml_2_4',   'skill_unsupervised',   1.0),
    ('el_ml_3_1',   'skill_neural_nets',    1.0),
    ('el_ml_3_2',   'skill_neural_nets',    1.0),
    ('el_ml_3_3',   'skill_neural_nets',    1.0),
    ('el_ml_3_3',   'skill_deep_learning',  0.5),
    ('el_ml_4_1',   'skill_ml_eval',        1.0),
    ('el_ml_4_2',   'skill_ml_eval',        1.0),
    ('el_ml_4_3',   'skill_regression',     0.5),
    ('el_ml_4_3',   'skill_supervised',     0.5),
    ('el_ml_4_3',   'skill_neural_nets',    0.5),
    ('el_ml_4_3',   'skill_ml_eval',        0.5),
    ('el_ml_4_4',   'skill_ml_eval',        0.5),

    -- Crypto course
    ('el_cry_1_1',  'skill_symmetric',      1.0),
    ('el_cry_1_2',  'skill_symmetric',      0.5),
    ('el_cry_1_3',  'skill_symmetric',      1.0),
    ('el_cry_2_1',  'skill_asymmetric',     1.0),
    ('el_cry_2_2',  'skill_asymmetric',     1.0),
    ('el_cry_2_3',  'skill_asymmetric',     1.0),
    ('el_cry_2_3',  'skill_symmetric',      0.5),
    ('el_cry_3_1',  'skill_hash_crypto',    1.0),
    ('el_cry_3_2',  'skill_signatures',     1.0),
    ('el_cry_3_3',  'skill_hash_crypto',    1.0),
    ('el_cry_3_4',  'skill_symmetric',      0.5),
    ('el_cry_3_4',  'skill_asymmetric',     0.5),
    ('el_cry_3_4',  'skill_hash_crypto',    0.5),
    ('el_cry_4_1',  'skill_zk',            1.0),
    ('el_cry_4_2',  'skill_zk',            1.0),
    ('el_cry_4_3',  'skill_symmetric',      0.5),
    ('el_cry_4_3',  'skill_asymmetric',     0.5),
    ('el_cry_4_3',  'skill_hash_crypto',    0.5),
    ('el_cry_4_3',  'skill_signatures',     0.5),
    ('el_cry_4_3',  'skill_zk',            0.5),

    -- UX course
    ('el_ux_1_1',   'skill_user_research',  1.0),
    ('el_ux_1_2',   'skill_user_research',  1.0),
    ('el_ux_1_3',   'skill_user_research',  0.5),
    ('el_ux_1_4',   'skill_user_research',  1.0),
    ('el_ux_2_1',   'skill_ia',            1.0),
    ('el_ux_2_2',   'skill_ia',            1.0),
    ('el_ux_2_3',   'skill_ia',            1.0),
    ('el_ux_3_1',   'skill_wireframing',    1.0),
    ('el_ux_3_2',   'skill_wireframing',    1.0),
    ('el_ux_3_3',   'skill_wireframing',    0.5),
    ('el_ux_3_4',   'skill_wireframing',    1.0),
    ('el_ux_4_1',   'skill_color_theory',   1.0),
    ('el_ux_4_2',   'skill_typography',     1.0),
    ('el_ux_4_3',   'skill_design_systems', 1.0),
    ('el_ux_4_3',   'skill_color_theory',   0.5),
    ('el_ux_4_3',   'skill_typography',     0.5),
    ('el_ux_4_4',   'skill_user_research',  0.5),
    ('el_ux_4_4',   'skill_ia',            0.5),
    ('el_ux_4_4',   'skill_wireframing',    0.5),
    ('el_ux_4_4',   'skill_color_theory',   0.5),
    ('el_ux_4_4',   'skill_typography',     0.5),

    -- Discrete Math course
    ('el_math_1_1', 'skill_logic',          1.0),
    ('el_math_1_2', 'skill_logic',          1.0),
    ('el_math_1_3', 'skill_logic',          1.0),
    ('el_math_2_1', 'skill_sets',           1.0),
    ('el_math_2_1', 'skill_combinatorics',  0.5),
    ('el_math_2_2', 'skill_combinatorics',  1.0),
    ('el_math_3_1', 'skill_graph_theory',   1.0),
    ('el_math_3_2', 'skill_graph_theory',   1.0),
    ('el_math_4_1', 'skill_probability',    1.0),
    ('el_math_4_2', 'skill_logic',          0.5),
    ('el_math_4_2', 'skill_sets',           0.5),
    ('el_math_4_2', 'skill_combinatorics',  0.5),
    ('el_math_4_2', 'skill_graph_theory',   0.5),
    ('el_math_4_2', 'skill_probability',    0.5);

-- ============================================================
-- Post-migration 040: SkillProof seed rows retired. The earned/
-- available/locked demo states that used these rows now land via
-- seeded Verifiable Credentials once the VC auto-issuance pipeline
-- lands; until then the demo graph shows everything without prereqs
-- as 'available' and the rest as 'locked'.
-- ============================================================

-- ============================================================
-- CIVIC ENGAGEMENT — subject field, subjects, skills, course
-- Framed for learners in the Global South / developing democracies.
-- All civics content is AI-generated example material.
-- ============================================================
INSERT INTO subject_fields (id, name, description) VALUES
    ('sf_civics', 'Civic Engagement', 'Constitutional literacy, rights, voting systems, public accountability, and civic participation — framed for learners in the Global South and emerging democracies');

INSERT INTO subjects (id, name, description, subject_field_id) VALUES
    ('sub_democratic_systems',   'Democratic Systems',        'Constitutions, separation of powers, federalism vs centralism, and how democracies are organised in practice', 'sf_civics'),
    ('sub_rights_governance',    'Rights & Governance',       'Civil and political rights, press freedom, judicial independence, and checks on state power',                 'sf_civics'),
    ('sub_global_citizenship',   'Global Citizenship',        'Post-colonial legacies, human rights frameworks, and the international order',                                'sf_civics'),
    ('sub_civic_participation',  'Civic Participation',       'Voting, community organising, public finance literacy, media literacy, and civic technology',                 'sf_civics');

INSERT INTO skills (id, name, description, subject_id, bloom_level) VALUES
    ('skill_constitutional_literacy',  'Constitutional Literacy',         'Read a national constitution and identify who holds which power',                    'sub_democratic_systems',  'understand'),
    ('skill_separation_of_powers',     'Separation of Powers',            'Distinguish executive, legislative, and judicial functions in a real system',         'sub_democratic_systems',  'analyze'),
    ('skill_federalism_vs_centralism', 'Federalism vs Centralism',        'Compare federal and unitary state structures and their trade-offs',                  'sub_democratic_systems',  'analyze'),
    ('skill_civil_rights_frameworks',  'Civil & Political Rights',        'Apply national bills of rights and international covenants to concrete situations',  'sub_rights_governance',   'apply'),
    ('skill_press_freedom',            'Press Freedom',                   'Evaluate legal and practical threats to an independent press',                       'sub_rights_governance',   'evaluate'),
    ('skill_judicial_independence',    'Judicial Independence',           'Assess appointment, tenure, and funding structures that protect (or erode) courts',  'sub_rights_governance',   'evaluate'),
    ('skill_human_rights_advocacy',    'Human Rights Advocacy',           'Use UN, regional, and NGO instruments to advocate for human rights',                  'sub_global_citizenship',  'apply'),
    ('skill_colonial_legacy_analysis', 'Colonial Legacy Analysis',        'Analyse how colonial institutions shape modern borders, law, and economies',         'sub_global_citizenship',  'analyze'),
    ('skill_voting_systems',           'Voting Systems',                  'Compare plurality, proportional, and ranked-choice electoral systems',                'sub_civic_participation', 'apply'),
    ('skill_election_integrity',       'Election Integrity',              'Identify and document irregularities in election administration',                     'sub_civic_participation', 'evaluate'),
    ('skill_public_accountability',    'Public Accountability',           'Use audit reports, RTI requests, and oversight bodies to hold officials accountable', 'sub_civic_participation', 'apply'),
    ('skill_community_organizing',     'Community Organising',            'Plan and run local civic campaigns on a small budget',                                'sub_civic_participation', 'create'),
    ('skill_media_literacy_political', 'Political Media Literacy',        'Distinguish reporting, opinion, propaganda, and manufactured consensus',              'sub_civic_participation', 'evaluate'),
    ('skill_public_finance_literacy',  'Public Finance Literacy',         'Read a national or municipal budget and track where tax money goes',                  'sub_civic_participation', 'understand'),
    ('skill_anti_corruption_mechanisms','Anti-Corruption Mechanisms',     'Design and evaluate transparency mechanisms that reduce corruption',                  'sub_civic_participation', 'analyze'),
    ('skill_civic_tech_and_data',      'Civic Tech & Open Data',          'Use open data and civic-tech tools for accountability and public interest research',  'sub_civic_participation', 'apply');

INSERT INTO skill_prerequisites (skill_id, prerequisite_id) VALUES
    ('skill_separation_of_powers',     'skill_constitutional_literacy'),
    ('skill_federalism_vs_centralism', 'skill_constitutional_literacy'),
    ('skill_judicial_independence',    'skill_separation_of_powers'),
    ('skill_press_freedom',            'skill_civil_rights_frameworks'),
    ('skill_civil_rights_frameworks',  'skill_constitutional_literacy'),
    ('skill_election_integrity',       'skill_voting_systems'),
    ('skill_public_accountability',    'skill_public_finance_literacy'),
    ('skill_anti_corruption_mechanisms','skill_public_accountability'),
    ('skill_community_organizing',     'skill_voting_systems'),
    ('skill_human_rights_advocacy',    'skill_civil_rights_frameworks'),
    ('skill_colonial_legacy_analysis', 'skill_constitutional_literacy');

INSERT INTO skill_relations (skill_id, related_skill_id, relation_type) VALUES
    ('skill_civic_tech_and_data',      'skill_sql',                'complementary'),
    ('skill_civic_tech_and_data',      'skill_accessibility',      'related'),
    ('skill_media_literacy_political', 'skill_user_research',      'related'),
    ('skill_public_accountability',    'skill_logic',              'complementary'),
    ('skill_election_integrity',       'skill_hash_crypto',        'related'),
    ('skill_anti_corruption_mechanisms','skill_db_design',         'related');

-- Civics DAO
INSERT INTO governance_daos (id, name, description, scope_type, scope_id, status, committee_size, election_interval_days) VALUES
    ('dao_civics', 'Civic Engagement DAO', 'Governs the Civic Engagement taxonomy and example-content standards', 'subject_field', 'sf_civics', 'active', 5, 365);

-- Civic Sense course
INSERT INTO courses (id, title, description, author_address, tags, skill_ids, status) VALUES
    ('course_civics_101',
     'Civic Sense for the Global South',
     'A practical civics course framed for learners in the Global South and emerging democracies. Covers constitutions, rights, voting systems, public accountability, and grassroots participation — with examples drawn from post-colonial contexts.',
     'addr_seed_author_5',
     '["civics","democracy","global-south","rights","accountability","post-colonial"]',
     '["skill_constitutional_literacy","skill_separation_of_powers","skill_civil_rights_frameworks","skill_press_freedom","skill_voting_systems","skill_election_integrity","skill_public_accountability","skill_community_organizing","skill_public_finance_literacy","skill_colonial_legacy_analysis"]',
     'published');

-- Chapters
INSERT INTO course_chapters (id, course_id, title, description, position) VALUES
    ('ch_civ_1', 'course_civics_101', 'The Shape of Democracy',            'Constitutions, separation of powers, federalism, and how democracies are organised',         0),
    ('ch_civ_2', 'course_civics_101', 'Rights You Actually Have',          'Civil and political rights, press freedom, and judicial independence',                        1),
    ('ch_civ_3', 'course_civics_101', 'Voting and Elections',              'Voting systems, election integrity, and public accountability',                              2),
    ('ch_civ_4', 'course_civics_101', 'Beyond the Ballot',                 'Community organising, media literacy, and civic technology',                                  3),
    ('ch_civ_5', 'course_civics_101', 'Power, Money, and Accountability',  'Public finance literacy, anti-corruption mechanisms, and the colonial legacy',                4);

-- Elements (22 across 5 chapters)
INSERT INTO course_elements (id, chapter_id, title, element_type, position, duration_seconds) VALUES
    -- Chapter 1: The Shape of Democracy
    ('el_civ_1_1', 'ch_civ_1', 'What a Constitution Actually Does',     'text',  0, NULL),
    ('el_civ_1_2', 'ch_civ_1', 'Separation of Powers in Practice',       'text',  1, NULL),
    ('el_civ_1_3', 'ch_civ_1', 'Federal vs Unitary States',              'text',  2, NULL),
    ('el_civ_1_4', 'ch_civ_1', 'Constitutions Quiz',                     'quiz',  3, NULL),
    ('el_civ_1_5', 'ch_civ_1', 'Reading a National Constitution',        'video', 4, 720),
    -- Chapter 2: Rights You Actually Have
    ('el_civ_2_1', 'ch_civ_2', 'Civil and Political Rights',             'text',  0, NULL),
    ('el_civ_2_2', 'ch_civ_2', 'Press Freedom Under Pressure',           'text',  1, NULL),
    ('el_civ_2_3', 'ch_civ_2', 'Judicial Independence',                  'text',  2, NULL),
    ('el_civ_2_4', 'ch_civ_2', 'Rights Frameworks Quiz',                 'quiz',  3, NULL),
    ('el_civ_2_5', 'ch_civ_2', 'Universal Declaration of Human Rights',  'pdf',   4, NULL),
    -- Chapter 3: Voting and Elections
    ('el_civ_3_1', 'ch_civ_3', 'How Voting Systems Differ',              'text',  0, NULL),
    ('el_civ_3_2', 'ch_civ_3', 'Election Integrity: Red Flags',          'text',  1, NULL),
    ('el_civ_3_3', 'ch_civ_3', 'Voting Systems Quiz',                    'quiz',  2, NULL),
    ('el_civ_3_4', 'ch_civ_3', 'Voting Systems Comparison',              'interactive', 3, NULL),
    -- Chapter 4: Beyond the Ballot
    ('el_civ_4_1', 'ch_civ_4', 'Community Organising on a Small Budget', 'text',  0, NULL),
    ('el_civ_4_2', 'ch_civ_4', 'Political Media Literacy',               'text',  1, NULL),
    ('el_civ_4_3', 'ch_civ_4', 'Civic Tech in Practice',                 'text',  2, NULL),
    ('el_civ_4_4', 'ch_civ_4', 'Designing a Civic Campaign',             'essay', 3, NULL),
    -- Chapter 5: Power, Money, and Accountability
    ('el_civ_5_1', 'ch_civ_5', 'How to Read a National Budget',          'text',  0, NULL),
    ('el_civ_5_2', 'ch_civ_5', 'Anti-Corruption Mechanisms',             'text',  1, NULL),
    ('el_civ_5_3', 'ch_civ_5', 'The Colonial Legacy',                    'text',  2, NULL),
    ('el_civ_5_4', 'ch_civ_5', 'Civic Sense Final Assessment',           'assessment', 3, NULL);

-- Element skill tags
INSERT INTO element_skill_tags (element_id, skill_id, weight) VALUES
    ('el_civ_1_1', 'skill_constitutional_literacy',  1.0),
    ('el_civ_1_2', 'skill_separation_of_powers',     1.0),
    ('el_civ_1_3', 'skill_federalism_vs_centralism', 1.0),
    ('el_civ_1_4', 'skill_constitutional_literacy',  0.5),
    ('el_civ_1_4', 'skill_separation_of_powers',     0.5),
    ('el_civ_1_4', 'skill_federalism_vs_centralism', 0.5),
    ('el_civ_1_5', 'skill_constitutional_literacy',  0.5),
    ('el_civ_2_1', 'skill_civil_rights_frameworks',  1.0),
    ('el_civ_2_2', 'skill_press_freedom',            1.0),
    ('el_civ_2_3', 'skill_judicial_independence',    1.0),
    ('el_civ_2_4', 'skill_civil_rights_frameworks',  0.5),
    ('el_civ_2_4', 'skill_press_freedom',            0.5),
    ('el_civ_2_4', 'skill_judicial_independence',    0.5),
    ('el_civ_2_5', 'skill_human_rights_advocacy',    0.5),
    ('el_civ_3_1', 'skill_voting_systems',           1.0),
    ('el_civ_3_2', 'skill_election_integrity',       1.0),
    ('el_civ_3_3', 'skill_voting_systems',           0.5),
    ('el_civ_3_3', 'skill_election_integrity',       0.5),
    ('el_civ_3_4', 'skill_voting_systems',           1.0),
    ('el_civ_4_1', 'skill_community_organizing',     1.0),
    ('el_civ_4_2', 'skill_media_literacy_political', 1.0),
    ('el_civ_4_3', 'skill_civic_tech_and_data',      1.0),
    ('el_civ_4_4', 'skill_community_organizing',     1.0),
    ('el_civ_4_4', 'skill_civic_tech_and_data',      0.5),
    ('el_civ_5_1', 'skill_public_finance_literacy',  1.0),
    ('el_civ_5_2', 'skill_anti_corruption_mechanisms', 1.0),
    ('el_civ_5_2', 'skill_public_accountability',    0.5),
    ('el_civ_5_3', 'skill_colonial_legacy_analysis', 1.0),
    ('el_civ_5_4', 'skill_constitutional_literacy',  0.5),
    ('el_civ_5_4', 'skill_civil_rights_frameworks',  0.5),
    ('el_civ_5_4', 'skill_voting_systems',           0.5),
    ('el_civ_5_4', 'skill_public_accountability',    0.5),
    ('el_civ_5_4', 'skill_colonial_legacy_analysis', 0.5);

"##;

const BACKFILL_SQL: &str = r##"
-- Temporarily disable FK checks during bulk seed insert
PRAGMA foreign_keys = OFF;

-- ============================================================
-- P1: ENROLLMENTS & PROGRESS
-- ============================================================
-- 4 enrollments: 1 completed, 2 active (in-progress), 1 recently started
INSERT INTO enrollments (id, course_id, enrolled_at, completed_at, status) VALUES
    ('enroll_algo',   'course_algo_101',      '2026-01-15T10:00:00', '2026-03-20T16:45:00', 'completed'),
    ('enroll_web',    'course_web_fullstack', '2026-02-01T09:30:00', NULL,                   'active'),
    ('enroll_ml',     'course_ml_foundations','2026-03-10T14:00:00', NULL,                   'active'),
    ('enroll_crypto', 'course_crypto_101',        '2026-04-01T11:15:00', NULL,                   'active');

-- Algo 101: fully completed (all elements done)
INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent, completed_at) VALUES
    ('ep_a1_1', 'enroll_algo', 'el_algo_1_1', 'completed', NULL, 420,  '2026-01-16T11:00:00'),
    ('ep_a1_2', 'enroll_algo', 'el_algo_1_2', 'completed', NULL, 1200, '2026-01-18T14:30:00'),
    ('ep_a1_3', 'enroll_algo', 'el_algo_1_3', 'completed', 0.92, 900,  '2026-01-20T10:15:00'),
    ('ep_a2_1', 'enroll_algo', 'el_algo_2_1', 'completed', NULL, 480,  '2026-01-22T09:00:00'),
    ('ep_a2_2', 'enroll_algo', 'el_algo_2_2', 'completed', NULL, 1500, '2026-01-25T16:00:00'),
    ('ep_a2_3', 'enroll_algo', 'el_algo_2_3', 'completed', 0.88, 1080, '2026-01-28T11:30:00'),
    ('ep_a3_1', 'enroll_algo', 'el_algo_3_1', 'completed', NULL, 600,  '2026-02-01T10:00:00'),
    ('ep_a3_2', 'enroll_algo', 'el_algo_3_2', 'completed', NULL, 1800, '2026-02-05T15:45:00'),
    ('ep_a3_3', 'enroll_algo', 'el_algo_3_3', 'completed', 0.95, 720,  '2026-02-08T10:30:00'),
    ('ep_a4_1', 'enroll_algo', 'el_algo_4_1', 'completed', NULL, 360,  '2026-02-10T09:15:00'),
    ('ep_a4_2', 'enroll_algo', 'el_algo_4_2', 'completed', NULL, 2400, '2026-02-15T14:00:00'),
    ('ep_a4_3', 'enroll_algo', 'el_algo_4_3', 'completed', 0.90, 1200, '2026-03-20T16:45:00');

-- Web fullstack: 60% through (chapters 1-3 done, partway through 4)
INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent, completed_at) VALUES
    ('ep_w1_1', 'enroll_web', 'el_web_1_1', 'completed', NULL, 300,  '2026-02-02T10:00:00'),
    ('ep_w1_2', 'enroll_web', 'el_web_1_2', 'completed', NULL, 900,  '2026-02-04T11:30:00'),
    ('ep_w1_3', 'enroll_web', 'el_web_1_3', 'completed', 0.96, 600,  '2026-02-06T09:45:00'),
    ('ep_w2_1', 'enroll_web', 'el_web_2_1', 'completed', NULL, 480,  '2026-02-08T14:00:00'),
    ('ep_w2_2', 'enroll_web', 'el_web_2_2', 'completed', NULL, 1200, '2026-02-11T10:30:00'),
    ('ep_w2_3', 'enroll_web', 'el_web_2_3', 'completed', 0.84, 900,  '2026-02-14T16:00:00'),
    ('ep_w3_1', 'enroll_web', 'el_web_3_1', 'completed', NULL, 600,  '2026-02-16T09:00:00'),
    ('ep_w3_2', 'enroll_web', 'el_web_3_2', 'completed', NULL, 1500, '2026-02-20T13:15:00'),
    ('ep_w3_3', 'enroll_web', 'el_web_3_3', 'completed', 0.91, 1080, '2026-02-24T11:00:00'),
    ('ep_w4_1', 'enroll_web', 'el_web_4_1', 'completed', NULL, 540,  '2026-03-01T10:00:00'),
    ('ep_w4_2', 'enroll_web', 'el_web_4_2', 'in_progress', NULL, 600, NULL);

-- ML foundations: just started (chapter 1 done)
INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent, completed_at) VALUES
    ('ep_m1_1', 'enroll_ml', 'el_ml_1_1', 'completed',   NULL, 360, '2026-03-12T10:00:00'),
    ('ep_m1_2', 'enroll_ml', 'el_ml_1_2', 'completed',   NULL, 900, '2026-03-14T14:30:00'),
    ('ep_m1_3', 'enroll_ml', 'el_ml_1_3', 'completed',   0.87, 720, '2026-03-16T11:00:00'),
    ('ep_m2_1', 'enroll_ml', 'el_ml_2_1', 'in_progress', NULL, 180, NULL);

-- Crypto: barely started
INSERT INTO element_progress (id, enrollment_id, element_id, status, score, time_spent, completed_at) VALUES
    ('ep_c1_1', 'enroll_crypto', 'el_cry_1_1', 'completed',   NULL, 480, '2026-04-02T10:30:00'),
    ('ep_c1_2', 'enroll_crypto', 'el_cry_1_2', 'in_progress', NULL, 120, NULL);

-- Course notes
INSERT INTO course_notes (id, enrollment_id, chapter_id, element_id, preview_text) VALUES
    ('note_001', 'enroll_algo', 'ch_algo_1', 'el_algo_1_2', 'Key insight: amortized O(1) for dynamic arrays because doubling only happens log(n) times. Think of it like paying a little extra each insertion to cover the rare expensive resize.'),
    ('note_002', 'enroll_web',  'ch_web_2',  'el_web_2_2',  'Vue 3 Composition API vs Options API: use composables for shared stateful logic. defineProps + defineEmits for type-safe component contracts. Remember: ref() for primitives, reactive() for objects.'),
    ('note_003', 'enroll_ml',   'ch_ml_1',   'el_ml_1_2',   'Bias-variance tradeoff: high bias = underfitting (model too simple), high variance = overfitting (model too complex). Cross-validation is the practical tool to detect both.');

-- ============================================================
-- P2: ASSESSMENTS, EVIDENCE RECORDS, & PROOF LINKS
--
-- Post-migration 040: all three legacy tables (skill_assessments,
-- evidence_records, skill_proof_evidence) are dropped. Leaving the
-- seeds out here keeps `cargo test --package alexandria-node`
-- runs from crashing on INSERT into a non-existent table; demo data
-- for the VC-first world gets reintroduced with the auto-issuance
-- pipeline.
-- ============================================================

-- (SkillProof / evidence_record / skill_proof_evidence seed blocks
--  removed with migration 040.)

-- ============================================================
-- P3: REPUTATION ASSERTIONS & IMPACT DATA
-- ============================================================
-- 3 instructors with reputation across different domains
INSERT INTO reputation_assertions (id, actor_address, role, skill_id, proficiency_level, score, evidence_count, median_impact, impact_p25, impact_p75, learner_count, impact_variance, window_start, window_end, computation_spec) VALUES
    -- Author 1: Algorithms & Web instructor
    ('rep_001', 'addr_seed_author_1', 'instructor', 'skill_arrays',    'apply',    0.91, 12, 0.08, 0.05, 0.12, 8, 0.003, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_002', 'addr_seed_author_1', 'instructor', 'skill_big_o',     'analyze',  0.87, 9,  0.07, 0.04, 0.10, 6, 0.004, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_003', 'addr_seed_author_1', 'instructor', 'skill_html_css',  'apply',    0.93, 15, 0.09, 0.06, 0.13, 10, 0.002, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_004', 'addr_seed_author_1', 'instructor', 'skill_javascript','apply',    0.89, 11, 0.07, 0.04, 0.11, 7, 0.003, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    -- Author 2: Data & Crypto instructor
    ('rep_005', 'addr_seed_author_2', 'instructor', 'skill_sql',       'apply',    0.85, 8,  0.06, 0.03, 0.09, 5, 0.005, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_006', 'addr_seed_author_2', 'instructor', 'skill_symmetric', 'apply',    0.82, 6,  0.05, 0.02, 0.08, 4, 0.006, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_007', 'addr_seed_author_2', 'instructor', 'skill_supervised','apply',    0.88, 10, 0.08, 0.05, 0.11, 7, 0.004, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    -- Author 3: Design instructor
    ('rep_008', 'addr_seed_author_3', 'instructor', 'skill_user_research', 'evaluate', 0.86, 7, 0.06, 0.03, 0.10, 5, 0.005, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_009', 'addr_seed_author_3', 'instructor', 'skill_ia',            'create',   0.84, 6, 0.05, 0.03, 0.08, 4, 0.004, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_010', 'addr_seed_author_3', 'instructor', 'skill_wireframing',   'create',   0.83, 5, 0.05, 0.02, 0.07, 4, 0.006, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2');

-- (reputation_evidence / reputation_impact_deltas seed blocks
--  removed with migration 040; both tables were dropped and will be
--  reintroduced — repointed at credentials — in a follow-up session.)

-- ============================================================
-- P4: GOVERNANCE (members, elections, proposals, votes)
-- ============================================================
-- DAO members (committee members for each DAO)
INSERT INTO governance_dao_members (dao_id, stake_address, role) VALUES
    -- CS DAO: 7 members
    ('dao_cs', 'addr_seed_author_1',   'chair'),
    ('dao_cs', 'addr_seed_author_2',   'committee'),
    ('dao_cs', 'addr_seed_member_1',   'committee'),
    ('dao_cs', 'addr_seed_member_2',   'committee'),
    ('dao_cs', 'addr_seed_member_3',   'member'),
    ('dao_cs', 'addr_seed_member_4',   'member'),
    ('dao_cs', 'addr_seed_member_5',   'member'),
    -- Math DAO: 5 members
    ('dao_math', 'addr_seed_author_2', 'chair'),
    ('dao_math', 'addr_seed_member_1', 'committee'),
    ('dao_math', 'addr_seed_member_6', 'committee'),
    ('dao_math', 'addr_seed_member_7', 'member'),
    ('dao_math', 'addr_seed_member_8', 'member'),
    -- Web DAO: 5 members
    ('dao_web', 'addr_seed_author_1',  'chair'),
    ('dao_web', 'addr_seed_member_2',  'committee'),
    ('dao_web', 'addr_seed_member_9',  'committee'),
    ('dao_web', 'addr_seed_member_10', 'member'),
    ('dao_web', 'addr_seed_member_11', 'member'),
    -- Design DAO: 5 members
    ('dao_design', 'addr_seed_author_3', 'chair'),
    ('dao_design', 'addr_seed_member_3',  'committee'),
    ('dao_design', 'addr_seed_member_12', 'committee'),
    ('dao_design', 'addr_seed_member_13', 'member'),
    ('dao_design', 'addr_seed_member_14', 'member'),
    -- Cyber DAO: 5 members
    ('dao_cyber', 'addr_seed_author_2', 'chair'),
    ('dao_cyber', 'addr_seed_member_4', 'committee'),
    ('dao_cyber', 'addr_seed_member_15', 'member'),
    ('dao_cyber', 'addr_seed_member_16', 'member'),
    ('dao_cyber', 'addr_seed_member_17', 'member'),
    -- Data DAO: 5 members
    ('dao_data', 'addr_seed_author_2', 'chair'),
    ('dao_data', 'addr_seed_member_5', 'committee'),
    ('dao_data', 'addr_seed_member_6', 'committee'),
    ('dao_data', 'addr_seed_member_18', 'member'),
    ('dao_data', 'addr_seed_member_19', 'member');

-- Elections: 1 finalized, 1 in voting phase, 1 in nomination phase
INSERT INTO governance_elections (id, dao_id, title, description, phase, seats, nominee_min_proficiency, voter_min_proficiency, nomination_start, nomination_end, voting_end, finalized_at) VALUES
    ('election_001', 'dao_cs', 'Q1 2026 CS Committee Election', 'Annual election for the Computer Science DAO committee seats', 'finalized', 5, 'apply', 'remember', '2025-12-01T00:00:00', '2025-12-15T00:00:00', '2025-12-31T00:00:00', '2026-01-02T00:00:00'),
    ('election_002', 'dao_web', 'Q2 2026 Web Dev Committee Election', 'Election for Web Development DAO committee seats', 'voting', 5, 'apply', 'remember', '2026-03-01T00:00:00', '2026-03-15T00:00:00', '2026-04-15T00:00:00', NULL),
    ('election_003', 'dao_design', 'Q2 2026 Design Committee Election', 'Election for Design DAO committee seats', 'nomination', 5, 'apply', 'remember', '2026-04-01T00:00:00', '2026-04-30T00:00:00', NULL, NULL);

-- Election nominees
INSERT INTO governance_election_nominees (id, election_id, stake_address, accepted, votes_received, is_winner) VALUES
    -- Finalized CS election: 4 nominees, 3 won
    ('nom_001', 'election_001', 'addr_seed_author_1',  1, 12, 1),
    ('nom_002', 'election_001', 'addr_seed_author_2',  1, 9,  1),
    ('nom_003', 'election_001', 'addr_seed_member_1',  1, 8,  1),
    ('nom_004', 'election_001', 'addr_seed_member_2',  1, 4,  0),
    -- Active Web election: 3 nominees, voting in progress
    ('nom_005', 'election_002', 'addr_seed_author_1',  1, 6, 0),
    ('nom_006', 'election_002', 'addr_seed_member_2',  1, 4, 0),
    ('nom_007', 'election_002', 'addr_seed_member_9',  1, 3, 0),
    -- Design nomination: 2 nominees so far
    ('nom_008', 'election_003', 'addr_seed_author_3',  1, 0, 0),
    ('nom_009', 'election_003', 'addr_seed_member_12', 0, 0, 0);

-- Election votes (for finalized + active elections)
INSERT INTO governance_election_votes (id, election_id, voter, nominee_id) VALUES
    ('evote_001', 'election_001', 'addr_seed_member_3', 'nom_001'),
    ('evote_002', 'election_001', 'addr_seed_member_4', 'nom_001'),
    ('evote_003', 'election_001', 'addr_seed_member_5', 'nom_002'),
    ('evote_004', 'election_001', 'addr_seed_member_6', 'nom_003'),
    ('evote_005', 'election_002', 'addr_seed_member_10', 'nom_005'),
    ('evote_006', 'election_002', 'addr_seed_member_11', 'nom_005'),
    ('evote_007', 'election_002', 'addr_seed_member_3',  'nom_006');

-- Proposals: varied states across DAOs
INSERT INTO governance_proposals (id, dao_id, title, description, category, status, proposer, votes_for, votes_against, voting_deadline, min_vote_proficiency) VALUES
    ('prop_001', 'dao_cs', 'Add Quantum Computing subject', 'Proposal to add Quantum Computing as a new subject under Computer Science, with skills for quantum gates, Shor/Grover algorithms, and quantum error correction.', 'taxonomy_change', 'approved', 'addr_seed_author_1', 5, 1, '2026-02-28T00:00:00', 'apply'),
    ('prop_002', 'dao_cs', 'Require 3 evidence records for analyze-level proofs', 'Increase minimum evidence threshold for analyze-level skill proofs from 2 to 3 to improve credential rigor.', 'policy', 'active', 'addr_seed_member_1', 3, 2, '2026-04-30T00:00:00', 'remember'),
    ('prop_003', 'dao_web', 'Add WebAssembly skill under Frontend', 'Proposal to add WASM as a new skill under Frontend Development: compiling Rust/C++ to WebAssembly, JS interop, and performance optimization.', 'taxonomy_change', 'active', 'addr_seed_author_1', 2, 0, '2026-04-20T00:00:00', 'apply'),
    ('prop_004', 'dao_design', 'Content moderation policy for design courses', 'Establish guidelines for reviewing design course content: original work requirements, attribution standards, and accessibility compliance.', 'content_moderation', 'draft', 'addr_seed_author_3', 0, 0, NULL, 'remember'),
    ('prop_005', 'dao_math', 'Add Applied Mathematics subject', 'Create a new Applied Mathematics subject covering numerical methods, optimization, and mathematical modelling.', 'taxonomy_change', 'rejected', 'addr_seed_member_7', 1, 4, '2026-03-15T00:00:00', 'apply');

-- Proposal votes
INSERT INTO governance_proposal_votes (id, proposal_id, voter, in_favor) VALUES
    ('pvote_001', 'prop_001', 'addr_seed_author_2',  1),
    ('pvote_002', 'prop_001', 'addr_seed_member_1',  1),
    ('pvote_003', 'prop_001', 'addr_seed_member_2',  1),
    ('pvote_004', 'prop_001', 'addr_seed_member_3',  1),
    ('pvote_005', 'prop_001', 'addr_seed_member_4',  1),
    ('pvote_006', 'prop_001', 'addr_seed_member_5',  0),
    ('pvote_007', 'prop_002', 'addr_seed_author_1',  1),
    ('pvote_008', 'prop_002', 'addr_seed_author_2',  1),
    ('pvote_009', 'prop_002', 'addr_seed_member_1',  1),
    ('pvote_010', 'prop_002', 'addr_seed_member_3',  0),
    ('pvote_011', 'prop_002', 'addr_seed_member_4',  0),
    ('pvote_012', 'prop_003', 'addr_seed_member_2',  1),
    ('pvote_013', 'prop_003', 'addr_seed_member_9',  1),
    ('pvote_014', 'prop_005', 'addr_seed_author_2',  0),
    ('pvote_015', 'prop_005', 'addr_seed_member_1',  0),
    ('pvote_016', 'prop_005', 'addr_seed_member_6',  0),
    ('pvote_017', 'prop_005', 'addr_seed_member_8',  0),
    ('pvote_018', 'prop_005', 'addr_seed_member_7',  1);

-- ============================================================
-- P5: CLASSROOMS
-- ============================================================
INSERT INTO classrooms (id, name, description, owner_address) VALUES
    ('class_algo_study', 'Algorithms Study Group', 'A collaborative space for learners working through Algorithms 101. Share solutions, discuss approaches, and prep for assessments.', 'addr_seed_author_1'),
    ('class_web_cohort', 'Web Dev Cohort — Spring 2026', 'Spring 2026 cohort for the Full-Stack Web Development course. Weekly sync calls, code reviews, and project feedback.', 'addr_seed_author_1'),
    ('class_design_crit', 'Design Critique Circle', 'Weekly design critiques and portfolio reviews. Share your work, get constructive feedback, and improve together.', 'addr_seed_author_3');

INSERT INTO classroom_members (classroom_id, stake_address, role, joined_at) VALUES
    -- Algo study group
    ('class_algo_study', 'addr_seed_author_1',   'owner',     '2026-01-20T10:00:00'),
    ('class_algo_study', 'addr_seed_learner_1',  'member',    '2026-01-21T09:30:00'),
    ('class_algo_study', 'addr_seed_learner_2',  'member',    '2026-01-22T14:00:00'),
    ('class_algo_study', 'addr_seed_learner_3',  'member',    '2026-01-23T11:15:00'),
    ('class_algo_study', 'addr_seed_learner_4',  'member',    '2026-01-25T16:00:00'),
    -- Web cohort
    ('class_web_cohort', 'addr_seed_author_1',   'owner',     '2026-02-01T09:00:00'),
    ('class_web_cohort', 'addr_seed_learner_1',  'member',    '2026-02-02T10:00:00'),
    ('class_web_cohort', 'addr_seed_learner_5',  'member',    '2026-02-03T11:30:00'),
    ('class_web_cohort', 'addr_seed_member_10',  'member',    '2026-02-04T14:00:00'),
    -- Design crit circle
    ('class_design_crit', 'addr_seed_author_3',  'owner',     '2026-03-01T10:00:00'),
    ('class_design_crit', 'addr_seed_learner_1', 'member',    '2026-03-02T09:00:00'),
    ('class_design_crit', 'addr_seed_member_13', 'member',    '2026-03-03T13:30:00');

INSERT INTO classroom_channels (id, classroom_id, name, description, channel_type) VALUES
    ('chan_001', 'class_algo_study', 'general',     'General discussion and announcements', 'text'),
    ('chan_002', 'class_algo_study', 'help',        'Ask for help with problems and concepts', 'text'),
    ('chan_003', 'class_web_cohort', 'general',     'Cohort announcements and weekly updates', 'text'),
    ('chan_004', 'class_web_cohort', 'code-review', 'Share code for peer review', 'text'),
    ('chan_005', 'class_web_cohort', 'standups',    'Async daily standups — what did you learn today?', 'text'),
    ('chan_006', 'class_design_crit', 'general',    'Announcements and scheduling', 'text'),
    ('chan_007', 'class_design_crit', 'critique',   'Post your designs for feedback', 'text');

INSERT INTO classroom_messages (id, channel_id, classroom_id, sender_address, content, sent_at) VALUES
    ('msg_001', 'chan_001', 'class_algo_study', 'addr_seed_author_1',  'Welcome to the Algorithms Study Group! Post questions anytime, and lets use the #help channel for specific problem discussions.', '2026-01-20T10:05:00'),
    ('msg_002', 'chan_001', 'class_algo_study', 'addr_seed_learner_1', 'Thanks for setting this up! Im working through chapter 2 on linked lists — anyone else at that point?', '2026-01-21T09:45:00'),
    ('msg_003', 'chan_001', 'class_algo_study', 'addr_seed_learner_3', 'Just finished the arrays quiz with a 95%. The amortized analysis question was tricky.', '2026-01-23T14:30:00'),
    ('msg_004', 'chan_002', 'class_algo_study', 'addr_seed_learner_2', 'Can someone explain why the time complexity of building a heap is O(n) and not O(n log n)? The sift-down approach is confusing me.', '2026-01-24T10:00:00'),
    ('msg_005', 'chan_002', 'class_algo_study', 'addr_seed_author_1',  'Great question! The key insight is that most nodes are near the bottom of the heap, so their sift-down cost is O(1). The mathematical proof uses the fact that the sum of h/2^h converges to 2.', '2026-01-24T10:30:00'),
    ('msg_006', 'chan_002', 'class_algo_study', 'addr_seed_learner_2', 'That makes so much more sense now — thanks!', '2026-01-24T10:45:00'),
    ('msg_007', 'chan_003', 'class_web_cohort', 'addr_seed_author_1',  'Welcome to the Spring 2026 Web Dev Cohort! Well have weekly sync calls on Thursdays at 6pm UTC. First call this Thursday.', '2026-02-01T09:15:00'),
    ('msg_008', 'chan_004', 'class_web_cohort', 'addr_seed_learner_5', 'Just pushed my first Vue component — a todo list with Composition API. Would love feedback on the reactivity patterns.', '2026-02-10T15:00:00'),
    ('msg_009', 'chan_004', 'class_web_cohort', 'addr_seed_author_1',  'Nice work! One suggestion: use computed() instead of watch() for derived state. Its more declarative and Vue can optimize it better.', '2026-02-10T16:30:00'),
    ('msg_010', 'chan_005', 'class_web_cohort', 'addr_seed_learner_1', 'Today: finished the REST API chapter. Finally understand why PUT is idempotent but POST isnt.', '2026-02-20T18:00:00'),
    ('msg_011', 'chan_006', 'class_design_crit', 'addr_seed_author_3',  'Welcome to the Design Critique Circle! Post your work in #critique anytime, and well do live critique sessions every Friday at 3pm UTC.', '2026-03-01T10:15:00'),
    ('msg_012', 'chan_007', 'class_design_crit', 'addr_seed_learner_1', 'Sharing my first wireframe for a learning dashboard. Looking for feedback on the information hierarchy — is the skill progress too buried?', '2026-03-05T14:00:00'),
    ('msg_013', 'chan_007', 'class_design_crit', 'addr_seed_author_3',  'Good start! I would move the skill progress above the course list — its the primary metric learners care about. Also consider a sparkline showing progress over time.', '2026-03-05T15:30:00'),
    ('msg_014', 'chan_007', 'class_design_crit', 'addr_seed_member_13', 'Agree with the feedback above. Also the color contrast on the secondary text might not meet WCAG AA — try bumping it to at least 4.5:1.', '2026-03-05T16:00:00');

-- Join request for the approval-required classroom
INSERT INTO classroom_join_requests (id, classroom_id, stake_address, message, status) VALUES
    ('jr_001', 'class_web_cohort', 'addr_seed_learner_6', 'Hi, I am enrolled in the Web Dev course and would love to join the cohort for code reviews and weekly syncs.', 'pending'),
    ('jr_002', 'class_web_cohort', 'addr_seed_learner_7', 'Currently in chapter 3 of the course. Looking for study partners!', 'approved');

-- ============================================================
-- P6: SENTINEL (integrity), TUTORING, APP SETTINGS
-- ============================================================
-- Integrity sessions (tied to algo enrollment)
INSERT INTO integrity_sessions (id, enrollment_id, status, integrity_score, started_at, ended_at) VALUES
    ('isess_001', 'enroll_algo', 'completed', 0.94, '2026-01-20T10:00:00', '2026-01-20T10:45:00'),
    ('isess_002', 'enroll_algo', 'completed', 0.91, '2026-02-08T10:00:00', '2026-02-08T11:00:00'),
    ('isess_003', 'enroll_web',  'completed', 0.96, '2026-02-06T09:30:00', '2026-02-06T10:00:00'),
    ('isess_004', 'enroll_ml',   'completed', 0.89, '2026-03-16T10:45:00', '2026-03-16T11:30:00');

-- Integrity snapshots (behavioral signals per session)
INSERT INTO integrity_snapshots (id, session_id, typing_score, mouse_score, human_score, tab_score, paste_score, devtools_score, camera_score, composite_score, captured_at) VALUES
    ('isnap_001a', 'isess_001', 0.95, 0.92, 0.98, 1.0, 1.0, 1.0, 0.88, 0.94, '2026-01-20T10:05:00'),
    ('isnap_001b', 'isess_001', 0.93, 0.94, 0.97, 1.0, 1.0, 1.0, 0.90, 0.95, '2026-01-20T10:15:00'),
    ('isnap_001c', 'isess_001', 0.96, 0.91, 0.96, 1.0, 1.0, 1.0, 0.87, 0.93, '2026-01-20T10:30:00'),
    ('isnap_002a', 'isess_002', 0.91, 0.88, 0.95, 1.0, 0.95, 1.0, 0.85, 0.91, '2026-02-08T10:10:00'),
    ('isnap_002b', 'isess_002', 0.89, 0.90, 0.94, 1.0, 1.0,  1.0, 0.86, 0.92, '2026-02-08T10:30:00'),
    ('isnap_002c', 'isess_002', 0.92, 0.87, 0.96, 0.95, 1.0, 1.0, 0.88, 0.91, '2026-02-08T10:50:00'),
    ('isnap_003a', 'isess_003', 0.97, 0.95, 0.99, 1.0, 1.0, 1.0, 0.92, 0.96, '2026-02-06T09:40:00'),
    ('isnap_003b', 'isess_003', 0.96, 0.96, 0.98, 1.0, 1.0, 1.0, 0.94, 0.97, '2026-02-06T09:55:00'),
    ('isnap_004a', 'isess_004', 0.88, 0.85, 0.92, 1.0, 1.0, 1.0, 0.80, 0.89, '2026-03-16T11:00:00'),
    ('isnap_004b', 'isess_004', 0.90, 0.84, 0.91, 0.90, 1.0, 1.0, 0.82, 0.88, '2026-03-16T11:15:00');

-- Tutoring sessions
INSERT INTO tutoring_sessions (id, title, status, created_at, ended_at) VALUES
    ('tutor_001', 'Dynamic Programming — Top-down vs Bottom-up', 'ended', '2026-02-25T15:05:00', '2026-02-25T16:00:00'),
    ('tutor_002', 'Wireframing Review — Learning Dashboard', 'ended', '2026-03-10T14:02:00', '2026-03-10T15:00:00'),
    ('tutor_003', 'Graph Algorithms — BFS & DFS Walkthrough', 'active', '2026-04-10T16:00:00', NULL);

-- App settings
INSERT INTO app_settings (key, value) VALUES
    ('theme', 'dark'),
    ('language', 'en'),
    ('notifications_enabled', 'true'),
    ('auto_sync', 'true'),
    ('sentinel_camera_enabled', 'true'),
    ('sentinel_keyboard_enabled', 'true');

-- ============================================================
-- P7: TUTORIALS (kind='tutorial' courses with video_chapters)
-- ============================================================
-- Standalone video tutorials — minimal courses (1 chapter, 1 video
-- element, no quiz for v1 of the seed). The video elements reuse
-- REMOTE_SEED_ASSETS URLs so `seed_content_if_needed` downloads them
-- into iroh on first boot.
--
-- Using INSERT OR IGNORE so the backfill is idempotent for users who
-- already seeded before this data existed.

-- Tutorial courses
INSERT OR IGNORE INTO courses
    (id, title, description, author_address, thumbnail_cid, tags, skill_ids, status, kind, version, published_at)
VALUES
    ('course_tut_bigO',
     'Big-O in 8 Minutes',
     'A fast, visual tour of time complexity: what it is, why it matters, and how to read the common classes (O(1), O(log n), O(n), O(n log n), O(n²)) without getting lost in the math.',
     'addr_seed_author_1',
     NULL,
     '["algorithms","intro","complexity"]',
     '["skill_big_o"]',
     'published',
     'tutorial',
     1,
     '2026-02-18 10:00:00'),

    ('course_tut_asyncawait',
     'Async/Await Quick Tour',
     'Understand async/await in 6 minutes. When to use it, what it actually does under the hood, and the three mistakes that bite every beginner.',
     'addr_seed_author_1',
     NULL,
     '["javascript","async","web"]',
     '["skill_javascript"]',
     'published',
     'tutorial',
     1,
     '2026-02-24 14:30:00'),

    ('course_tut_ml_regression',
     'Linear Regression from First Principles',
     'Derive the least-squares fit on a napkin. No libraries, no black boxes — just geometry and one quadratic.',
     'addr_seed_author_2',
     NULL,
     '["ml","regression","math"]',
     '["skill_regression"]',
     'published',
     'tutorial',
     1,
     '2026-03-08 09:15:00'),

    ('course_tut_aes',
     'AES Walkthrough: What S-Boxes Actually Do',
     'A concrete walk through one full AES round, including the substitution box and why it exists. Good for anyone who has read "just use AES" but wants to know what "AES" means.',
     'addr_seed_author_2',
     NULL,
     '["crypto","aes","symmetric"]',
     '["skill_symmetric"]',
     'published',
     'tutorial',
     1,
     '2026-03-15 16:45:00'),

    ('course_tut_ux_interviews',
     'Running a Good User Interview',
     '12 minutes on what to ask, what not to ask, and how to listen. Includes a 3-question opening script that works for almost any product.',
     'addr_seed_author_3',
     NULL,
     '["ux","research","interviews"]',
     '["skill_user_research"]',
     'published',
     'tutorial',
     1,
     '2026-03-22 11:00:00');

-- Synthetic single-chapter wrappers for each tutorial
INSERT OR IGNORE INTO course_chapters (id, course_id, title, position) VALUES
    ('ch_tut_bigO',         'course_tut_bigO',         'Big-O in 8 Minutes',           0),
    ('ch_tut_asyncawait',   'course_tut_asyncawait',   'Async/Await Quick Tour',       0),
    ('ch_tut_ml_regression','course_tut_ml_regression','Linear Regression from First Principles', 0),
    ('ch_tut_aes',          'course_tut_aes',          'AES Walkthrough',              0),
    ('ch_tut_ux_interviews','course_tut_ux_interviews','Running a Good User Interview',0);

-- Video elements. content_cid is NULL; seed_content_if_needed fills
-- it in by downloading the URL from REMOTE_SEED_ASSETS (see seed_content.rs).
INSERT OR IGNORE INTO course_elements
    (id, chapter_id, title, element_type, content_cid, position, duration_seconds)
VALUES
    ('el_tut_bigO_video',         'ch_tut_bigO',         'Big-O in 8 Minutes',                     'video', NULL, 0, 480),
    ('el_tut_asyncawait_video',   'ch_tut_asyncawait',   'Async/Await Quick Tour',                 'video', NULL, 0, 360),
    ('el_tut_ml_regression_video','ch_tut_ml_regression','Linear Regression from First Principles','video', NULL, 0, 540),
    ('el_tut_aes_video',          'ch_tut_aes',          'AES Walkthrough',                        'video', NULL, 0, 600),
    ('el_tut_ux_interviews_video','ch_tut_ux_interviews','Running a Good User Interview',          'video', NULL, 0, 720);

-- Skill tags on the video elements (drives evidence contribution
-- if/when a learner runs an assessment under this tutorial)
INSERT OR IGNORE INTO element_skill_tags (element_id, skill_id, weight) VALUES
    ('el_tut_bigO_video',         'skill_big_o',         1.0),
    ('el_tut_asyncawait_video',   'skill_javascript',    1.0),
    ('el_tut_ml_regression_video','skill_regression',    1.0),
    ('el_tut_aes_video',          'skill_symmetric',     1.0),
    ('el_tut_ux_interviews_video','skill_user_research', 1.0);

-- Video chapter markers (timestamp navigation)
INSERT OR IGNORE INTO video_chapters (id, element_id, title, start_seconds, position) VALUES
    -- Big-O
    ('vc_bigO_1', 'el_tut_bigO_video', 'What Big-O actually measures', 0,   0),
    ('vc_bigO_2', 'el_tut_bigO_video', 'The common classes',           90,  1),
    ('vc_bigO_3', 'el_tut_bigO_video', 'Reading code for complexity',  240, 2),
    ('vc_bigO_4', 'el_tut_bigO_video', 'Practice: two walkthroughs',   360, 3),
    -- Async/Await
    ('vc_aa_1',   'el_tut_asyncawait_video', 'Why async exists at all',   0,   0),
    ('vc_aa_2',   'el_tut_asyncawait_video', 'await, the 10-second version', 60, 1),
    ('vc_aa_3',   'el_tut_asyncawait_video', 'The three mistakes',         180, 2),
    -- Linear regression
    ('vc_lr_1',   'el_tut_ml_regression_video', 'What we are fitting',          0,   0),
    ('vc_lr_2',   'el_tut_ml_regression_video', 'The least-squares derivation', 120, 1),
    ('vc_lr_3',   'el_tut_ml_regression_video', 'A worked example',             360, 2),
    -- AES
    ('vc_aes_1',  'el_tut_aes_video', 'Block ciphers, quickly',     0,   0),
    ('vc_aes_2',  'el_tut_aes_video', 'SubBytes and the S-Box',     120, 1),
    ('vc_aes_3',  'el_tut_aes_video', 'ShiftRows + MixColumns',     300, 2),
    ('vc_aes_4',  'el_tut_aes_video', 'AddRoundKey + wrapping up',  480, 3),
    -- UX interviews
    ('vc_ux_1',   'el_tut_ux_interviews_video', 'Planning the interview',      0,   0),
    ('vc_ux_2',   'el_tut_ux_interviews_video', 'The 3-question opening',     180, 1),
    ('vc_ux_3',   'el_tut_ux_interviews_video', 'Following signal, not script',420, 2),
    ('vc_ux_4',   'el_tut_ux_interviews_video', 'Debrief patterns',           600, 3);

-- ============================================================
-- P8: OPINIONS (Field Commentary)
-- ============================================================
-- Opinions appear as received-from-peers content. Each references a
-- locally-seeded credential id so the credential-verification path has
-- something to chew on in the UI. Signatures here are placeholder
-- strings — because opinions are inserted directly (not via the P2P
-- handler), no re-verification happens on read.
--
-- Authors map to the same seed author addresses used by courses, so
-- the Detail page can show a non-empty "credentials" section.

INSERT OR IGNORE INTO opinions
    (id, author_address, subject_field_id, title, summary, video_cid,
     thumbnail_cid, duration_seconds, credential_proof_ids,
     signature, public_key, published_at, received_at, withdrawn)
VALUES
    ('op_cs_01',
     'addr_seed_author_1',
     'sf_cs',
     'Teach arrays before objects. Always.',
     'The modern curriculum keeps trying to make the first two weeks "real-world". It should not. Arrays first, allocation mental model first, everything else later.',
     'https://media.w3.org/2010/05/bunny/trailer.mp4',
     NULL, 420, '["proof_001","proof_002"]',
     'seed_signature_cs01',
     'seed_publickey_cs01',
     '2026-03-04 10:00:00',
     '2026-03-04 10:02:00',
     0),

    ('op_cs_02',
     'addr_seed_author_1',
     'sf_cs',
     'Big-O is the wrong thing to test freshmen on',
     'We end up testing symbol manipulation instead of the underlying cost model. I''ve watched students get the right answer for the wrong reason too many times.',
     'https://media.w3.org/2010/05/bunny/trailer.mp4',
     NULL, 540, '["proof_002"]',
     'seed_signature_cs02',
     'seed_publickey_cs02',
     '2026-03-10 14:20:00',
     '2026-03-10 14:22:00',
     0),

    ('op_web_01',
     'addr_seed_author_1',
     'sf_web',
     'Why I stopped teaching flexbox first',
     'Grid is simpler to reason about once you are past the "boxes next to each other" case. Start with grid; flexbox comes up naturally when you need row-within-grid.',
     'https://media.w3.org/2010/05/video/movie_300.mp4',
     NULL, 360, '["proof_005","proof_006"]',
     'seed_signature_web01',
     'seed_publickey_web01',
     '2026-03-12 09:30:00',
     '2026-03-12 09:32:00',
     0),

    ('op_cyber_01',
     'addr_seed_author_2',
     'sf_cyber',
     'The block cipher zoo is too big',
     'We do not need to teach ten block ciphers. AES, ChaCha, and why you do not roll your own. Everything else is footnotes for people who want to read footnotes.',
     'https://media.w3.org/2010/05/sintel/trailer.mp4',
     NULL, 480, '["proof_009"]',
     'seed_signature_cyb01',
     'seed_publickey_cyb01',
     '2026-03-17 16:00:00',
     '2026-03-17 16:01:00',
     0),

    ('op_design_01',
     'addr_seed_author_3',
     'sf_design',
     'UX research is not optional, it''s the cheapest step',
     'Five $50 interviews shipped before a single Figma file will save you more weeks of engineering than any amount of tooling. I will die on this hill.',
     'https://interactive-examples.mdn.mozilla.net/media/cc0-videos/friday.mp4',
     NULL, 660, '["proof_010","proof_011"]',
     'seed_signature_dsg01',
     'seed_publickey_dsg01',
     '2026-03-21 11:15:00',
     '2026-03-21 11:16:00',
     0),

    ('op_design_02',
     'addr_seed_author_3',
     'sf_design',
     'Stop calling it "lo-fi wireframes"',
     'Call them sketches, or drawings, or prototypes. "Lo-fi wireframe" makes newcomers think there is a taxonomy that matters. There is not. Draw the thing.',
     'https://interactive-examples.mdn.mozilla.net/media/cc0-videos/friday.mp4',
     NULL, 300, '["proof_012"]',
     'seed_signature_dsg02',
     'seed_publickey_dsg02',
     '2026-03-28 13:45:00',
     '2026-03-28 13:46:00',
     0),

    ('op_civics_01',
     'addr_seed_author_5',
     'sf_civics',
     'Teach budget literacy before the constitution',
     'Most citizens never read the constitution. But every citizen pays taxes. Start with how public money flows, and the rest of civics becomes tangible — not academic.',
     'https://media.w3.org/2010/05/bunny/trailer.mp4',
     NULL, 390, '["proof_demo_constitutional","proof_demo_voting"]',
     'seed_signature_civ01',
     'seed_publickey_civ01',
     '2026-04-04 12:00:00',
     '2026-04-04 12:02:00',
     0);

-- ============================================================
-- P9: CIVIC SENSE TUTORIALS (kind='tutorial')
-- ============================================================
INSERT OR IGNORE INTO courses
    (id, title, description, author_address, thumbnail_cid, tags, skill_ids, status, kind, version, published_at)
VALUES
    ('course_tut_civ_constitution',
     'What a Constitution Actually Does (12 min)',
     'A short, practical walk through what constitutions do, what they do not, and why "follow the constitution" is a harder sentence than it sounds.',
     'addr_seed_author_5',
     NULL,
     '["civics","constitution","governance","global-south"]',
     '["skill_constitutional_literacy"]',
     'published',
     'tutorial',
     1,
     '2026-04-05 10:00:00'),

    ('course_tut_civ_budget',
     'How to Read a National Budget (8 min)',
     'Skip the jargon. Walk through a real budget document, find the three numbers that matter, and learn what to ask at the next public forum.',
     'addr_seed_author_5',
     NULL,
     '["civics","budget","finance","accountability"]',
     '["skill_public_finance_literacy"]',
     'published',
     'tutorial',
     1,
     '2026-04-06 14:00:00');

INSERT OR IGNORE INTO course_chapters (id, course_id, title, position) VALUES
    ('ch_tut_civ_constitution', 'course_tut_civ_constitution', 'What a Constitution Actually Does', 0),
    ('ch_tut_civ_budget',       'course_tut_civ_budget',       'How to Read a National Budget',     0);

INSERT OR IGNORE INTO course_elements
    (id, chapter_id, title, element_type, content_cid, position, duration_seconds)
VALUES
    ('el_tut_civ_constitution_video', 'ch_tut_civ_constitution', 'What a Constitution Actually Does', 'video', NULL, 0, 720),
    ('el_tut_civ_budget_video',       'ch_tut_civ_budget',       'How to Read a National Budget',     'video', NULL, 0, 480);

INSERT OR IGNORE INTO element_skill_tags (element_id, skill_id, weight) VALUES
    ('el_tut_civ_constitution_video', 'skill_constitutional_literacy', 1.0),
    ('el_tut_civ_budget_video',       'skill_public_finance_literacy', 1.0);

INSERT OR IGNORE INTO video_chapters (id, element_id, title, start_seconds, position) VALUES
    ('vc_civc_1', 'el_tut_civ_constitution_video', 'What a constitution is', 0,   0),
    ('vc_civc_2', 'el_tut_civ_constitution_video', 'What a constitution is not', 180, 1),
    ('vc_civc_3', 'el_tut_civ_constitution_video', 'Reading the one you have', 420, 2),
    ('vc_civb_1', 'el_tut_civ_budget_video',       'The budget in 30 seconds', 0,   0),
    ('vc_civb_2', 'el_tut_civ_budget_video',       'The three numbers that matter', 120, 1),
    ('vc_civb_3', 'el_tut_civ_budget_video',       'What to ask next', 300, 2);

-- ============================================================
-- P10: AUTHOR 5 (Dr. Nomvula Dlamini) — visual + reputation
-- ============================================================
UPDATE courses SET author_name = 'Dr. Nomvula Dlamini' WHERE author_address = 'addr_seed_author_5';

-- Dr. Dlamini's instructor reputation in civics skills
INSERT OR IGNORE INTO reputation_assertions (id, actor_address, role, skill_id, proficiency_level, score, evidence_count, median_impact, impact_p25, impact_p75, learner_count, impact_variance, window_start, window_end, computation_spec) VALUES
    ('rep_011', 'addr_seed_author_5', 'instructor', 'skill_constitutional_literacy', 'understand', 0.90, 8, 0.07, 0.04, 0.10, 6, 0.003, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_012', 'addr_seed_author_5', 'instructor', 'skill_voting_systems',          'apply',      0.87, 6, 0.06, 0.03, 0.09, 4, 0.004, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_013', 'addr_seed_author_5', 'instructor', 'skill_public_finance_literacy', 'understand', 0.85, 5, 0.05, 0.02, 0.08, 3, 0.005, '2025-10-01T00:00:00', '2026-04-01T00:00:00', 'v2');

-- ============================================================
-- P11: DEMO LEARNER — per-user state bound to addr_demo_learner
-- These rows are rewritten to the real wallet address by
-- `bind_current_user_to_seed()` after the wallet is created.
-- Until then, they are visible in debug/headless scenarios but
-- unreachable from the UI (dashboards filter by local_identity).
-- ============================================================

-- DID key registry — one active DID per seed author so the Credentials
-- dashboard can resolve issuer DIDs. Pubkeys are placeholder hex
-- (64 chars = 32 bytes). Real verification does not happen on read.
INSERT OR IGNORE INTO key_registry (did, key_id, public_key_hex, valid_from) VALUES
    ('did:key:z6MkSeedAuthor1AlgoWebInstructorXXXXXXXXXXXXXXX', 'did:key:z6MkSeedAuthor1AlgoWebInstructorXXXXXXXXXXXXXXX#key-1', '11a1b1c1d1e1f1012121314151617181192a2b2c2d2e2f01121314151617181', '2025-09-01T00:00:00Z'),
    ('did:key:z6MkSeedAuthor2DataCryptoInstructorXXXXXXXXXXXX', 'did:key:z6MkSeedAuthor2DataCryptoInstructorXXXXXXXXXXXX#key-1', '22b2c2d2e2f2022323242526272829212a3b3c3d3e3f02232425262728291301', '2025-09-01T00:00:00Z'),
    ('did:key:z6MkSeedAuthor3DesignInstructorXXXXXXXXXXXXXXX',  'did:key:z6MkSeedAuthor3DesignInstructorXXXXXXXXXXXXXXX#key-1',  '33c3d3e3f3041434343536373839313a4b4c4d4e4f504334353637383931401',  '2025-09-01T00:00:00Z'),
    ('did:key:z6MkSeedAuthor4MathInstructorXXXXXXXXXXXXXXXXX',  'did:key:z6MkSeedAuthor4MathInstructorXXXXXXXXXXXXXXXXX#key-1',  '44d4e4f405144454546474849414b5c5d5e5f6054455464748494a1b5c501',     '2025-09-01T00:00:00Z'),
    ('did:key:z6MkSeedAuthor5CivicsInstructorXXXXXXXXXXXXXXX',  'did:key:z6MkSeedAuthor5CivicsInstructorXXXXXXXXXXXXXXX#key-1',  '55e5f5061254555657585a6a7c7d7e7f605565574757677879891b2c5d601',     '2025-09-01T00:00:00Z'),
    ('did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX',  'did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX#key-1',  '66f60617236465666768797b8c9d8e8f706667685858789a8b9c1d2e7f801',     '2025-10-01T00:00:00Z');

-- Revocation status list for credential suspension demo
INSERT OR IGNORE INTO credential_status_lists (list_id, issuer_did, version, status_purpose, bits, bit_length, signature) VALUES
    ('urn:status-list:seed-author-5-revoke-2026',
     'did:key:z6MkSeedAuthor5CivicsInstructorXXXXXXXXXXXXXXX',
     1,
     'revocation',
     X'02',       -- binary: bit 1 set => credential index 1 revoked
     8,
     'seed_statuslist_sig_civ5');

-- Demo learner's 5 Verifiable Credentials. signed_vc_json is a minimal
-- W3C VC shape — the UI renders issuer, subject, type, issuance, and
-- skill from these columns without re-parsing.
INSERT OR IGNORE INTO credentials
    (id, issuer_did, subject_did, credential_type, claim_kind, skill_id,
     issuance_date, expiration_date, signed_vc_json, integrity_hash,
     status_list_id, status_list_index, revoked, received_at)
VALUES
    ('urn:uuid:cred-demo-algo-completion',
     'did:key:z6MkSeedAuthor1AlgoWebInstructorXXXXXXXXXXXXXXX',
     'did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX',
     'FormalCredential',
     'skill',
     'skill_big_o',
     '2026-03-20T16:45:00Z',
     NULL,
     '{"@context":["https://www.w3.org/ns/credentials/v2"],"type":["VerifiableCredential","FormalCredential"],"issuer":"did:key:z6MkSeedAuthor1AlgoWebInstructorXXXXXXXXXXXXXXX","validFrom":"2026-03-20T16:45:00Z","credentialSubject":{"id":"did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX","skill":"skill_big_o","proficiency":"analyze","course":"course_algo_101"}}',
     'a1b2c3d4e5f60718293a4b5c6d7e8f9001112233445566778899aabbccddeeff',
     NULL, NULL, 0, '2026-03-21T09:00:00Z'),

    ('urn:uuid:cred-demo-civics-constitution',
     'did:key:z6MkSeedAuthor5CivicsInstructorXXXXXXXXXXXXXXX',
     'did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX',
     'AssessmentCredential',
     'skill',
     'skill_constitutional_literacy',
     '2026-04-08T11:15:00Z',
     NULL,
     '{"@context":["https://www.w3.org/ns/credentials/v2"],"type":["VerifiableCredential","AssessmentCredential"],"issuer":"did:key:z6MkSeedAuthor5CivicsInstructorXXXXXXXXXXXXXXX","validFrom":"2026-04-08T11:15:00Z","credentialSubject":{"id":"did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX","skill":"skill_constitutional_literacy","proficiency":"remember","score":0.82}}',
     'b2c3d4e5f60718293a4b5c6d7e8f900111223344556677889900aabbccddeeff',
     'urn:status-list:seed-author-5-revoke-2026', 0, 0, '2026-04-08T11:20:00Z'),

    ('urn:uuid:cred-demo-ml-regression',
     'did:key:z6MkSeedAuthor2DataCryptoInstructorXXXXXXXXXXXX',
     'did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX',
     'AssessmentCredential',
     'skill',
     'skill_regression',
     '2026-03-16T11:00:00Z',
     NULL,
     '{"@context":["https://www.w3.org/ns/credentials/v2"],"type":["VerifiableCredential","AssessmentCredential"],"issuer":"did:key:z6MkSeedAuthor2DataCryptoInstructorXXXXXXXXXXXX","validFrom":"2026-03-16T11:00:00Z","credentialSubject":{"id":"did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX","skill":"skill_regression","proficiency":"remember","score":0.87,"course":"course_ml_foundations"}}',
     'c3d4e5f607182939abcdef012233445566778899aabbccddee001122334455ff',
     NULL, NULL, 0, '2026-03-17T09:30:00Z'),

    ('urn:uuid:cred-demo-design-role',
     'did:key:z6MkSeedAuthor3DesignInstructorXXXXXXXXXXXXXXX',
     'did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX',
     'RoleCredential',
     'role',
     NULL,
     '2026-03-05T10:00:00Z',
     NULL,
     '{"@context":["https://www.w3.org/ns/credentials/v2"],"type":["VerifiableCredential","RoleCredential"],"issuer":"did:key:z6MkSeedAuthor3DesignInstructorXXXXXXXXXXXXXXX","validFrom":"2026-03-05T10:00:00Z","credentialSubject":{"id":"did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX","role":"classroom.member","classroom":"class_design_crit"}}',
     'd4e5f6071829394abcdef0123344556677889900aabbccddee0011223344556',
     NULL, NULL, 0, '2026-03-05T10:05:00Z'),

    ('urn:uuid:cred-demo-civic-interest',
     'did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX',
     'did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX',
     'SelfAssertion',
     'custom',
     NULL,
     '2026-04-10T09:00:00Z',
     NULL,
     '{"@context":["https://www.w3.org/ns/credentials/v2"],"type":["VerifiableCredential","SelfAssertion"],"issuer":"did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX","validFrom":"2026-04-10T09:00:00Z","credentialSubject":{"id":"did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX","interest":"civic-tech","description":"Building open-data tools for municipal accountability"}}',
     'e5f6071829394abcdef0123344556677889900aabbccddeeff001122334455667',
     NULL, NULL, 0, '2026-04-10T09:00:00Z');

-- Revoked credential: the civics assessment cred gets its bit set
UPDATE credentials SET revoked = 1, revoked_at = '2026-04-11T12:00:00Z', revocation_reason = 'demo: issuer retracted after review'
  WHERE id = 'urn:uuid:cred-demo-civics-constitution';

-- Post-migration 040: the demo-learner civics seeds (skill_assessments,
-- evidence_records, skill_proof_evidence, skill_proofs,
-- reputation_assertions for the demo learner, reputation_impact_deltas)
-- are retired because the underlying tables are dropped. The
-- `derived_skill_states` snapshot below still represents the demo
-- learner's progression because that table is independent of the
-- SkillProof pipeline and reflects the aggregation output directly.

-- Learner-role reputation assertions for the demo user (will be
-- rewritten to the real wallet by bind_current_user_to_seed).
INSERT OR IGNORE INTO reputation_assertions (id, actor_address, role, skill_id, proficiency_level, score, evidence_count, median_impact, impact_p25, impact_p75, learner_count, impact_variance, window_start, window_end, computation_spec) VALUES
    ('rep_demo_01', 'addr_demo_learner', 'learner', 'skill_big_o',                   'analyze',  0.88, 2, 0.0, 0.0, 0.0, 0, 0.002, '2026-01-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_demo_02', 'addr_demo_learner', 'learner', 'skill_arrays',                  'apply',    0.91, 1, 0.0, 0.0, 0.0, 0, 0.003, '2026-01-01T00:00:00', '2026-04-01T00:00:00', 'v2'),
    ('rep_demo_03', 'addr_demo_learner', 'learner', 'skill_constitutional_literacy', 'remember', 0.82, 2, 0.0, 0.0, 0.0, 0, 0.002, '2026-04-01T00:00:00', '2026-04-15T00:00:00', 'v2');

-- Derived skill states — materialised aggregation output per (subject, skill).
-- The aggregation engine would normally write these; we inline a realistic
-- snapshot so the skill graph can render filled-in progress without the
-- user having to trigger a recompute.
INSERT OR IGNORE INTO derived_skill_states (subject_did, skill_id, calculation_version, raw_score, confidence, trust_score, level, evidence_mass, unique_issuer_clusters, active_evidence_count, state_json, computed_at) VALUES
    ('did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX', 'skill_big_o',                   'v2', 0.88, 0.85, 0.90, 4, 1.8, 1, 2, '{"level":4,"raw":0.88,"confidence":0.85,"last_updated":"2026-03-10T10:00:00Z"}', '2026-04-11T09:00:00Z'),
    ('did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX', 'skill_arrays',                  'v2', 0.91, 0.80, 0.90, 3, 1.0, 1, 1, '{"level":3,"raw":0.91,"confidence":0.80,"last_updated":"2026-02-05T15:00:00Z"}', '2026-04-11T09:00:00Z'),
    ('did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX', 'skill_regression',              'v2', 0.86, 0.75, 0.90, 1, 0.9, 1, 1, '{"level":1,"raw":0.86,"confidence":0.75,"last_updated":"2026-03-16T11:00:00Z"}', '2026-04-11T09:00:00Z'),
    ('did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX', 'skill_html_css',                'v2', 0.94, 0.78, 0.90, 3, 1.0, 1, 1, '{"level":3,"raw":0.94,"confidence":0.78,"last_updated":"2026-02-06T09:45:00Z"}', '2026-04-11T09:00:00Z'),
    ('did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX', 'skill_user_research',           'v2', 0.82, 0.72, 0.85, 5, 0.9, 1, 1, '{"level":5,"raw":0.82,"confidence":0.72,"last_updated":"2026-02-20T14:00:00Z"}', '2026-04-11T09:00:00Z'),
    ('did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX', 'skill_constitutional_literacy', 'v2', 0.82, 0.80, 0.90, 1, 1.8, 1, 2, '{"level":1,"raw":0.82,"confidence":0.80,"last_updated":"2026-04-11T09:30:00Z"}', '2026-04-11T09:30:00Z'),
    ('did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX', 'skill_voting_systems',          'v2', 0.81, 0.70, 0.90, 3, 0.9, 1, 1, '{"level":3,"raw":0.81,"confidence":0.70,"last_updated":"2026-04-09T14:00:00Z"}', '2026-04-11T09:30:00Z'),
    ('did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX', 'skill_public_finance_literacy', 'v2', 0.78, 0.65, 0.85, 2, 0.9, 1, 1, '{"level":2,"raw":0.78,"confidence":0.65,"last_updated":"2026-04-10T10:00:00Z"}', '2026-04-11T09:30:00Z');

-- Bind demo-learner integrity sessions via placeholder; keeps the
-- Sentinel dashboard populated until bind_current_user_to_seed fires.

-- ============================================================
-- P12: MULTI-DEVICE SYNC (second paired device + recent activity log)
-- ============================================================
INSERT OR IGNORE INTO devices (id, device_name, platform, first_seen, last_synced, is_local, peer_id) VALUES
    ('dev_demo_local',  'This Device',         'macos',   '2026-01-10T09:00:00', '2026-04-14T09:00:00', 1, '12D3KooWDemoLocalPlaceholderPeerIdXXXXXXXXXXXXX'),
    ('dev_demo_mobile', 'Pixel 9 Pro (paired)','android', '2026-03-02T11:15:00', '2026-04-14T08:42:00', 0, '12D3KooWDemoMobilePlaceholderPeerIdXXXXXXXXXXXX');

INSERT OR IGNORE INTO sync_state (device_id, table_name, last_synced_at, row_count) VALUES
    ('dev_demo_mobile', 'enrollments',        '2026-04-14T08:42:00', 4),
    ('dev_demo_mobile', 'element_progress',   '2026-04-14T08:42:00', 40),
    ('dev_demo_mobile', 'course_notes',       '2026-04-13T21:10:00', 3),
    ('dev_demo_mobile', 'credentials',        '2026-04-14T08:42:00', 5);

INSERT OR IGNORE INTO sync_log (entity_type, entity_id, direction, peer_id, synced_at) VALUES
    ('catalog',          'course_civics_101',                  'received', '12D3KooWDemoMobilePlaceholderPeerIdXXXXXXXXXXXX', '2026-04-12T09:30:00'),
    ('credentials',      'urn:uuid:cred-demo-civics-constitution', 'sent', '12D3KooWDemoMobilePlaceholderPeerIdXXXXXXXXXXXX', '2026-04-13T10:02:00'),
    ('taxonomy',         'sf_civics',                          'sent',     '12D3KooWDemoMobilePlaceholderPeerIdXXXXXXXXXXXX', '2026-04-13T10:05:00'),
    ('catalog',          'course_tut_civ_constitution',        'received', '12D3KooWDemoMobilePlaceholderPeerIdXXXXXXXXXXXX', '2026-04-14T08:41:00');

-- ============================================================
-- P13: ATTESTATION REQUIREMENTS (retired with migration 040 — the
-- attestation_requirements table is dropped; gating moves to the
-- completion validator on-chain in a follow-up session.)
-- ============================================================

-- ============================================================
-- P14: PINBOARD OBSERVATIONS (opt-in content pinning commitments)
-- ============================================================
INSERT OR IGNORE INTO pinboard_observations (id, pinner_did, subject_did, scope, commitment_since, signature, public_key) VALUES
    ('pb_demo_01',
     'did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX',
     'course_algo_101',
     '["courses","element_media"]',
     '2026-02-20T10:00:00Z',
     'seed_pinboard_sig_demo_01',
     '66f60617236465666768797b8c9d8e8f706667685858789a8b9c1d2e7f801'),
    ('pb_demo_02',
     'did:key:z6MkDemoLearnerPlaceholderXXXXXXXXXXXXXXXXXXXX',
     'course_civics_101',
     '["courses","element_media","opinions"]',
     '2026-04-12T09:00:00Z',
     'seed_pinboard_sig_demo_02',
     '66f60617236465666768797b8c9d8e8f706667685858789a8b9c1d2e7f801'),
    ('pb_peer_01',
     'did:key:z6MkSeedPeerEducator1XXXXXXXXXXXXXXXXXXXXXXXX',
     'course_ml_foundations',
     '["courses"]',
     '2026-03-10T14:00:00Z',
     'seed_pinboard_sig_peer_01',
     '77aabbccddeeff00112233445566778899aabbccddeeff001122334455667788'),
    ('pb_peer_02',
     'did:key:z6MkSeedPeerEducator2XXXXXXXXXXXXXXXXXXXXXXXX',
     'course_crypto_101',
     '["courses","element_media"]',
     '2026-03-18T16:00:00Z',
     'seed_pinboard_sig_peer_02',
     '88bbccddeeff001122334455667788990011223344556677889900aabbccddee');

-- ============================================================
-- P15: CONTENT PROVENANCE — mark all seeded content as AI-generated
-- ============================================================
UPDATE courses  SET provenance = 'ai_generated' WHERE provenance IS NULL;
UPDATE opinions SET provenance = 'ai_generated' WHERE provenance IS NULL;

-- ============================================================
-- P16: VC-FIRST DEMO SEEDS
-- Populates a demo completion observation (waiting on auto-issuance
-- once the observer daemon ticks) and a demo attestation requirement
-- on a high-stakes civics course so the frontend can render the full
-- state machine: observation → attestation requirement → issued VC.
-- ============================================================
INSERT OR IGNORE INTO completion_attestation_requirements
    (course_id, required_attestors, dao_id, set_by_proposal)
VALUES
    ('course_civics_101',                   2, 'dao_civics', NULL),
    ('636f757273655f636976696373203131',    2, 'dao_civics', NULL);

-- A pending observation keyed on the civics course (hex-encoded
-- bytes of "course_civics_101"). The credential_id is NULL, which
-- means the observer saw a mint but has not yet auto-issued.
INSERT OR IGNORE INTO completion_observations (
    policy_id, asset_name_hex, tx_hash, subject_pubkey,
    course_id, completion_root, completion_time,
    credential_id, observed_at, issued_at
) VALUES (
    '6380450179a6933acdf76213732f8626e1486b9ed5cc7fe7f46c98e0',
    'aabbccddeeff00112233445566778899aabbccddeeff0011deadbeef',
    'deadbeef00000000000000000000000000000000000000000000000000000000',
    'abababababababababababababababababababababababababababababababab',
    '636f757273655f636976696373203131',
    '11aa22bb33cc44dd55ee66ff778899aa11bb22cc33dd44ee55ff66aa7788cc99',
    '2026-04-24T12:00:00Z',
    NULL,
    '2026-04-24 12:00:00',
    NULL
);

-- A demo completion attestation on the pending observation. One
-- attestor signed the witness tx; a second is needed before the
-- observer will auto-issue.
INSERT OR IGNORE INTO completion_attestations (
    id, witness_tx_hash, attestor_did, attestor_pubkey, signature, note
) VALUES (
    'ca_demo_first',
    'deadbeef00000000000000000000000000000000000000000000000000000000',
    'did:key:zCivicsDemoAttestorX',
    'cafef00dcafef00dcafef00dcafef00dcafef00dcafef00dcafef00dcafef00d',
    'beefcafe0011223344556677889900aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899aabb',
    'Witnessed live; integrity score 0.92'
);

-- Re-enable FK checks
PRAGMA foreign_keys = ON;

"##;

// Gate tests behind `has_app_lib` because this file is shared with the CLI
// crate via `#[path]`, and the tests depend on `crate::db::Database` which
// only exists in the main Tauri crate.
#[cfg(all(test, has_app_lib))]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn seed_inserts_data() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");

        let inserted = seed_if_empty(db.conn()).expect("seed");
        assert!(inserted, "should insert seed data on empty db");

        // Verify counts
        let fields: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM subject_fields", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fields, 7);

        let subjects: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM subjects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(subjects, 22);

        let skills: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM skills", [], |r| r.get(0))
            .unwrap();
        assert!(skills >= 85, "expected >= 85 skills, got {}", skills);

        let prereqs: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM skill_prerequisites", [], |r| r.get(0))
            .unwrap();
        assert!(
            prereqs >= 50,
            "expected >= 50 prerequisite edges, got {}",
            prereqs
        );

        let relations: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM skill_relations", [], |r| r.get(0))
            .unwrap();
        assert!(
            relations >= 10,
            "expected >= 10 relation edges, got {}",
            relations
        );

        let daos: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM governance_daos", [], |r| r.get(0))
            .unwrap();
        assert_eq!(daos, 7);

        // Courses: 6 original full courses + 1 civics + 5 tutorials (backfill)
        // + 2 civics tutorials = 14 total when backfill runs on first seed.
        let courses: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM courses", [], |r| r.get(0))
            .unwrap();
        assert!(
            courses >= 7,
            "expected >= 7 full courses (full + civics + tutorials), got {}",
            courses
        );

        let chapters: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM course_chapters", [], |r| r.get(0))
            .unwrap();
        assert!(chapters >= 20, "expected >= 20 chapters, got {}", chapters);

        let elements: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM course_elements", [], |r| r.get(0))
            .unwrap();
        assert!(elements >= 80, "expected >= 80 elements, got {}", elements);

        // Verify fair representation of element types
        let element_types: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(DISTINCT element_type) FROM course_elements",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            element_types >= 9,
            "expected >= 9 distinct element types, got {}",
            element_types
        );

        let tags: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM element_skill_tags", [], |r| r.get(0))
            .unwrap();
        assert!(
            tags >= 90,
            "expected >= 90 element-skill tags, got {}",
            tags
        );
    }

    #[test]
    fn seed_populates_visual_assets() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        seed_if_empty(db.conn()).expect("seed");

        // Check thumbnail_svg — 7 full courses (6 original + civics) have SVGs.
        // Tutorials intentionally don't.
        let svg_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM courses WHERE thumbnail_svg IS NOT NULL AND thumbnail_svg != ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            svg_count >= 7,
            "expected >= 7 courses with thumbnail_svg, got {}",
            svg_count
        );

        // Check author_name on all full + tutorial courses
        let author_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM courses WHERE author_name IS NOT NULL AND author_name != ''",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            author_count >= 7,
            "expected >= 7 courses with author_name, got {}",
            author_count
        );

        // Check a specific SVG starts correctly
        let svg: String = db
            .conn()
            .query_row(
                "SELECT thumbnail_svg FROM courses WHERE id = 'course_algo_101'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            svg.starts_with("<svg"),
            "SVG should start with <svg, got: {}",
            &svg[..40.min(svg.len())]
        );
    }

    #[test]
    fn seed_is_idempotent() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");

        let first = seed_if_empty(db.conn()).expect("first seed");
        assert!(first);

        let second = seed_if_empty(db.conn()).expect("second seed");
        assert!(!second, "should skip seed on non-empty db");

        // Counts unchanged
        let fields: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM subject_fields", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fields, 7);
    }

    #[test]
    fn civics_content_is_seeded() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        seed_if_empty(db.conn()).expect("seed");

        let civics_field: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM subject_fields WHERE id = 'sf_civics'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(civics_field, 1, "civics subject field must exist");

        let civics_skills: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM skills s JOIN subjects sub ON s.subject_id = sub.id WHERE sub.subject_field_id = 'sf_civics'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            civics_skills >= 15,
            "expected >= 15 civics skills, got {}",
            civics_skills
        );

        let civics_course: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM courses WHERE id = 'course_civics_101'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(civics_course, 1, "Civic Sense course must exist");

        let civics_tutorials: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM courses WHERE kind = 'tutorial' AND author_address = 'addr_seed_author_5'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(civics_tutorials, 2, "2 civics tutorials must exist");
    }

    #[test]
    fn ai_generated_provenance_is_set() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        seed_if_empty(db.conn()).expect("seed");

        let courses_unset: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM courses WHERE provenance IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            courses_unset, 0,
            "all seeded courses should have provenance set"
        );

        let opinions_unset: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM opinions WHERE provenance IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            opinions_unset, 0,
            "all seeded opinions should have provenance set"
        );
    }

    #[test]
    fn demo_learner_state_is_seeded() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        seed_if_empty(db.conn()).expect("seed");

        let creds: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM credentials", [], |r| r.get(0))
            .unwrap();
        assert_eq!(creds, 5, "5 demo VCs should be seeded");

        let revoked: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM credentials WHERE revoked = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(revoked, 1, "1 demo VC should be marked revoked");

        let devices: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0))
            .unwrap();
        assert_eq!(devices, 2, "2 devices should be seeded");

        let pinboard: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM pinboard_observations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(
            pinboard >= 4,
            "expected >= 4 pinboard observations, got {pinboard}"
        );

        let pending_completions: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM completion_observations WHERE credential_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            pending_completions >= 1,
            "expected >= 1 pending completion observation"
        );

        let requirements: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM completion_attestation_requirements",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            requirements >= 1,
            "expected >= 1 completion attestation requirement"
        );

        let attestations: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM completion_attestations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(attestations >= 1, "expected >= 1 completion attestation");
    }

    #[test]
    fn bind_current_user_rewrites_learner_rows() {
        let db = Database::open_in_memory().expect("open");
        db.run_migrations().expect("migrate");
        seed_if_empty(db.conn()).expect("seed");

        // Before bind: addr_demo_learner should have learner-role assertions.
        let before: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM reputation_assertions WHERE actor_address = 'addr_demo_learner'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(before >= 3, "expected demo learner assertions before bind");

        // Populate local_identity and bind.
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) VALUES (1, 'stake_test_user_1', 'addr_test_user_1')",
                [],
            )
            .expect("insert identity");
        let rewritten = bind_current_user_to_seed(db.conn()).expect("bind");
        assert!(rewritten >= before as usize);

        // After: no demo rows remain.
        let after: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM reputation_assertions WHERE actor_address = 'addr_demo_learner'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(after, 0, "no demo sentinel rows should remain after bind");

        // Idempotent second call.
        let again = bind_current_user_to_seed(db.conn()).expect("bind2");
        assert_eq!(again, 0);
    }
}
