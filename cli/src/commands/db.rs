use anyhow::{Context, Result};
use bytesize::ByteSize;
use clap::Subcommand;
use rusqlite::Connection;
use std::fs;

use crate::context::ProjectContext;
use crate::output;

#[derive(Subcommand)]
pub enum DbCommand {
    /// Show database status (tables, row counts, size)
    Status,
    /// Reset all app data (database + vault + iroh). Requires --force.
    Reset {
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
}

pub fn execute(cmd: &DbCommand, ctx: &ProjectContext) -> Result<()> {
    match cmd {
        DbCommand::Status => show_status(ctx),
        DbCommand::Reset { force } => reset_data(ctx, *force),
    }
}

fn show_status(ctx: &ProjectContext) -> Result<()> {
    output::header("Database status");

    // App data dir
    output::kv("App data", &ctx.app_data_dir.display().to_string());

    if !ctx.has_app_data() {
        output::warning("App data directory does not exist (app never launched)");
        return Ok(());
    }

    // Vault status
    if ctx.has_vault() {
        let meta = fs::metadata(ctx.vault_path()).ok();
        let size = meta
            .map(|m| ByteSize(m.len()).to_string())
            .unwrap_or_default();
        output::kv("Vault", &format!("exists ({})", size));
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

    // Open DB and show table counts
    let conn = Connection::open(ctx.db_path()).context("Failed to open database")?;

    // Get migration version
    let version: Result<i64, _> =
        conn.query_row("SELECT MAX(version) FROM _migrations", [], |row| row.get(0));
    match version {
        Ok(v) => output::kv("Migration", &format!("v{}", v)),
        Err(_) => output::kv("Migration", "unknown"),
    }

    output::blank();
    output::header("Table row counts");

    // Query all user-created tables (escape _ since it's a LIKE wildcard in SQLite)
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

    output::success("App data reset. Run the app to re-initialize and re-seed.");
    Ok(())
}

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
