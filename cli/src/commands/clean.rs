use anyhow::{Context, Result};
use bytesize::ByteSize;
use clap::Subcommand;
use std::fs;
use std::path::Path;

use crate::context::ProjectContext;
use crate::output;

#[derive(Subcommand)]
pub enum CleanCommand {
    /// Remove build artifacts (target/, dist/, .vite cache)
    Build,
    /// Remove all app data (database + vault + iroh). Requires --force.
    Data {
        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },
    /// Remove everything (build artifacts + app data). Requires --force.
    All {
        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },
}

pub fn execute(cmd: &CleanCommand, ctx: &ProjectContext) -> Result<()> {
    match cmd {
        CleanCommand::Build => clean_build(ctx),
        CleanCommand::Data { force } => clean_data(ctx, *force),
        CleanCommand::All { force } => {
            clean_build(ctx)?;
            clean_data(ctx, *force)?;
            Ok(())
        }
    }
}

fn clean_build(ctx: &ProjectContext) -> Result<()> {
    output::header("Cleaning build artifacts");

    let targets = [
        ("Rust target", ctx.tauri_dir.join("target")),
        ("Vite dist", ctx.root.join("dist")),
        ("Vite cache", ctx.root.join("node_modules/.vite")),
    ];

    let mut cleaned = false;
    for (label, path) in &targets {
        if path.exists() {
            let size = dir_size(path);
            output::info(&format!("Removing {} ({})", label, ByteSize(size)));
            fs::remove_dir_all(path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
            output::success(&format!("{} removed", label));
            cleaned = true;
        } else {
            output::faint(&format!("{} — not present", label));
        }
    }

    if cleaned {
        output::blank();
        output::success("Build artifacts cleaned");
    } else {
        output::info("Nothing to clean");
    }

    Ok(())
}

fn clean_data(ctx: &ProjectContext, force: bool) -> Result<()> {
    output::header("Cleaning app data");

    if !ctx.has_app_data() {
        output::info("No app data directory — nothing to clean");
        return Ok(());
    }

    if !force {
        output::error(
            "This will delete ALL app data (database, vault, iroh store).\n\
             Pass --force to confirm.",
        );
        output::faint(&format!("  Directory: {}", ctx.app_data_dir.display()));
        return Ok(());
    }

    let size = dir_size(&ctx.app_data_dir);
    output::warning(&format!(
        "Deleting {} ({})",
        ctx.app_data_dir.display(),
        ByteSize(size)
    ));

    fs::remove_dir_all(&ctx.app_data_dir).context("Failed to remove app data directory")?;
    output::success("App data cleaned. Run the app to re-initialize.");
    Ok(())
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(m) = entry.metadata() {
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
