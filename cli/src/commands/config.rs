use anyhow::{Context, Result};
use clap::Subcommand;
use std::fs;

use crate::context::ProjectContext;
use crate::output;

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Show project and app configuration
    Show,
    /// Print the app data directory path
    Path,
}

pub fn execute(cmd: &ConfigCommand, ctx: &ProjectContext) -> Result<()> {
    match cmd {
        ConfigCommand::Show => show_config(ctx),
        ConfigCommand::Path => print_path(ctx),
    }
}

fn show_config(ctx: &ProjectContext) -> Result<()> {
    output::header("Project");
    output::kv("Root", &ctx.root.display().to_string());
    output::kv("Tauri dir", &ctx.tauri_dir.display().to_string());

    // Read tauri.conf.json for basic info
    let conf_path = ctx.tauri_dir.join("tauri.conf.json");
    if conf_path.exists() {
        let conf_str = fs::read_to_string(&conf_path).context("Failed to read tauri.conf.json")?;
        if let Ok(conf) = serde_json::from_str::<serde_json::Value>(&conf_str) {
            if let Some(name) = conf.get("productName").and_then(|v| v.as_str()) {
                output::kv("Product", name);
            }
            if let Some(id) = conf.get("identifier").and_then(|v| v.as_str()) {
                output::kv("Identifier", id);
            }
            if let Some(version) = conf.get("version").and_then(|v| v.as_str()) {
                output::kv("Version", version);
            }
        }
    }

    output::blank();
    output::header("App data");
    output::kv("Directory", &ctx.app_data_dir.display().to_string());
    output::kv("Database", &ctx.db_path().display().to_string());
    output::kv("Vault", &ctx.vault_dir().display().to_string());
    output::kv("Iroh", &ctx.iroh_dir().display().to_string());

    output::blank();
    output::header("Status");
    output::kv(
        "App data exists",
        if ctx.has_app_data() { "yes" } else { "no" },
    );
    output::kv(
        "Database",
        if ctx.has_db() {
            "exists"
        } else {
            "not created"
        },
    );
    output::kv(
        "Vault",
        if ctx.has_vault() {
            "exists"
        } else {
            "not created"
        },
    );
    output::kv(
        "Iroh store",
        if ctx.iroh_dir().exists() {
            "exists"
        } else {
            "not created"
        },
    );

    output::blank();
    output::header("Tools");

    for tool in &["cargo", "rustc", "node", "npm", "vue-tsc"] {
        let version = crate::runner::run_silent(&ctx.root, tool, &["--version"]);
        match version {
            Ok(v) => output::kv(tool, v.trim()),
            Err(_) => output::kv(tool, "not found"),
        }
    }

    // Tauri CLI version
    let tauri_ver = crate::runner::run_silent(&ctx.root, "cargo", &["tauri", "--version"]);
    match tauri_ver {
        Ok(v) => output::kv("tauri-cli", v.trim()),
        Err(_) => output::kv("tauri-cli", "not installed (cargo install tauri-cli)"),
    }

    output::blank();
    Ok(())
}

fn print_path(ctx: &ProjectContext) -> Result<()> {
    // Print to stdout (not stderr) so it can be used in scripts: cd $(alex config path)
    println!("{}", ctx.app_data_dir.display());
    Ok(())
}
