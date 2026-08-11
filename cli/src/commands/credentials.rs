//! `alexandria credentials` (alias `alexandria vc`) — the full Verifiable Credential
//! lifecycle.
//!
//! Opens the SQLCipher-encrypted database against the same vault password
//! the app uses, then delegates to the impl functions in
//! `app_lib::commands::credentials::*` so there is one source of truth
//! between the GUI's IPC handlers and the CLI. Commands that mint a
//! credential additionally unlock Stronghold to obtain the issuer signing
//! key — see [`crate::vault`].
//!
//! Subcommands:
//!   list                    — every credential in the store
//!   get <id>                — one VC as JSON
//!   issue --request FILE    — mint and sign a new VC
//!   revoke <id> --reason    — status-list revocation (permanent)
//!   suspend <id>            — reversible suspension
//!   reinstate <id>          — lift a suspension
//!   import <file>           — verify and store credentials from a file
//!   export --out FILE       — §20.4 survivability bundle
//!   verify <bundle.json>    — offline-verify a bundle, no DB needed

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use serde_json::json;

use crate::context::ProjectContext;
use crate::output;
use crate::vault;

#[derive(Subcommand)]
pub enum CredentialsCommand {
    /// List every credential in the local store
    List {
        /// Only credentials issued to this subject DID
        #[arg(long)]
        subject: Option<String>,

        /// Only credentials attesting this skill id
        #[arg(long)]
        skill: Option<String>,
    },

    /// Print one credential as pretty JSON
    Get {
        /// Credential URN (e.g. `urn:uuid:abc-…`)
        id: String,
    },

    /// Issue and sign a new credential
    ///
    /// The request is read as JSON so the full `IssueCredentialRequest`
    /// shape (credential type, claim, evidence refs, supersession,
    /// integrity binding and policy) is expressible without flattening it
    /// into a wall of flags that would drift from the struct.
    Issue {
        /// Path to a JSON `IssueCredentialRequest`, or `-` for stdin
        #[arg(long)]
        request: PathBuf,
    },

    /// Revoke a credential permanently via its status list
    Revoke {
        /// Credential URN
        id: String,

        /// Why it was revoked — recorded in the status list entry
        #[arg(long)]
        reason: String,
    },

    /// Suspend a credential (reversible, unlike revocation)
    Suspend {
        /// Credential URN
        id: String,

        /// Auto-expire the suspension at this time (ISO 8601 UTC)
        #[arg(long)]
        until: Option<String>,

        /// Why it was suspended
        #[arg(long)]
        reason: Option<String>,
    },

    /// Lift a suspension
    Reinstate {
        /// Credential URN
        id: String,
    },

    /// Verify and store credentials from a file
    Import {
        /// Path to a credential, bundle, or presentation JSON file
        file: PathBuf,
    },

    /// Write a §20.4 survivability bundle to disk
    Export {
        /// Output path for the JCS-canonical bundle JSON
        #[arg(long)]
        out: PathBuf,
    },

    /// Offline-verify a bundle with no Alexandria infrastructure
    Verify {
        /// Path to a bundle JSON file, or `-` to read it from stdin
        bundle: PathBuf,

        /// Verify as of this moment (ISO 8601 UTC), rather than now.
        ///
        /// Changes three things: whether a credential had expired, whether a
        /// suspension window was still open, and which issuer key was current
        /// — so a credential signed before a key rotation still verifies when
        /// checked at a time when that key was valid.
        #[arg(long)]
        at: Option<String>,
    },
}

pub fn execute(
    cmd: &CredentialsCommand,
    ctx: &ProjectContext,
    password_file: Option<&Path>,
) -> Result<()> {
    match cmd {
        CredentialsCommand::List { subject, skill } => {
            run_list(ctx, password_file, subject.as_deref(), skill.as_deref())
        }
        CredentialsCommand::Get { id } => run_get(ctx, password_file, id),
        CredentialsCommand::Issue { request } => run_issue(ctx, password_file, request),
        CredentialsCommand::Revoke { id, reason } => run_revoke(ctx, password_file, id, reason),
        CredentialsCommand::Suspend { id, until, reason } => {
            run_suspend(ctx, password_file, id, until.as_deref(), reason.as_deref())
        }
        CredentialsCommand::Reinstate { id } => run_reinstate(ctx, password_file, id),
        CredentialsCommand::Import { file } => run_import(ctx, password_file, file),
        CredentialsCommand::Export { out } => run_export(ctx, password_file, out),
        CredentialsCommand::Verify { bundle, at } => run_verify(bundle, at.as_deref()),
    }
}

