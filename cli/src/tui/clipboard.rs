//! Putting text on the system clipboard from inside the TUI.
//!
//! Two mechanisms, tried in order.
//!
//! A platform helper (`pbcopy`, `wl-copy`, `xclip`, `clip`) is tried first
//! because it is unambiguous: it either runs or it does not, and the text
//! lands on the clipboard of the machine the user is sitting at.
//!
//! OSC 52 is the fallback. It asks the *terminal* to set the clipboard, which
//! is the only thing that works over SSH — the helper would otherwise copy to
//! the remote machine's clipboard, where nobody can paste from it. Support is
//! not universal (Terminal.app on macOS ignores it) and there is no reply to
//! read, so it can fail silently; that is why it is second rather than first.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{bail, Result};

/// How the text reached the clipboard, for reporting back to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// A platform helper ran and accepted the text.
    Helper(&'static str),
    /// An OSC 52 sequence was emitted. The terminal may or may not honour it.
    Osc52,
}

impl Method {
    pub fn describe(self) -> String {
        match self {
            Method::Helper(name) => format!("copied via {name}"),
            // Honest about the uncertainty: we cannot confirm this one.
            Method::Osc52 => "copy sent to the terminal (OSC 52)".to_string(),
        }
    }
}

/// Helpers to try, in order, for the current platform.
fn helpers() -> &'static [(&'static str, &'static [&'static str])] {
    #[cfg(target_os = "macos")]
    {
        &[("pbcopy", &[])]
    }
    #[cfg(target_os = "windows")]
    {
        &[("clip", &[])]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &[
            // Wayland first: on a Wayland session xclip may exist but talk to
            // an Xwayland clipboard the user cannot paste from.
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    }
}

/// Copy `text` to the clipboard.
pub fn copy(text: &str) -> Result<Method> {
    for (program, args) in helpers() {
        match try_helper(program, args, text) {
            Ok(()) => return Ok(Method::Helper(program)),
            // Not installed, or it failed — fall through to the next.
            Err(_) => continue,
        }
    }

    write_osc52(text)?;
    Ok(Method::Osc52)
}

fn try_helper(program: &str, args: &[&str], text: &str) -> Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    // Drop stdin before waiting: these helpers read until EOF, so holding the
    // pipe open would deadlock.
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("{program} has no stdin"))?;
        stdin.write_all(text.as_bytes())?;
    }

    if child.wait()?.success() {
        Ok(())
    } else {
        bail!("{program} exited non-zero")
    }
}

/// Emit `ESC ] 52 ; c ; <base64> BEL` on stdout.
fn write_osc52(text: &str) -> Result<()> {
    let encoded = base64(text.as_bytes());
    let mut out = std::io::stdout();
    write!(out, "\x1b]52;c;{encoded}\x07")?;
    out.flush()?;
    Ok(())
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
///
/// Hand-rolled rather than pulling in a dependency for one call site — this is
/// the only thing in the CLI that needs it, and the encoder is small enough to
/// verify against known vectors in a test.
fn base64(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18 & 63) as usize] as char);
        out.push(B64[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc4648_vectors() {
        // From RFC 4648 §10 — these cover every padding case.
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_bytes_above_ascii() {
        // A credential carries UTF-8; encoding must be over bytes, not chars.
        assert_eq!(base64("é".as_bytes()), "w6k=");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn base64_output_length_is_always_a_multiple_of_four() {
        // Terminals reject a malformed OSC 52 payload outright.
        for len in 0..64 {
            let input = vec![b'x'; len];
            assert_eq!(base64(&input).len() % 4, 0, "length {len}");
        }
    }

    /// Round-trips through the real system clipboard, so it is `#[ignore]`d:
    /// CI has no clipboard, and a test run should not clobber whatever the
    /// developer had copied. Run explicitly with
    /// `cargo test clipboard_round_trip -- --ignored`.
    #[test]
    #[ignore]
    fn clipboard_round_trip() {
        use std::process::Command;

        let read = || -> String {
            let out = Command::new(if cfg!(target_os = "macos") {
                "pbpaste"
            } else {
                "xclip"
            })
            .args(if cfg!(target_os = "macos") {
                Vec::new()
            } else {
                vec!["-selection", "clipboard", "-o"]
            })
            .output()
            .expect("read clipboard");
            String::from_utf8_lossy(&out.stdout).to_string()
        };

        let saved = read();
        let payload = "{\n  \"id\": \"urn:uuid:round-trip\"\n}";

        let method = copy(payload).expect("copy");
        assert!(
            matches!(method, Method::Helper(_)),
            "expected a platform helper on a desktop session, got {method:?}"
        );
        assert_eq!(
            read(),
            payload,
            "the clipboard does not hold what was copied"
        );

        // Put back whatever the developer had.
        let _ = copy(&saved);
    }

    #[test]
    fn method_describes_osc52_without_claiming_success() {
        // There is no reply to an OSC 52 sequence, so the wording must not
        // promise the text actually reached the clipboard.
        let text = Method::Osc52.describe();
        assert!(text.contains("OSC 52"));
        assert!(!text.contains("copied to clipboard"));
        assert_eq!(Method::Helper("pbcopy").describe(), "copied via pbcopy");
    }
}
