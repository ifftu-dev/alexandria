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

/// ALEXANDRIA at 7 pixels tall, in half-blocks.
///
/// The X is plain diagonals meeting at a centre. An X built from hooked corner
/// pieces reads as a swastika — that shipped once in an earlier box-drawing
/// version of this banner and had to be pulled. Whatever else changes here,
/// the X must not grow right-angle arms.
pub const WORDMARK: [&str; 4] = [
    "▄▀▀▀▄ █     █▀▀▀▀ █   █ ▄▀▀▀▄ █▄  █ █▀▀▀▄ █▀▀▀▄ ▀█▀ ▄▀▀▀▄",
    "█▄▄▄█ █     █▄▄▄   ▀▄▀  █▄▄▄█ █▀▄ █ █   █ █▄▄▄▀  █  █▄▄▄█",
    "█   █ █     █     ▄▀ ▀▄ █   █ █  ██ █   █ █ ▀▄   █  █   █",
    "▀   ▀ ▀▀▀▀▀ ▀▀▀▀▀ ▀   ▀ ▀   ▀ ▀   ▀ ▀▀▀▀  ▀   ▀ ▀▀▀ ▀   ▀",
];

const TAGLINE: &str = "Knowledge belongs to everyone";

/// Brand gradient, taken from the website's icon-tile gradient
/// (`linear-gradient(135deg, primary, cyan)` in `website/assets/css/main.css`).
///
/// `primary` is the dark-mode value (`139 133 255`) rather than the light-mode
/// `79 70 229`: terminals are overwhelmingly dark, and the site itself
/// lightens primary for dark backgrounds for the same reason.
const BRAND_INDIGO: (u8, u8, u8) = (139, 133, 255);
const BRAND_CYAN: (u8, u8, u8) = (34, 211, 238);

/// Colour at a point on the gradient, `t` in `0.0..=1.0`.
pub fn gradient_at(t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    (
        mix(BRAND_INDIGO.0, BRAND_CYAN.0),
        mix(BRAND_INDIGO.1, BRAND_CYAN.1),
        mix(BRAND_INDIGO.2, BRAND_CYAN.2),
    )
}

/// Position along the gradient for a cell, sweeping diagonally.
///
/// Weighted mostly horizontal with a vertical component, which approximates
/// the website's 135° gradients — a purely horizontal ramp looks flat next to
/// them.
pub fn gradient_t(x: usize, y: usize, width: usize, height: usize) -> f32 {
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    (x as f32 / w) * 0.82 + (y as f32 / h) * 0.18
}

/// Whether the terminal advertises 24-bit colour.
///
/// Truecolor escapes on a 256-colour terminal render as the wrong colour or as
/// literal garbage, so the banner falls back to a single ANSI colour rather
/// than assuming.
fn supports_truecolor() -> bool {
    std::env::var("COLORTERM")
        .map(|v| {
            let v = v.to_ascii_lowercase();
            v.contains("truecolor") || v.contains("24bit")
        })
        .unwrap_or(false)
}

/// Paint one row of art, offset by `y` within the whole block.
fn paint(row: &str, y: usize, width: usize, height: usize, truecolor: bool) -> String {
    row.chars()
        .enumerate()
        .map(|(x, ch)| {
            if ch == ' ' {
                return " ".to_string();
            }
            if !truecolor {
                return ch.cyan().to_string();
            }
            let (r, g, b) = gradient_at(gradient_t(x, y, width, height));
            ch.truecolor(r, g, b).to_string()
        })
        .collect()
}