// ---- Helpers ------------------------------------------------------------

/// Read a JSON argument from a file, or from stdin when the path is `-`.
fn read_json_arg(path: &Path) -> Result<String> {
    if path.as_os_str() == "-" {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("read JSON from stdin")?;
        Ok(buf)
    } else {
        fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
    }
}

/// Render a credential as aligned key-value lines.
///
/// `VerifiableCredential` lives in `alexandria-verify` and reaches us through
/// `app_lib::domain::vc`; `commands::credentials` only imports it privately.
fn print_vc_summary(vc: &app_lib::domain::vc::VerifiableCredential) {
    let class = vc
        .type_
        .iter()
        .find(|t| t.as_str() != "VerifiableCredential")
        .map(|s| s.as_str())
        .unwrap_or("Credential");
    output::kv("ID", vc.id.as_deref().unwrap_or("(no envelope id)"));
    output::kv("Type", class);
    output::kv("Issuer", vc.issuer.as_str());
    output::kv("Subject", vc.credential_subject.id.as_str());
    output::kv("Issued", &vc.valid_from);
    if let Some(exp) = &vc.valid_until {
        output::kv("Expires", exp);
    }
}

// ---- Subcommands --------------------------------------------------------

fn run_list(
    ctx: &ProjectContext,
    password_file: Option<&Path>,
    subject: Option<&str>,
    skill: Option<&str>,
) -> Result<()> {
    output::header("Credentials");
    let conn = vault::open_db(ctx, password_file)?;

    let creds = app_lib::commands::credentials::list_credentials_impl(&conn, subject, skill)
        .map_err(|e| anyhow::anyhow!(e))
        .context("list_credentials_impl failed")?;

    if creds.is_empty() {
        output::info("No credentials in the local store.");
    }
    for vc in &creds {
        output::blank();
        print_vc_summary(vc);
    }
    if !creds.is_empty() {
        output::blank();
        output::success(&format!("{} credential(s)", creds.len()));
    }

    output::emit(&creds)
}

fn run_get(ctx: &ProjectContext, password_file: Option<&Path>, id: &str) -> Result<()> {
    let conn = vault::open_db(ctx, password_file)?;
    let vc = app_lib::commands::credentials::get_credential_impl(&conn, id)
        .map_err(|e| anyhow::anyhow!(e))?
        .ok_or_else(|| anyhow::anyhow!("credential `{id}` not found"))?;

    if output::is_json() {
        return output::emit(&vc);
    }
    // Human mode still prints the full JSON to stdout — this subcommand has
    // always been the "give me the document" escape hatch.
    let pretty = serde_json::to_string_pretty(&vc).context("serialize VC")?;
    println!("{pretty}");
    Ok(())
}

fn run_issue(ctx: &ProjectContext, password_file: Option<&Path>, request: &Path) -> Result<()> {
    output::header("Issue credential");

    let raw = read_json_arg(request)?;
    let req: app_lib::commands::credentials::IssueCredentialRequest =
        serde_json::from_str(&raw).context("parse IssueCredentialRequest JSON")?;

    // Unlocks Stronghold, not just the database — issuance signs.
    let signer = vault::unlock(ctx, password_file)?;
    let now = vault::now_rfc3339();

    let vc = app_lib::commands::credentials::issue_credential_impl(
        &signer.conn,
        &signer.signing_key,
        &signer.issuer_did,
        &req,
        &now,
    )
    .map_err(|e| anyhow::anyhow!(e))
    .context("issue_credential_impl failed")?;

    output::blank();
    print_vc_summary(&vc);
    output::blank();
    output::success("Credential issued and signed");

    output::emit(&vc)
}

fn run_revoke(
    ctx: &ProjectContext,
    password_file: Option<&Path>,
    id: &str,
    reason: &str,
) -> Result<()> {
    output::header("Revoke credential");
    output::kv("ID", id);
    output::kv("Reason", reason);

    let conn = vault::open_db(ctx, password_file)?;
    let now = vault::now_rfc3339();

    app_lib::commands::credentials::revoke_credential_impl(&conn, id, reason, &now)
        .map_err(|e| anyhow::anyhow!(e))
        .context("revoke_credential_impl failed")?;

    output::blank();
    output::success("Revoked — this is permanent and cannot be reinstated");

    output::emit(&json!({ "id": id, "status": "revoked", "reason": reason, "at": now }))
}

