mod android_env;
mod commands;
mod context;
mod output;
mod runner;
mod tauri_config;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use commands::{build, clean, config, db, dev, health, run};
use context::ProjectContext;

#[derive(Parser)]
#[command(
    name = "alex",
    about = "Alexandria developer CLI",
    version,
    propagate_version = true
)]
struct Cli {
    /// Read vault password from a file instead of prompting interactively.
    /// Useful for CI/CD pipelines and scripted operations.
    #[arg(long, global = true)]
    password_file: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the app on desktop, iOS, or Android (with device picker)
    #[command(subcommand)]
    Run(run::RunCommand),

    /// Development workflow commands (test, lint, fmt)
    #[command(subcommand)]
    Dev(dev::DevCommand),

    /// Database and app data operations
    #[command(subcommand)]
    Db(db::DbCommand),

    /// Build and compile operations
    #[command(subcommand)]
    Build(build::BuildCommand),

    /// Project and environment configuration
    #[command(subcommand)]
    Config(config::ConfigCommand),

    /// Check if the app and services are running
    Health,

    /// Clean build artifacts and app data
    #[command(subcommand)]
    Clean(clean::CleanCommand),
}

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        output::blank();
        output::error(&format!("{:#}", e));
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    output::banner();

    let ctx = ProjectContext::detect()?;

    match &cli.command {
        Commands::Run(cmd) => run::execute(cmd, &ctx),
        Commands::Dev(cmd) => dev::execute(cmd, &ctx),
        Commands::Db(cmd) => db::execute(cmd, &ctx, cli.password_file.as_deref()),
        Commands::Build(cmd) => build::execute(cmd, &ctx),
        Commands::Config(cmd) => config::execute(cmd, &ctx),
        Commands::Health => health::execute(&ctx),
        Commands::Clean(cmd) => clean::execute(cmd, &ctx),
    }
}
