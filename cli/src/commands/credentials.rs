//! `alex credentials` — inspect and manage Verifiable Credentials.
//!
//! Mirrors the `alex db` pattern: opens the SQLCipher-encrypted DB
//! against the same vault password, then delegates to the impl
//! functions in `app_lib::commands::credentials::*` so there's one
//! source of truth between the GUI's IPC handlers and the CLI.
//!
//! Subcommands:
//!   list                  — print every credential as kv lines
//!   get <id>              — print one VC as pretty JSON
//!   export --out FILE     — write the §20.4 survivability bundle
//!   verify <bundle.json>  — offline-verify a bundle (no DB needed)

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use rusqlite::{Connection, OpenFlags};

use crate::context::ProjectContext;
use crate::output;

#[derive(Subcommand)]
pub enum CredentialsCommand {
    /// List every credential in the local store
    List,

    /// Print one credential as pretty JSON
    Get {
        /// Credential URN (e.g. `urn:uuid:abc-…`)
        id: String,
    },

    /// Write a §20.4 survivability bundle to disk
    Export {
        /// Output path for the JCS-canonical bundle JSON
        #[arg(long)]
        out: PathBuf,
    },

    /// Offline-verify a bundle with no Alexandria infrastructure
    Verify {
        /// Path to a bundle JSON file
        bundle: PathBuf,

        /// Verification time (ISO 8601 UTC). Defaults to now.
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
        CredentialsCommand::List => run_list(ctx, password_file),
        CredentialsCommand::Get { id } => run_get(ctx, password_file, id),
        CredentialsCommand::Export { out } => run_export(ctx, password_file, out),
        CredentialsCommand::Verify { bundle, at } => run_verify(bundle, at.as_deref()),
    }
}

// ---- DB open ------------------------------------------------------------

fn get_vault_password(password_file: Option<&Path>) -> Result<String> {
    if let Some(path) = password_file {
        fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .with_context(|| format!("Failed to read password file: {}", path.display()))
    } else {
        dialoguer::Password::new()
            .with_prompt("Vault password")
            .interact()
            .context("Failed to read password")
    }
}

fn open_db(ctx: &ProjectContext, password_file: Option<&Path>) -> Result<Connection> {
    if !ctx.has_vault() {
        bail!(
            "No vault found at {}.\n\
             Launch the app and create a wallet first.",
            ctx.vault_dir().display()
        );
    }
    let password = get_vault_password(password_file)?;
    let db_key = ctx.derive_db_key(&password)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
    let conn = Connection::open_with_flags(ctx.db_path(), flags)
        .with_context(|| format!("Failed to open database at {}", ctx.db_path().display()))?;
    conn.pragma_update(None, "key", format!("x'{}'", hex::encode(db_key)))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
        .context("Failed to decrypt database — wrong password?")?;
    Ok(conn)
}

// ---- Subcommands --------------------------------------------------------

fn run_list(ctx: &ProjectContext, password_file: Option<&Path>) -> Result<()> {
    output::header("Credentials");
    let conn = open_db(ctx, password_file)?;

    let creds = app_lib::commands::credentials::list_credentials_impl(&conn, None, None)
        .map_err(|e| anyhow::anyhow!(e))
        .context("list_credentials_impl failed")?;

    if creds.is_empty() {
        output::info("No credentials in the local store.");
        return Ok(());
    }

    for vc in &creds {
        let class = vc
            .type_
            .iter()
            .find(|t| t.as_str() != "VerifiableCredential")
            .map(|s| s.as_str())
            .unwrap_or("Credential");
        output::blank();
        output::kv("ID", &vc.id);
        output::kv("Type", class);
        output::kv("Issuer", vc.issuer.as_str());
        output::kv("Subject", vc.credential_subject.id.as_str());
        output::kv("Issued", &vc.issuance_date);
        if let Some(exp) = &vc.expiration_date {
            output::kv("Expires", exp);
        }
    }
    output::blank();
    output::success(&format!("{} credential(s)", creds.len()));
    Ok(())
}

fn run_get(ctx: &ProjectContext, password_file: Option<&Path>, id: &str) -> Result<()> {
    let conn = open_db(ctx, password_file)?;
    let vc = app_lib::commands::credentials::get_credential_impl(&conn, id)
        .map_err(|e| anyhow::anyhow!(e))?
        .ok_or_else(|| anyhow::anyhow!("credential `{id}` not found"))?;
    let pretty = serde_json::to_string_pretty(&vc).context("serialize VC")?;
    println!("{pretty}");
    Ok(())
}

fn run_export(ctx: &ProjectContext, password_file: Option<&Path>, out: &Path) -> Result<()> {
    output::header("Export bundle");
    output::kv("Output", &out.display().to_string());
    let conn = open_db(ctx, password_file)?;

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
    Ok(())
}

fn run_verify(bundle_path: &Path, at: Option<&str>) -> Result<()> {
    output::header("Offline bundle verify");
    output::kv("Bundle", &bundle_path.display().to_string());

    let json = fs::read_to_string(bundle_path)
        .with_context(|| format!("read bundle at {}", bundle_path.display()))?;
    let now_owned;
    let now = if let Some(t) = at {
        t
    } else {
        now_owned = chrono_now();
        &now_owned
    };
    output::kv("At", now);

    let (accepted, total) = app_lib::commands::credentials::verify_bundle_offline_impl(&json, now)
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
    Ok(())
}

/// Lightweight RFC 3339 "now" without pulling chrono into cli/Cargo.toml.
/// `app_lib` already brings chrono in as a transitive dep, so we just
/// reach through it instead of duplicating the dependency declaration.
fn chrono_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

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
        assert!(matches!(cli.cmd, CredentialsCommand::List));
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
        assert!(text.contains("list"));
        assert!(text.contains("export"));
        assert!(text.contains("verify"));
    }
}
