use anyhow::Result;
use clap::Subcommand;

use crate::context::ProjectContext;
use crate::output;
use crate::runner;

type DevStep = fn(&ProjectContext) -> Result<()>;

#[derive(Subcommand)]
pub enum DevCommand {
    /// Launch the app in dev mode (cargo tauri dev)
    Run,
    /// Type-check the Vue frontend (vue-tsc -b)
    Check,
    /// Run Rust tests with the host tutoring media feature enabled
    Test,
    /// Run clippy with the host tutoring media feature enabled
    Clippy,
    /// Check Rust formatting (cargo fmt --check)
    Fmt,
    /// Run everything: fmt, clippy, test, check
    All,
    /// Run the exact checks CI runs on push/PR — covers both `src-tauri`
    /// and `cli` crates plus the frontend. Use this before `git push`
    /// to avoid red builds on main.
    Ci,
}

pub fn execute(cmd: &DevCommand, ctx: &ProjectContext) -> Result<()> {
    match cmd {
        DevCommand::Run => run_dev(ctx),
        DevCommand::Check => run_check(ctx),
        DevCommand::Test => run_test(ctx),
        DevCommand::Clippy => run_clippy(ctx),
        DevCommand::Fmt => run_fmt(ctx),
        DevCommand::All => run_all(ctx),
        DevCommand::Ci => run_ci(ctx),
    }
}

fn host_tutoring_feature() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        Some("tutoring-video-static")
    } else if cfg!(target_os = "macos") {
        Some("tutoring-video-aec")
    } else if cfg!(target_os = "windows") {
        Some("tutoring-video")
    } else {
        None
    }
}

fn extend_with_host_tutoring_feature(args: &mut Vec<&'static str>) {
    if let Some(feature) = host_tutoring_feature() {
        args.extend(["--features", feature]);
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
    let mut args = vec!["test"];
    extend_with_host_tutoring_feature(&mut args);
    runner::run_step(&ctx.tauri_dir, "cargo", &args)?;
    output::success("All tests passed");
    Ok(())
}

fn run_clippy(ctx: &ProjectContext) -> Result<()> {
    output::header("Running clippy");
    let mut args = vec!["clippy"];
    extend_with_host_tutoring_feature(&mut args);
    args.extend(["--", "-D", "warnings"]);
    runner::run_step(&ctx.tauri_dir, "cargo", &args)?;
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

/// Mirror CI: catch everything CI would catch, before pushing.
/// Covers both `src-tauri` and `cli` crates + strict vue-tsc with
/// `--noEmit`. Stops at the first failure.
fn run_ci(ctx: &ProjectContext) -> Result<()> {
    let steps: [(&str, DevStep); 6] = [
        ("Workspace formatting (cargo fmt --check)", ci_workspace_fmt),
        ("src-tauri clippy (-D warnings)", run_clippy),
        ("CLI clippy (-D warnings)", ci_cli_clippy),
        ("src-tauri tests", run_test),
        ("CLI tests", ci_cli_test),
        ("Frontend type-check (vue-tsc --noEmit)", ci_vue_tsc_strict),
    ];
    let total = steps.len();

    for (i, (label, func)) in steps.iter().enumerate() {
        output::step(i + 1, total, label);
        func(ctx)?;
    }

    output::blank();
    output::success("All CI checks passed. Safe to push.");
    Ok(())
}

fn ci_workspace_fmt(ctx: &ProjectContext) -> Result<()> {
    // Run from workspace root so both src-tauri and cli are formatted.
    runner::run_step(&ctx.root, "cargo", &["fmt", "--check"])?;
    Ok(())
}

fn ci_cli_clippy(ctx: &ProjectContext) -> Result<()> {
    let cli_dir = ctx.root.join("cli");
    runner::run_step(&cli_dir, "cargo", &["clippy", "--", "-D", "warnings"])?;
    output::success("CLI clippy passed (no warnings)");
    Ok(())
}

fn ci_cli_test(ctx: &ProjectContext) -> Result<()> {
    let cli_dir = ctx.root.join("cli");
    runner::run_step(&cli_dir, "cargo", &["test"])?;
    output::success("CLI tests passed");
    Ok(())
}

fn ci_vue_tsc_strict(ctx: &ProjectContext) -> Result<()> {
    // Match CI exactly: `npx vue-tsc -b --noEmit`. `--noEmit` ensures
    // no `.tsbuildinfo` caches can mask type errors across runs.
    runner::run_step(&ctx.root, "npx", &["vue-tsc", "-b", "--noEmit"])?;
    output::success("Frontend type check passed");
    Ok(())
}
