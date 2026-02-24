use anyhow::{bail, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

/// Run a command with real-time output streaming, returning its exit status.
pub fn run(dir: &Path, program: &str, args: &[&str]) -> Result<ExitStatus> {
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(status)
}

/// Run a command silently, capturing its output.
pub fn run_silent(dir: &Path, program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program).args(args).current_dir(dir).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "`{} {}` failed (exit {}):\n{}{}",
            program,
            args.join(" "),
            output.status.code().unwrap_or(-1),
            stdout,
            stderr
        );
    }
    Ok(stdout)
}

/// Run a command and bail if it fails, with a step message.
pub fn run_step(dir: &Path, program: &str, args: &[&str]) -> Result<()> {
    let status = run(dir, program, args)?;
    if !status.success() {
        bail!(
            "`{} {}` failed with exit code {}",
            program,
            args.join(" "),
            status.code().unwrap_or(-1),
        );
    }
    Ok(())
}

/// Check if a command exists on PATH
pub fn command_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Create a spinner with the given message
#[allow(dead_code)]
pub fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("  {spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}
