mod commands;
mod context;
mod output;
mod runner;

use anyhow::Result;
use clap::{Parser, Subcommand};

use commands::{build, clean, config, db, dev, health};
use context::ProjectContext;

#[derive(Parser)]
#[command(
    name = "alex",
    about = "Alexandria developer CLI",
    version,
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Development workflow commands
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
        Commands::Dev(cmd) => dev::execute(cmd, &ctx),
        Commands::Db(cmd) => db::execute(cmd, &ctx),
        Commands::Build(cmd) => build::execute(cmd, &ctx),
        Commands::Config(cmd) => config::execute(cmd, &ctx),
        Commands::Health => health::execute(&ctx),
        Commands::Clean(cmd) => clean::execute(cmd, &ctx),
    }
}
