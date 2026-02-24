use anyhow::Result;
use clap::Subcommand;

use crate::context::ProjectContext;
use crate::output;
use crate::runner;

#[derive(Subcommand)]
pub enum DevCommand {
    /// Launch the app in dev mode (cargo tauri dev)
    Run,
    /// Type-check the Vue frontend (vue-tsc -b)
    Check,
    /// Run all Rust tests (cargo test)
    Test,
    /// Run clippy linter on Rust code
    Clippy,
    /// Check Rust formatting (cargo fmt --check)
    Fmt,
    /// Run everything: fmt, clippy, test, check
    All,
}

pub fn execute(cmd: &DevCommand, ctx: &ProjectContext) -> Result<()> {
    match cmd {
        DevCommand::Run => run_dev(ctx),
        DevCommand::Check => run_check(ctx),
        DevCommand::Test => run_test(ctx),
        DevCommand::Clippy => run_clippy(ctx),
        DevCommand::Fmt => run_fmt(ctx),
        DevCommand::All => run_all(ctx),
    }
}

fn run_dev(ctx: &ProjectContext) -> Result<()> {
    output::header("Starting dev server");
    output::info("Running cargo tauri dev...");
    output::faint("Press Ctrl+C to stop");
    output::blank();

    runner::run_step(&ctx.root, "cargo", &["tauri", "dev"])?;
    Ok(())
}

fn run_check(ctx: &ProjectContext) -> Result<()> {
    output::header("Type-checking frontend");

    if !runner::command_exists("vue-tsc") {
        output::warning("vue-tsc not found globally, using npx...");
        runner::run_step(&ctx.root, "npx", &["vue-tsc", "-b"])?;
    } else {
        runner::run_step(&ctx.root, "vue-tsc", &["-b"])?;
    }

    output::success("Frontend type check passed");
    Ok(())
}

fn run_test(ctx: &ProjectContext) -> Result<()> {
    output::header("Running Rust tests");
    runner::run_step(&ctx.tauri_dir, "cargo", &["test"])?;
    output::success("All tests passed");
    Ok(())
}

fn run_clippy(ctx: &ProjectContext) -> Result<()> {
    output::header("Running clippy");
    runner::run_step(&ctx.tauri_dir, "cargo", &["clippy", "--", "-D", "warnings"])?;
    output::success("Clippy passed (no warnings)");
    Ok(())
}

fn run_fmt(ctx: &ProjectContext) -> Result<()> {
    output::header("Checking Rust formatting");
    runner::run_step(&ctx.tauri_dir, "cargo", &["fmt", "--check"])?;
    output::success("Formatting OK");
    Ok(())
}

fn run_all(ctx: &ProjectContext) -> Result<()> {
    let steps = [
        (
            "Checking formatting",
            run_fmt as fn(&ProjectContext) -> Result<()>,
        ),
        ("Running clippy", run_clippy),
        ("Running tests", run_test),
        ("Type-checking frontend", run_check),
    ];
    let total = steps.len();

    for (i, (label, func)) in steps.iter().enumerate() {
        output::step(i + 1, total, label);
        func(ctx)?;
    }

    output::blank();
    output::success("All checks passed!");
    Ok(())
}
