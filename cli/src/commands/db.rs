use anyhow::{bail, Context, Result};
use bytesize::ByteSize;
use clap::Subcommand;
use rusqlite::{Connection, OpenFlags};
use std::fs;

use crate::context::ProjectContext;
use crate::output;

// ── Shared SQL from src-tauri ───────────────────────────────────────
// Include the schema and seed modules directly from the Tauri crate so
// there is a single source of truth for migrations and seed data.

#[path = "../../../src-tauri/src/db/schema.rs"]
mod schema;

#[path = "../../../src-tauri/src/db/seed.rs"]
mod seed;

// Provide the SEED_CONTENT constant that seed.rs references via
// `super::seed_content::SEED_CONTENT`. In the main Tauri crate this
// lives in `db::seed_content`; for the CLI we include only the
// dependency-free data constant (the iroh seeding function is not
// needed here).
#[path = "../../../src-tauri/src/db/seed_content_data.rs"]
mod seed_content;

// ── CLI subcommands ─────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum DbCommand {
    /// Show database status (tables, row counts, size)
    Status,

    /// Run pending database schema migrations
    Migrate,

    /// Seed demo data (taxonomy, courses, governance)
    Seed {
        /// Force re-seed even if data already exists (clears seed tables first)
        #[arg(long)]
        force: bool,
    },

    /// Reset all app data (database + vault + iroh). Requires --force.
    Reset {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
}

pub fn execute(
    cmd: &DbCommand,
    ctx: &ProjectContext,
    password_file: Option<&std::path::Path>,
) -> Result<()> {
    match cmd {
        DbCommand::Status => show_status(ctx, password_file),
        DbCommand::Migrate => run_migrate(ctx, password_file),
        DbCommand::Seed { force } => run_seed(ctx, *force, password_file),
        DbCommand::Reset { force } => reset_data(ctx, *force),
    }
}

// ── Migration runner ────────────────────────────────────────────────
// Mirrors the logic in src-tauri/src/db/mod.rs — small enough to
// duplicate rather than pulling in the full app_lib crate.

fn ensure_migration_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            version    INTEGER PRIMARY KEY,
            name       TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .context("Failed to create _migrations table")?;
    Ok(())
}