fn run_suspend(
    ctx: &ProjectContext,
    password_file: Option<&Path>,
    id: &str,
    until: Option<&str>,
    reason: Option<&str>,
) -> Result<()> {
    output::header("Suspend credential");
    output::kv("ID", id);
    if let Some(u) = until {
        output::kv("Until", u);
    }
    if let Some(r) = reason {
        output::kv("Reason", r);
    }

    let conn = vault::open_db(ctx, password_file)?;
    let now = vault::now_rfc3339();

    app_lib::commands::credentials::suspend_credential_impl(&conn, id, until, reason, &now)
        .map_err(|e| anyhow::anyhow!(e))
        .context("suspend_credential_impl failed")?;

    output::blank();
    output::success("Suspended — reversible with `alexandria credentials reinstate`");

    output::emit(&json!({
        "id": id,
        "status": "suspended",
        "until": until,
        "reason": reason,
        "at": now,
    }))
}

fn run_reinstate(ctx: &ProjectContext, password_file: Option<&Path>, id: &str) -> Result<()> {
    output::header("Reinstate credential");
    output::kv("ID", id);

    let conn = vault::open_db(ctx, password_file)?;
    app_lib::commands::credentials::reinstate_credential_impl(&conn, id)
        .map_err(|e| anyhow::anyhow!(e))
        .context("reinstate_credential_impl failed")?;

    output::blank();
    output::success("Suspension lifted");

    output::emit(&json!({ "id": id, "status": "active" }))
}

fn run_import(ctx: &ProjectContext, password_file: Option<&Path>, file: &Path) -> Result<()> {
    output::header("Import credentials");
    output::kv("File", &file.display().to_string());

    let payload = read_json_arg(file)?;
    let conn = vault::open_db(ctx, password_file)?;
    let now = vault::now_rfc3339();

    let summary = app_lib::commands::import::import_credentials_impl(&conn, &payload, &now)
        .map_err(|e| anyhow::anyhow!(e))
        .context("import_credentials_impl failed")?;

    output::blank();
    output::kv("Imported", &summary.imported.to_string());
    output::kv("Already present", &summary.already_present.to_string());
    output::kv("Failed", &summary.failed.len().to_string());

    // Never summarize failures away: a user handing over ten credentials and
    // getting nine needs to know which one failed and why.
    if !summary.failed.is_empty() {
        output::blank();
        for failure in &summary.failed {
            output::warning(&format!("{:?}", failure));
        }
    }

    output::blank();
    if summary.failed.is_empty() {
        output::success(&format!("{} credential(s) imported", summary.imported));
    } else {
        output::warning(&format!(
            "{} credential(s) rejected — see above",
            summary.failed.len()
        ));
    }

    output::emit(&summary)
}

fn run_export(ctx: &ProjectContext, password_file: Option<&Path>, out: &Path) -> Result<()> {
    output::header("Export bundle");
    output::kv("Output", &out.display().to_string());
    let conn = vault::open_db(ctx, password_file)?;

    let bundle_json = app_lib::commands::credentials::export_bundle_impl(&conn)
        .map_err(|e| anyhow::anyhow!(e))
        .context("export_bundle_impl failed")?;

    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }
    }
    fs::write(out, &bundle_json).with_context(|| format!("write bundle to {}", out.display()))?;

    let bytes = bundle_json.len();
    output::success(&format!("Wrote {bytes} bytes"));

    output::emit(&json!({ "path": out.display().to_string(), "bytes": bytes }))
}

fn run_verify(bundle_path: &Path, at: Option<&str>) -> Result<()> {
    output::header("Offline bundle verify");
    output::kv("Bundle", &bundle_path.display().to_string());

    let json_text = read_json_arg(bundle_path)?;
    let now_owned;
    let now = if let Some(t) = at {
        t
    } else {
        now_owned = vault::now_rfc3339();
        &now_owned
    };
    output::kv("At", now);

    let (accepted, total) =
        app_lib::commands::credentials::verify_bundle_offline_impl(&json_text, now)
            .map_err(|e| anyhow::anyhow!(e))
            .context("verify_bundle_offline_impl failed")?;

    output::blank();
    output::kv("Total", &total.to_string());
    output::kv("Accepted", &accepted.to_string());
    if accepted == total {
        output::success("Every credential in the bundle verifies offline.");
    } else {
        output::warning(&format!(
            "{} of {} credential(s) failed verification",
            total - accepted,
            total
        ));
    }

    output::emit(&json!({
        "total": total,
        "accepted": accepted,
        "at": now,
        "allAccepted": accepted == total,
    }))
}

