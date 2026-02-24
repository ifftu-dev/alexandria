use anyhow::Result;
use clap::Subcommand;

use crate::context::ProjectContext;
use crate::output;
use crate::runner;

#[derive(Subcommand)]
pub enum BuildCommand {
    /// Run cargo check (fast compile check, no codegen)
    Check,
    /// Full release build (cargo tauri build)
    Release,
}

pub fn execute(cmd: &BuildCommand, ctx: &ProjectContext) -> Result<()> {
    match cmd {
        BuildCommand::Check => run_check(ctx),
        BuildCommand::Release => run_release(ctx),
    }
}

fn run_check(ctx: &ProjectContext) -> Result<()> {
    let steps: &[(&str, &str, &[&str])] = &[
        ("Rust cargo check", "cargo", &["check"]),
        ("Frontend type check", "npx", &["vue-tsc", "-b"]),
    ];

    for (i, (label, prog, args)) in steps.iter().enumerate() {
        output::step(i + 1, steps.len(), label);
        let dir = if *prog == "cargo" {
            &ctx.tauri_dir
        } else {
            &ctx.root
        };
        runner::run_step(dir, prog, args)?;
        output::success(&format!("{} passed", label));
    }

    output::blank();
    output::success("All checks passed!");
    Ok(())
}

fn run_release(ctx: &ProjectContext) -> Result<()> {
    output::header("Building release");
    output::info("This will compile the full Tauri app bundle...");
    output::faint("(includes vue-tsc + vite build + cargo build --release)");
    output::blank();

    runner::run_step(&ctx.root, "cargo", &["tauri", "build"])?;

    output::blank();
    output::success("Release build complete!");
    output::faint("Bundle output: src-tauri/target/release/bundle/");
    Ok(())
}