fn current_version(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM _migrations",
        [],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

fn latest_version() -> i64 {
    schema::MIGRATIONS.last().map(|(v, _, _)| *v).unwrap_or(0)
}

fn apply_migrations(conn: &Connection) -> Result<usize> {
    ensure_migration_table(conn)?;

    let current = current_version(conn);
    let mut applied = 0;

    for (version, name, sql) in schema::MIGRATIONS {
        if *version > current {
            output::info(&format!("Applying migration {}: {}", version, name));
            conn.execute_batch(sql)
                .with_context(|| format!("Migration {} ({}) failed", version, name))?;
            conn.execute(
                "INSERT INTO _migrations (version, name) VALUES (?1, ?2)",
                rusqlite::params![version, name],
            )?;
            applied += 1;
        }
    }

    Ok(applied)
}

// ── Open DB helper ──────────────────────────────────────────────────

/// Get the vault password — from file if provided, otherwise prompt interactively.
fn get_vault_password(password_file: Option<&std::path::Path>) -> Result<String> {
    if let Some(path) = password_file {
        fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .with_context(|| format!("Failed to read password file: {}", path.display()))
    } else {
        let password = dialoguer::Password::new()
            .with_prompt("Vault password")
            .interact()
            .context("Failed to read password")?;
        Ok(password)
    }
}

/// Open the SQLCipher-encrypted database, deriving the key from the vault.
fn open_db(ctx: &ProjectContext, password_file: Option<&std::path::Path>) -> Result<Connection> {
    if !ctx.has_app_data() {
        fs::create_dir_all(&ctx.app_data_dir).context("Failed to create app data directory")?;
    }

    if !ctx.has_vault() {
        bail!(
            "No vault found at {}.\n\
             Launch the app and create a wallet first, then use the CLI.",
            ctx.vault_dir().display()
        );
    }

    let password = get_vault_password(password_file)?;
    let db_key = ctx.derive_db_key(&password)?;

    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
    let conn = Connection::open_with_flags(ctx.db_path(), flags)
        .with_context(|| format!("Failed to open database at {}", ctx.db_path().display()))?;

    let key_hex = hex::encode(db_key);
    conn.pragma_update(None, "key", format!("x'{key_hex}'"))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;

    // Verify the key works by reading sqlite_master
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
        .context("Failed to decrypt database — wrong password?")?;

    Ok(conn)
}

// ── Subcommand: migrate ─────────────────────────────────────────────

fn run_migrate(ctx: &ProjectContext, password_file: Option<&std::path::Path>) -> Result<()> {
    output::header("Database migrate");
    output::kv("Database", &ctx.db_path().display().to_string());

    let conn = open_db(ctx, password_file)?;
    ensure_migration_table(&conn)?;

    let before = current_version(&conn);
    let latest = latest_version();

    output::kv("Current version", &format!("v{}", before));
    output::kv("Latest version", &format!("v{}", latest));

    if before >= latest {
        output::blank();
        output::success("Already up to date — nothing to migrate.");
        return Ok(());
    }

    output::blank();

    let applied = apply_migrations(&conn)?;

    output::blank();
    output::success(&format!(
        "Applied {} migration(s) (v{} → v{})",
        applied,
        before,
        current_version(&conn)
    ));

    Ok(())
}

// ── Subcommand: seed ────────────────────────────────────────────────

fn run_seed(
    ctx: &ProjectContext,
    force: bool,
    password_file: Option<&std::path::Path>,
) -> Result<()> {
    output::header("Database seed");
    output::kv("Database", &ctx.db_path().display().to_string());

    let conn = open_db(ctx, password_file)?;

    // Ensure migrations are current first
    let applied = apply_migrations(&conn)?;
    if applied > 0 {
        output::info(&format!("Applied {} pending migration(s) first", applied));
    }

    if force {
        output::blank();
        output::warning("Force mode: clearing existing seed data...");

        // Delete in dependency order (leaf tables first).
        // Use PRAGMA foreign_keys = OFF to avoid ordering headaches on
        // interconnected tables (evidence → proofs → skills, etc.).
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;

             -- Classrooms & messaging
             DELETE FROM classroom_messages;
             DELETE FROM classroom_calls;
             DELETE FROM classroom_channels;
             DELETE FROM classroom_join_requests;
             DELETE FROM classroom_members;
             DELETE FROM classroom_group_keys;
             DELETE FROM classrooms;

             -- Tutoring & integrity
             DELETE FROM tutoring_sessions;
             DELETE FROM integrity_snapshots;
             DELETE FROM integrity_sessions;

             -- Governance lifecycle
             DELETE FROM governance_proposal_votes;
             DELETE FROM governance_election_votes;
             DELETE FROM governance_election_nominees;
             DELETE FROM governance_elections;
             DELETE FROM governance_proposals;
             DELETE FROM governance_dao_members;
             DELETE FROM governance_daos;

             -- Reputation
             DELETE FROM reputation_impact_deltas;
             DELETE FROM reputation_evidence;
             DELETE FROM reputation_snapshots;
             DELETE FROM reputation_assertions;

             -- Evidence & proofs
             DELETE FROM skill_proof_evidence;
             DELETE FROM skill_proofs;
             DELETE FROM evidence_records;
             DELETE FROM skill_assessments;

             -- Progress & notes
             DELETE FROM element_progress;
             DELETE FROM course_notes;
             DELETE FROM enrollments;

             -- Courses & taxonomy
             DELETE FROM element_skill_tags;
             DELETE FROM course_elements;
             DELETE FROM course_chapters;
             DELETE FROM courses;
             DELETE FROM skill_prerequisites;
             DELETE FROM skill_relations;
             DELETE FROM skills;
             DELETE FROM subjects;
             DELETE FROM subject_fields;

             -- App settings (seed-managed keys only)
             DELETE FROM app_settings WHERE key IN (
                 'theme','language','notifications_enabled','auto_sync',
                 'sentinel_camera_enabled','sentinel_keyboard_enabled'
             );

             PRAGMA foreign_keys = ON;",
        )
        .context("Failed to clear seed data")?;
        output::success("Existing data cleared.");

        // Wipe the iroh content store too — seeded blobs (videos, PDFs,
        // downloadables) are content-addressed by their hash, and those
        // hashes were just nulled out of the DB. Leaving the blobs behind
        // just wastes disk and creates orphans; the next seed will re-fetch.
        let iroh_dir = ctx.iroh_dir();
        if iroh_dir.exists() {
            fs::remove_dir_all(&iroh_dir)
                .with_context(|| format!("Failed to remove iroh dir {}", iroh_dir.display()))?;
            output::info(&format!(
                "Cleared iroh content store ({})",
                iroh_dir.display()
            ));
        }
    }

    output::blank();

    match seed::seed_if_empty(&conn) {
        Ok(true) => {
            output::success("Seed data inserted (taxonomy, courses, governance).");
        }
        Ok(false) => {
            output::info("Database already has data — seed skipped.");
            output::faint("Use --force to wipe and re-seed.");
        }
        Err(e) => {
            bail!("Seed failed: {}", e);
        }
    }

    Ok(())
}

