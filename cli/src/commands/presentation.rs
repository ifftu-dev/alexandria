//! `alexandria vp` — Verifiable Presentation verification.
//!
//! The companion to `alexandria credentials verify`: that one checks a
//! survivability bundle offline, this one checks a presentation an
//! individual handed to a verifier, including the audience binding that
//! stops a presentation made for one party being replayed at another.
//!
//! Verification needs the database because replay detection consults the
//! nonce store — an accepted presentation is only accepted once.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use serde_json::json;

use crate::context::ProjectContext;
use crate::output;
use crate::vault;

#[derive(Subcommand)]
pub enum PresentationCommand {
    /// Verify a presentation envelope against an expected audience
    Verify {
        /// Path to a presentation envelope JSON file, or `-` for stdin
        envelope: PathBuf,

        /// The audience the presentation must be bound to (a verifier
        /// identifier). A presentation built for someone else is rejected
        /// as an audience mismatch rather than silently accepted.
        #[arg(long)]
        audience: String,
    },
}

pub fn execute(
    cmd: &PresentationCommand,
    ctx: &ProjectContext,
    password_file: Option<&Path>,
) -> Result<()> {
    match cmd {
        PresentationCommand::Verify { envelope, audience } => {
            run_verify(ctx, password_file, envelope, audience)
        }
    }
}

fn run_verify(
    ctx: &ProjectContext,
    password_file: Option<&Path>,
    envelope_path: &Path,
    audience: &str,
) -> Result<()> {
    output::header("Verify presentation");
    output::kv("Envelope", &envelope_path.display().to_string());
    output::kv("Audience", audience);

    let raw = if envelope_path.as_os_str() == "-" {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("read envelope from stdin")?;
        buf
    } else {
        fs::read_to_string(envelope_path)
            .with_context(|| format!("read envelope at {}", envelope_path.display()))?
    };

    let envelope: app_lib::commands::presentation::PresentationEnvelope =
        serde_json::from_str(&raw).context("parse PresentationEnvelope JSON")?;

    let conn = vault::open_db(ctx, password_file)?;
    let verdict =
        app_lib::commands::presentation::verify_presentation_impl(&conn, &envelope, audience)
            .map_err(|e| anyhow::anyhow!(e))
            .context("verify_presentation_impl failed")?;

    use app_lib::commands::presentation::PresentationVerification as V;
    output::blank();
    output::kv("Subject", &envelope.subject);
    match verdict {
        V::Accepted => output::success("Accepted — signature, audience, and nonce all check out"),
        V::BadSignature => output::error("Rejected — signature does not verify"),
        V::AudienceMismatch => {
            output::error("Rejected — presentation was not bound to this audience")
        }
        V::Replayed => output::error("Rejected — this presentation has already been used"),
        V::Malformed => output::error("Rejected — envelope payload is malformed"),
    }

    // An unverifiable presentation is a verdict, not a command failure: the
    // caller inspects `accepted` (or the human line above) and decides.
    output::emit(&json!({
        "subject": envelope.subject,
        "audience": audience,
        "verdict": verdict,
        "accepted": verdict == V::Accepted,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(clap::Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: PresentationCommand,
    }

    #[test]
    fn parses_verify_with_audience() {
        let cli = TestCli::parse_from(["test", "verify", "/tmp/vp.json", "--audience", "acme"]);
        match cli.cmd {
            PresentationCommand::Verify { envelope, audience } => {
                assert_eq!(envelope.to_string_lossy(), "/tmp/vp.json");
                assert_eq!(audience, "acme");
            }
        }
    }

    #[test]
    fn audience_is_required() {
        // Without an expected audience there is nothing to bind against, so
        // verification would be meaningless — clap must reject it.
        assert!(TestCli::try_parse_from(["test", "verify", "/tmp/vp.json"]).is_err());
    }
}