/// Print the Alexandria banner.
///
/// Three tiers, because this prints before every command and a wrapped
/// wordmark is worse than no wordmark: mark plus wordmark when the terminal is
/// wide enough, wordmark alone when it is not, and a single line when stderr
/// is not a terminal at all — a script reading `alexandria path` or a CI log
/// has no use for art.
pub fn banner() {
    if is_json() {
        return;
    }
    if !std::io::stderr().is_terminal() {
        eprintln!("Alexandria CLI v{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let width = WORDMARK[0].chars().count();
    if terminal_width() < width + 4 {
        eprintln!();
        eprintln!("  {}  {}", "⬡ Alexandria".bold(), dim_version());
        eprintln!();
        return;
    }

    let truecolor = supports_truecolor();
    eprintln!();
    for (y, row) in WORDMARK.iter().enumerate() {
        eprintln!("  {}", paint(row, y, width, WORDMARK.len(), truecolor));
    }
    eprintln!("  {}  {}", TAGLINE.dimmed(), dim_version());
    eprintln!();
}

fn dim_version() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
        .dimmed()
        .to_string()
}

/// Terminal width, defaulting wide enough for the full banner when it cannot
/// be determined — the art is the intended default, not the fallback.
fn terminal_width() -> usize {
    // A pty that has never been sized reports Ok((0, 0)) rather than an error,
    // so zero has to be treated as "unknown" too — otherwise the banner
    // silently downgrades to its narrowest form under `script`, in CI, and in
    // anything else that allocates a pty without setting a window size.
    crossterm::terminal::size()
        .ok()
        .map(|(w, _)| w as usize)
        .filter(|w| *w > 0)
        .unwrap_or(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_rows_are_all_the_same_width() {
        // Rows of differing width shear the letterforms apart. Counted in
        // chars, not bytes — every glyph here is multi-byte.
        let widths: Vec<usize> = WORDMARK.iter().map(|r| r.chars().count()).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "wordmark rows differ in width: {widths:?}"
        );
    }

    #[test]
    fn the_art_is_built_only_from_half_blocks() {
        // The structural guard against the swastika that shipped in the first
        // version of this banner. That shape needs hooked corner pieces
        // (╔ ╗ ╚ ╝ ╠ ╣ ╦ ╩ ╬); half-blocks and spaces cannot form it at all.
        // Keeping the alphabet this narrow makes the whole class impossible
        // rather than policing one glyph.
        for row in WORDMARK {
            for ch in row.chars() {
                assert!(
                    matches!(ch, ' ' | '▀' | '▄' | '█'),
                    "wordmark contains `{ch}`, which is not a half-block"
                );
            }
        }
    }

    #[test]
    fn the_banner_fits_a_standard_terminal() {
        // The wordmark plus a two-space indent has to clear 80 columns, or the
        // art wraps and looks worse than no art at all.
        let full = WORDMARK[0].chars().count() + 2;
        assert!(full <= 80, "banner is {full} columns");
    }

    #[test]
    fn gradient_runs_from_brand_indigo_to_cyan_and_clamps() {
        assert_eq!(gradient_at(0.0), BRAND_INDIGO);
        assert_eq!(gradient_at(1.0), BRAND_CYAN);
        // Out-of-range positions must not wrap around to a wild colour.
        assert_eq!(gradient_at(-5.0), BRAND_INDIGO);
        assert_eq!(gradient_at(5.0), BRAND_CYAN);
    }

    #[test]
    fn gradient_sweeps_diagonally() {
        // Both axes must contribute, otherwise the ramp is flat and looks
        // nothing like the website's 135° gradients.
        let (w, h) = (60, 4);
        assert!(
            gradient_t(59, 0, w, h) > gradient_t(0, 0, w, h),
            "no horizontal sweep"
        );
        assert!(
            gradient_t(0, 3, w, h) > gradient_t(0, 0, w, h),
            "no vertical sweep"
        );
        // Horizontal dominates, so the sweep reads left-to-right.
        assert!(gradient_t(59, 0, w, h) > gradient_t(0, 3, w, h));
        // And it stays in range for every cell of the real art.
        let width = WORDMARK[0].chars().count();
        for y in 0..WORDMARK.len() {
            for x in 0..width {
                let t = gradient_t(x, y, width, WORDMARK.len());
                assert!((0.0..=1.0).contains(&t), "t={t} out of range at ({x},{y})");
            }
        }
    }

    #[test]
    fn painting_preserves_the_characters() {
        // Colouring must change how the art looks, never what it draws.
        for truecolor in [true, false] {
            let painted = paint(WORDMARK[0], 0, 60, 4, truecolor);
            for ch in WORDMARK[0].chars().filter(|c| *c != ' ') {
                assert!(painted.contains(ch), "lost `{ch}` (truecolor={truecolor})");
            }
        }
    }
}
