mod android_env;
mod commands;
mod context;
mod output;
mod runner;
mod synth;
mod tauri_config;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use commands::{clean, credentials, db, doctor, run, synth_sentinel};
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
    /// Run the app on iOS or Android (with device picker)
    #[command(subcommand)]
    Run(run::RunCommand),

    /// Database and app data operations
    #[command(subcommand)]
    Db(db::DbCommand),

    /// Inspect, export, and verify Verifiable Credentials
    #[command(subcommand)]
    Credentials(credentials::CredentialsCommand),

    /// Diagnose the project, app data, toolchain, and mobile prerequisites
    Doctor(doctor::DoctorArgs),

    /// Print the app data directory path (for scripting)
    Path,

    /// Clean build artifacts and app data
    #[command(subcommand)]
    Clean(clean::CleanCommand),

    /// Generate synthetic adversarial-prior data for the Sentinel paste classifier
    #[command(subcommand, name = "synth-sentinel")]
    SynthSentinel(synth_sentinel::SynthSentinelCommand),
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
        Commands::Db(cmd) => db::execute(cmd, &ctx, cli.password_file.as_deref()),
        Commands::Credentials(cmd) => credentials::execute(cmd, &ctx, cli.password_file.as_deref()),
        Commands::Doctor(args) => doctor::execute(args, &ctx),
        Commands::Path => doctor::print_path(&ctx),
        Commands::Clean(cmd) => clean::execute(cmd, &ctx),
        Commands::SynthSentinel(cmd) => synth_sentinel::execute(cmd),
    }
}
