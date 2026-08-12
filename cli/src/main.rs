mod android_env;
mod commands;
mod context;
mod output;
mod runner;
mod synth;
mod tauri_config;
mod tui;
mod vault;

use std::io;
use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use commands::{
    clean, credentials, db, doctor, presentation, role_assessment, run, synth_sentinel,
};
use context::ProjectContext;

#[derive(Parser)]
#[command(
    name = "alexandria",
    about = "Alexandria developer CLI",
    version,
    propagate_version = true
)]
struct Cli {
    /// Read vault password from a file instead of prompting interactively.
    /// Useful for CI/CD pipelines and scripted operations.
    #[arg(long, global = true)]
    password_file: Option<PathBuf>,

    /// Emit the command's result as JSON on stdout instead of decorated
    /// text. Human output goes to stderr, so `alexandria --json … > out.json`
    /// captures exactly the result document.
    #[arg(long, global = true)]
    json: bool,

    /// Which profile's data to operate on — a profile id, an id prefix, or a
    /// display name. Defaults to the most recently unlocked profile, which is
    /// the one the app itself opens.
    #[arg(long, global = true)]
    profile: Option<String>,

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

    /// Issue, inspect, import, export, and verify Verifiable Credentials
    #[command(subcommand, alias = "vc")]
    Credentials(credentials::CredentialsCommand),

    /// Verify Verifiable Presentations
    #[command(subcommand, name = "vp")]
    Presentation(presentation::PresentationCommand),

    /// Organizations, role assessments, and role credential issuance
    #[command(subcommand, name = "role-assessment", alias = "ra")]
    RoleAssessment(role_assessment::RoleAssessmentCommand),

    /// Launch the interactive terminal UI
    Tui,

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

    /// Print a shell completion script (bash, zsh, fish, elvish, powershell)
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

/// Whether this invocation is going to print help rather than do work.
///
/// Clap handles help inside `parse()` and exits there, so anything that should
/// accompany it has to happen first. Checked by scanning the raw arguments,
/// because by the time there is a parsed `Cli`, clap has already printed and
/// gone.
///
/// `--version` is deliberately excluded. The banner already carries the
/// version, so printing both says it twice, and `-V` is the one thing here a
/// script is most likely to parse.
fn is_help(args: &[String]) -> bool {
    // No arguments at all is clap's help too, since every path needs a
    // subcommand.
    if args.len() <= 1 {
        return true;
    }
    args.iter()
        .any(|a| matches!(a.as_str(), "-h" | "--help" | "help"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // The banner belongs with help: help is where somebody meets this tool for
    // the first time, and it is the one place the wordmark is doing a job
    // rather than decorating a log line. Printed before `parse()` because clap
    // prints help and exits without returning.
    if is_help(&args) {
        output::banner();
    }

    let cli = Cli::parse();
    output::set_json(cli.json);

    if let Err(e) = run(cli) {
        output::blank();
        output::fatal(&format!("{:#}", e));
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    // Completions must emit nothing but the script, and they work outside a
    // project, so they are handled before the banner and root detection.
    if let Commands::Completions { shell } = &cli.command {
        let mut cmd = Cli::command();
        let name = cmd.get_name().to_string();
        clap_complete::generate(*shell, &mut cmd, name, &mut io::stdout());
        return Ok(());
    }

    // Not before the TUI. The TUI takes over the alternate screen, so a banner
    // printed here is invisible for the whole session and then reappears intact
    // the moment the alternate screen is dropped — which reads as the CLI
    // printing a banner *after* you quit, which is exactly what it looked
    // like.
    if !matches!(cli.command, Commands::Tui) {
        output::banner();
    }

    let ctx = ProjectContext::detect(cli.profile.as_deref())?;
    let password_file = cli.password_file.as_deref();

    match &cli.command {
        Commands::Run(cmd) => run::execute(cmd, &ctx),
        Commands::Db(cmd) => db::execute(cmd, &ctx, password_file),
        Commands::Credentials(cmd) => credentials::execute(cmd, &ctx, password_file),
        Commands::Presentation(cmd) => presentation::execute(cmd, &ctx, password_file),
        Commands::RoleAssessment(cmd) => role_assessment::execute(cmd, &ctx, password_file),
        Commands::Tui => tui::run(&ctx, password_file),
        Commands::Doctor(args) => doctor::execute(args, &ctx),
        Commands::Path => doctor::print_path(&ctx),
        Commands::Clean(cmd) => clean::execute(cmd, &ctx),
        Commands::SynthSentinel(cmd) => synth_sentinel::execute(cmd),
        // Handled above, before project detection.
        Commands::Completions { .. } => unreachable!(),
    }
}
