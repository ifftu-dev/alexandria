//! Terminal output.
//!
//! Two modes. In the default human mode the helpers below write decorated
//! text to **stderr**, leaving stdout free for the one thing a caller might
//! want to pipe (a credential's JSON, a path). Under `--json` the human
//! helpers fall silent and commands emit a single machine-readable document
//! to **stdout** via [`emit`].
//!
//! The split matters: `alexandria path` and `alexandria credentials get` were already
//! stdout-clean, so `cd $(alexandria path)` works. `--json` generalizes that to
//! every command rather than bolting a second output system beside it.

use std::sync::atomic::{AtomicBool, Ordering};

use std::io::IsTerminal;

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

// ---- Wordmark -----------------------------------------------------------

/// The Alexandria wordmark.
///
/// Drawn in box-drawing characters rather than a solid block font so it sits
/// in the same visual family as the TUI's borders, and so it stays 39 columns
/// wide — narrow enough to survive a split terminal, where a block wordmark
/// would wrap into noise.
pub const WORDMARK: [&str; 3] = [
    "╔═╗ ╦   ╔═╗ ═╗ ╦ ╔═╗ ╔╗╔ ╔╦╗ ╦═╗ ╦ ╔═╗",
    "╠═╣ ║   ║╣  ╔╩╦╝ ╠═╣ ║║║  ║║ ╠╦╝ ║ ╠═╣",
    "╩ ╩ ╩═╝ ╚═╝ ╩ ╚═ ╩ ╩ ╝╚╝ ═╩╝ ╩╚═ ╩ ╩ ╩",
];

/// Gradient endpoints, cyan → violet.
const GRADIENT_FROM: (u8, u8, u8) = (34, 211, 238);
const GRADIENT_TO: (u8, u8, u8) = (167, 139, 250);

/// Colour for a character at horizontal position `t` in `0.0..=1.0`.
pub fn gradient_at(t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    (
        mix(GRADIENT_FROM.0, GRADIENT_TO.0),
        mix(GRADIENT_FROM.1, GRADIENT_TO.1),
        mix(GRADIENT_FROM.2, GRADIENT_TO.2),
    )
}

/// Render one wordmark row with a horizontal gradient.
///
/// The gradient runs across the *whole* wordmark rather than per row, so the
/// three rows line up into one continuous sweep.
fn gradient_row(row: &str) -> String {
    let width = WORDMARK[0].chars().count().max(1) as f32;
    row.chars()
        .enumerate()
        .map(|(i, ch)| {
            if ch == ' ' {
                return " ".to_string();
            }
            let (r, g, b) = gradient_at(i as f32 / width);
            ch.truecolor(r, g, b).to_string()
        })
        .collect()
}

/// Print the Alexandria banner.
///
/// The full wordmark is for a human at a terminal. Piped or redirected — a
/// script reading `alexandria path`, a CI log — it collapses to one line,
/// because three rows of box-drawing characters in a build log is vandalism.
pub fn banner() {
    if is_json() {
        return;
    }
    if std::io::stderr().is_terminal() {
        wordmark_banner();
    } else {
        eprintln!("Alexandria CLI v{}", env!("CARGO_PKG_VERSION"));
    }
}

fn wordmark_banner() {
    let version = env!("CARGO_PKG_VERSION");
    eprintln!();
    for row in WORDMARK {
        eprintln!("  {}", gradient_row(row));
    }
    eprintln!(
        "   {}  {}  {}",
        "⬡".truecolor(GRADIENT_TO.0, GRADIENT_TO.1, GRADIENT_TO.2),
        "learning you own".dimmed(),
        format!("v{version}").dimmed()
    );
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordmark_rows_are_all_the_same_width() {
        // Rows of differing width shear the letterforms apart. Counted in
        // chars, not bytes — every glyph here is multi-byte.
        let widths: Vec<usize> = WORDMARK.iter().map(|r| r.chars().count()).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "wordmark rows differ in width: {widths:?}"
        );
    }

    #[test]
    fn wordmark_fits_a_narrow_terminal() {
        // It is printed with a two-space indent and shown inside a bordered
        // TUI modal, so it has to leave room for both.
        assert!(
            WORDMARK[0].chars().count() <= 44,
            "wordmark is {} columns",
            WORDMARK[0].chars().count()
        );
    }

    #[test]
    fn gradient_runs_from_cyan_to_violet_and_clamps() {
        assert_eq!(gradient_at(0.0), GRADIENT_FROM);
        assert_eq!(gradient_at(1.0), GRADIENT_TO);
        // Out-of-range positions must not wrap around to a wild colour.
        assert_eq!(gradient_at(-5.0), GRADIENT_FROM);
        assert_eq!(gradient_at(5.0), GRADIENT_TO);
        // The midpoint sits between the endpoints on every channel.
        let mid = gradient_at(0.5);
        assert!(mid.0 > GRADIENT_FROM.0 && mid.0 < GRADIENT_TO.0);
        assert!(mid.2 > GRADIENT_FROM.2 && mid.2 < GRADIENT_TO.2);
    }

    #[test]
    fn gradient_row_preserves_the_characters() {
        // Colouring must not change what is drawn, only how it looks.
        let colored = gradient_row(WORDMARK[0]);
        let stripped: String = colored
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .replace(
                |c: char| c == '[' || c == 'm' || c.is_ascii_digit() || c == ';',
                "",
            );
        for ch in WORDMARK[0].chars().filter(|c| *c != ' ') {
            assert!(stripped.contains(ch), "lost `{ch}` while colouring");
        }
    }
}