/// Verification failures are a result, not a crash — the command still exits
/// 0 and reports the counts, so a caller can act on `allAccepted`.
#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    #[derive(clap::Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: CredentialsCommand,
    }

    #[test]
    fn parses_list_subcommand() {
        let cli = TestCli::parse_from(["test", "list"]);
        assert!(matches!(
            cli.cmd,
            CredentialsCommand::List {
                subject: None,
                skill: None
            }
        ));
    }

    #[test]
    fn parses_list_filters() {
        let cli =
            TestCli::parse_from(["test", "list", "--subject", "did:key:z6Mk", "--skill", "s1"]);
        match cli.cmd {
            CredentialsCommand::List { subject, skill } => {
                assert_eq!(subject.as_deref(), Some("did:key:z6Mk"));
                assert_eq!(skill.as_deref(), Some("s1"));
            }
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn parses_get_with_id() {
        let cli = TestCli::parse_from(["test", "get", "urn:uuid:abc"]);
        match cli.cmd {
            CredentialsCommand::Get { id } => assert_eq!(id, "urn:uuid:abc"),
            _ => panic!("expected Get"),
        }
    }

    #[test]
    fn parses_issue_with_request() {
        let cli = TestCli::parse_from(["test", "issue", "--request", "/tmp/req.json"]);
        match cli.cmd {
            CredentialsCommand::Issue { request } => {
                assert_eq!(request.to_string_lossy(), "/tmp/req.json");
            }
            _ => panic!("expected Issue"),
        }
    }

    #[test]
    fn revoke_requires_a_reason() {
        // Revocation is permanent; the reason is recorded in the status list
        // entry, so clap must refuse a revoke with no explanation.
        assert!(TestCli::try_parse_from(["test", "revoke", "urn:uuid:abc"]).is_err());

        let cli = TestCli::parse_from(["test", "revoke", "urn:uuid:abc", "--reason", "fraud"]);
        match cli.cmd {
            CredentialsCommand::Revoke { id, reason } => {
                assert_eq!(id, "urn:uuid:abc");
                assert_eq!(reason, "fraud");
            }
            _ => panic!("expected Revoke"),
        }
    }

    #[test]
    fn suspend_takes_optional_until_and_reason() {
        let cli = TestCli::parse_from(["test", "suspend", "urn:uuid:abc"]);
        match cli.cmd {
            CredentialsCommand::Suspend { id, until, reason } => {
                assert_eq!(id, "urn:uuid:abc");
                assert!(until.is_none());
                assert!(reason.is_none());
            }
            _ => panic!("expected Suspend"),
        }
    }

    #[test]
    fn parses_import_and_reinstate() {
        match TestCli::parse_from(["test", "import", "/tmp/creds.json"]).cmd {
            CredentialsCommand::Import { file } => {
                assert_eq!(file.to_string_lossy(), "/tmp/creds.json");
            }
            _ => panic!("expected Import"),
        }
        match TestCli::parse_from(["test", "reinstate", "urn:uuid:abc"]).cmd {
            CredentialsCommand::Reinstate { id } => assert_eq!(id, "urn:uuid:abc"),
            _ => panic!("expected Reinstate"),
        }
    }

    #[test]
    fn parses_export_with_out() {
        let cli = TestCli::parse_from(["test", "export", "--out", "/tmp/bundle.json"]);
        match cli.cmd {
            CredentialsCommand::Export { out } => {
                assert_eq!(out.to_string_lossy(), "/tmp/bundle.json");
            }
            _ => panic!("expected Export"),
        }
    }

    #[test]
    fn parses_verify_with_optional_at() {
        let cli = TestCli::parse_from([
            "test",
            "verify",
            "/tmp/bundle.json",
            "--at",
            "2026-04-13T00:00:00Z",
        ]);
        match cli.cmd {
            CredentialsCommand::Verify { bundle, at } => {
                assert_eq!(bundle.to_string_lossy(), "/tmp/bundle.json");
                assert_eq!(at.as_deref(), Some("2026-04-13T00:00:00Z"));
            }
            _ => panic!("expected Verify"),
        }
    }

    #[test]
    fn help_renders_without_panic() {
        // Smoke-test that clap's derive emits sensible help output.
        let mut cmd = TestCli::command();
        let help = cmd.render_long_help();
        let text = help.to_string();
        for expected in [
            "list", "issue", "revoke", "suspend", "import", "export", "verify",
        ] {
            assert!(text.contains(expected), "help missing `{expected}`");
        }
    }
}
