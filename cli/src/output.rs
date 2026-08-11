//! Terminal output.
//!
//! Two modes. In the default human mode the helpers below write decorated
//! text to **stderr**, leaving stdout free for the one thing a caller might
//! want to pipe (a credential's JSON, a path). Under `--json` the human
//! helpers fall silent and commands emit a single machine-readable document
//! to **stdout** via [`emit`].
//!
//! The split matters: `alex path` and `alex credentials get` were already
//! stdout-clean, so `cd $(alex path)` works. `--json` generalizes that to
//! every command rather than bolting a second output system beside it.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use owo_colors::OwoColorize;
use serde::Serialize;

static JSON_MODE: AtomicBool = AtomicBool::new(false);

/// Switch every human-facing helper into silence and route results through
/// [`emit`]. Set once from `main` before any command runs.
pub fn set_json(on: bool) {
    JSON_MODE.store(on, Ordering::Relaxed);
}

pub fn is_json() -> bool {
    JSON_MODE.load(Ordering::Relaxed)
}

/// Serialize `value` as the command's result document on stdout.
///
/// A no-op in human mode, so a command can call this unconditionally after
/// printing its human rendering — whichever mode is active, exactly one of
/// the two paths produces output.
pub fn emit<T: Serialize>(value: &T) -> Result<()> {
    if !is_json() {
        return Ok(());
    }
    let text = serde_json::to_string_pretty(value).context("serialize result as JSON")?;
    println!("{text}");
    Ok(())
}

/// Report a fatal error. JSON mode gets `{"error": "..."}` on stderr so a
/// script can distinguish a failure document from a result document by
/// stream, without parsing stdout that may never have been written.
pub fn fatal(msg: &str) {
    if is_json() {
        let doc = serde_json::json!({ "error": msg });
        eprintln!("{}", serde_json::to_string_pretty(&doc).unwrap_or_default());
    } else {
        eprintln!("  {} {}", "✗".red().bold(), msg);
    }
}

/// Print a success message with green checkmark
pub fn success(msg: &str) {
    if is_json() {
        return;
    }
    eprintln!("  {} {}", "✓".green().bold(), msg);
}

/// Print a warning message with yellow exclamation
pub fn warning(msg: &str) {
    if is_json() {
        return;
    }
    eprintln!("  {} {}", "!".yellow().bold(), msg);
}

/// Print an error message with red X
pub fn error(msg: &str) {
    if is_json() {
        return;
    }
    eprintln!("  {} {}", "✗".red().bold(), msg);
}

/// Print an info message with cyan arrow
pub fn info(msg: &str) {
    if is_json() {
        return;
    }
    eprintln!("  {} {}", "→".cyan().bold(), msg);
}

/// Print a header/section title
pub fn header(msg: &str) {
    if is_json() {
        return;
    }
    eprintln!("\n  {}", msg.bold());
}

/// Print a faint/dimmed message
pub fn faint(msg: &str) {
    if is_json() {
        return;
    }
    eprintln!("    {}", msg.dimmed());
}

/// Print a numbered step indicator
#[allow(dead_code)]
pub fn step(n: usize, total: usize, msg: &str) {
    if is_json() {
        return;
    }
    eprintln!("  {} {}", format!("[{}/{}]", n, total).cyan().bold(), msg);
}

/// Print a key-value pair, aligned
pub fn kv(key: &str, value: &str) {
    if is_json() {
        return;
    }
    eprintln!("  {:>16}  {}", key.dimmed(), value);
}

/// Print a blank line to stderr
pub fn blank() {
    if is_json() {
        return;
    }
    eprintln!();
}

/// Print the Alexandria banner
pub fn banner() {
    if is_json() {
        return;
    }
    let version = env!("CARGO_PKG_VERSION");
    eprintln!();
    eprintln!(
        "  {}  {}",
        "⬡ Alexandria".bold(),
        format!("v{}", version).dimmed()
    );
    eprintln!("  {}", "Developer CLI".dimmed());
    eprintln!();
}