// ── Subcommand: status ──────────────────────────────────────────────

fn show_status(ctx: &ProjectContext, password_file: Option<&std::path::Path>) -> Result<()> {
    output::header("Database status");

    // App data dir
    output::kv("App data", &ctx.app_data_dir.display().to_string());

    if !ctx.has_app_data() {
        output::warning("App data directory does not exist (app never launched)");
        return Ok(());
    }

    // Vault status
    if ctx.has_vault() {
        let vault_size = dir_size(&ctx.vault_dir());
        output::kv("Vault", &format!("exists ({})", ByteSize(vault_size)));
    } else {
        output::kv("Vault", "not created");
    }

    // Iroh status
    if ctx.iroh_dir().exists() {
        let iroh_size = dir_size(&ctx.iroh_dir());
        output::kv("Iroh store", &format!("exists ({})", ByteSize(iroh_size)));
    } else {
        output::kv("Iroh store", "not created");
    }

    // Database
    if !ctx.has_db() {
        output::kv("Database", "not created");
        return Ok(());
    }

    let db_meta = fs::metadata(ctx.db_path()).ok();
    let db_size = db_meta
        .map(|m| ByteSize(m.len()).to_string())
        .unwrap_or_default();
    output::kv(
        "Database",
        &format!("{} ({})", ctx.db_path().display(), db_size),
    );

    output::blank();

    // Open encrypted DB to show migration info
    let conn = open_db(ctx, password_file)?;

    ensure_migration_table(&conn)?;
    let current = current_version(&conn);
    let latest = latest_version();
    output::kv("Schema version", &format!("v{} / v{}", current, latest));

    if current < latest {
        output::warning(&format!("{} pending migration(s)", latest - current));
    } else {
        output::kv("Migrations", "up to date");
    }

    // List applied migrations
    let mut stmt =
        conn.prepare("SELECT version, name, applied_at FROM _migrations ORDER BY version")?;
    let rows: Vec<(i64, String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();

    if !rows.is_empty() {
        output::blank();
        output::header("Applied migrations");
        for (v, name, applied) in &rows {
            output::kv(&format!("v{}", v), &format!("{:<30} {}", name, applied));
        }
    }

    // Pending migrations
    let pending: Vec<_> = schema::MIGRATIONS
        .iter()
        .filter(|(v, _, _)| *v > current)
        .collect();
    if !pending.is_empty() {
        output::blank();
        output::header("Pending migrations");
        for (v, name, _) in pending {
            output::kv(&format!("v{}", v), name);
        }
    }

    // Table summary
    output::blank();
    output::header("Table row counts");

    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' \
         AND name NOT LIKE '\\_migrations' ESCAPE '\\' \
         AND name NOT LIKE 'sqlite_%' \
         ORDER BY name",
    )?;
    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    if tables.is_empty() {
        output::faint("No tables found");
    } else {
        for table in &tables {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM [{}]", table), [], |row| {
                    row.get(0)
                })
                .unwrap_or(0);
            output::kv(table, &count.to_string());
        }
    }

    output::blank();
    Ok(())
}

// ── Subcommand: reset ───────────────────────────────────────────────

fn reset_data(ctx: &ProjectContext, force: bool) -> Result<()> {
    output::header("Reset app data");

    if !ctx.has_app_data() {
        output::info("No app data directory found — nothing to reset");
        return Ok(());
    }

    if !force {
        output::error(
            "This will delete ALL app data (database, vault, iroh store).\n\
             Pass --force to confirm.",
        );
        output::blank();
        output::faint(&format!("  Directory: {}", ctx.app_data_dir.display()));
        return Ok(());
    }

    output::warning(&format!("Deleting {}", ctx.app_data_dir.display()));

    fs::remove_dir_all(&ctx.app_data_dir).context("Failed to remove app data directory")?;

    output::success("App data reset. Launch the app to create a new wallet and re-initialize.");
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Recursively calculate the size of a directory
fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let meta = entry.metadata();
            if let Ok(m) = meta {
                if m.is_dir() {
                    total += dir_size(&entry.path());
                } else {
                    total += m.len();
                }
            }
        }
    }
    total
}
