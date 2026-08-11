//! `alex role-assessment` — enterprise role assessments and the
//! organizations that own them.
//!
//! This is the headless half of the Sentinel role-assessment surface: an
//! employer defining a role, publishing it, and issuing a role credential
//! to a candidate who cleared it — without a Tauri window in the loop.
//!
//! Everything delegates to `app_lib::commands::role_assessment::*`, so a
//! role credential minted here is byte-identical to one the app mints.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;

use crate::context::ProjectContext;
use crate::output;
use crate::vault;

#[derive(Subcommand)]
pub enum RoleAssessmentCommand {
    /// Organization operations
    #[command(subcommand)]
    Org(OrgCommand),

    /// Create a role assessment from a JSON request
    Create {
        /// Path to a JSON `CreateRoleAssessmentRequest`, or `-` for stdin
        #[arg(long)]
        request: PathBuf,
    },

    /// Show one role assessment
    Get {
        /// Role assessment id
        id: String,
    },

    /// List role assessments
    List {
        /// Only assessments belonging to this organization
        #[arg(long)]
        org: Option<String>,
    },

    /// Move a role assessment between draft, published, and archived
    Status {
        /// Role assessment id
        id: String,

        /// New status
        #[arg(value_parser = ["draft", "published", "archived"])]
        status: String,
    },

    /// Issue a role credential to a candidate who cleared the assessment
    Issue {
        /// Role assessment id
        id: String,

        /// Candidate's subject DID
        #[arg(long)]
        subject: String,

        /// Integrity session the assessment was sat under — the credential
        /// is bound to it, so the issuance is auditable back to the run.
        #[arg(long)]
        session: String,
    },
}

#[derive(Subcommand)]
pub enum OrgCommand {
    /// Create an organization
    Create {
        /// Display name
        name: String,

        /// Owner's wallet address
        #[arg(long)]
        owner: String,

        /// Optional DID for the organization as issuer
        #[arg(long)]
        did: Option<String>,
    },

    /// Show one organization
    Get {
        /// Organization id
        id: String,
    },

    /// List organizations
    List {
        /// Only organizations owned by this address
        #[arg(long)]
        owner: Option<String>,
    },
}

pub fn execute(
    cmd: &RoleAssessmentCommand,
    ctx: &ProjectContext,
    password_file: Option<&Path>,
) -> Result<()> {
    match cmd {
        RoleAssessmentCommand::Org(sub) => execute_org(sub, ctx, password_file),
        RoleAssessmentCommand::Create { request } => run_create(ctx, password_file, request),
        RoleAssessmentCommand::Get { id } => run_get(ctx, password_file, id),
        RoleAssessmentCommand::List { org } => run_list(ctx, password_file, org.as_deref()),
        RoleAssessmentCommand::Status { id, status } => run_status(ctx, password_file, id, status),
        RoleAssessmentCommand::Issue {
            id,
            subject,
            session,
        } => run_issue(ctx, password_file, id, subject, session),
    }
}

fn execute_org(cmd: &OrgCommand, ctx: &ProjectContext, password_file: Option<&Path>) -> Result<()> {
    match cmd {
        OrgCommand::Create { name, owner, did } => {
            run_org_create(ctx, password_file, name, owner, did.as_deref())
        }
        OrgCommand::Get { id } => run_org_get(ctx, password_file, id),
        OrgCommand::List { owner } => run_org_list(ctx, password_file, owner.as_deref()),
    }
}

// ---- Rendering ----------------------------------------------------------

fn print_org(org: &app_lib::commands::role_assessment::Organization) {
    output::kv("ID", &org.id);
    output::kv("Name", &org.name);
    output::kv("Owner", &org.owner_address);
    if let Some(did) = &org.did {
        output::kv("DID", did);
    }
    output::kv("Created", &org.created_at);
}

fn print_assessment(ra: &app_lib::commands::role_assessment::RoleAssessment) {
    output::kv("ID", &ra.id);
    output::kv("Org", &ra.org_id);
    output::kv("Role", &ra.role_title);
    output::kv("Status", &ra.status);
    if let Some(course) = &ra.course_id {
        output::kv("Course", course);
    }
    if !ra.skill_ids.is_empty() {
        output::kv("Skills", &ra.skill_ids.join(", "));
    }
    if let Some(level) = &ra.required_assurance_level {
        output::kv("Assurance", level);
    }
    output::kv("Updated", &ra.updated_at);
}

fn read_json_arg(path: &Path) -> Result<String> {
    if path.as_os_str() == "-" {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("read JSON from stdin")?;
        Ok(buf)
    } else {
        std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
    }
}

// ---- Organizations ------------------------------------------------------

fn run_org_create(
    ctx: &ProjectContext,
    password_file: Option<&Path>,
    name: &str,
    owner: &str,
    did: Option<&str>,
) -> Result<()> {
    output::header("Create organization");
    let conn = vault::open_db(ctx, password_file)?;
    let now = vault::now_rfc3339();

    let org =
        app_lib::commands::role_assessment::create_organization_impl(&conn, name, owner, did, &now)
            .map_err(|e| anyhow::anyhow!(e))
            .context("create_organization_impl failed")?;

    output::blank();
    print_org(&org);
    output::blank();
    output::success("Organization created");

    output::emit(&org)
}

fn run_org_get(ctx: &ProjectContext, password_file: Option<&Path>, id: &str) -> Result<()> {
    output::header("Organization");
    let conn = vault::open_db(ctx, password_file)?;

    let org = app_lib::commands::role_assessment::get_organization_impl(&conn, id)
        .map_err(|e| anyhow::anyhow!(e))?
        .ok_or_else(|| anyhow::anyhow!("organization `{id}` not found"))?;

    output::blank();
    print_org(&org);
    output::emit(&org)
}

fn run_org_list(
    ctx: &ProjectContext,
    password_file: Option<&Path>,
    owner: Option<&str>,
) -> Result<()> {
    output::header("Organizations");
    let conn = vault::open_db(ctx, password_file)?;

    let orgs = app_lib::commands::role_assessment::list_organizations_impl(&conn, owner)
        .map_err(|e| anyhow::anyhow!(e))
        .context("list_organizations_impl failed")?;

    if orgs.is_empty() {
        output::info("No organizations.");
    }
    for org in &orgs {
        output::blank();
        print_org(org);
    }
    if !orgs.is_empty() {
        output::blank();
        output::success(&format!("{} organization(s)", orgs.len()));
    }

    output::emit(&orgs)
}

// ---- Role assessments ---------------------------------------------------

fn run_create(ctx: &ProjectContext, password_file: Option<&Path>, request: &Path) -> Result<()> {
    output::header("Create role assessment");

    let raw = read_json_arg(request)?;
    let req: app_lib::commands::role_assessment::CreateRoleAssessmentRequest =
        serde_json::from_str(&raw).context("parse CreateRoleAssessmentRequest JSON")?;

    let conn = vault::open_db(ctx, password_file)?;
    let now = vault::now_rfc3339();

    let ra = app_lib::commands::role_assessment::create_role_assessment_impl(&conn, &req, &now)
        .map_err(|e| anyhow::anyhow!(e))
        .context("create_role_assessment_impl failed")?;

    output::blank();
    print_assessment(&ra);
    output::blank();
    output::success("Role assessment created");

    output::emit(&ra)
}

fn run_get(ctx: &ProjectContext, password_file: Option<&Path>, id: &str) -> Result<()> {
    output::header("Role assessment");
    let conn = vault::open_db(ctx, password_file)?;

    let ra = app_lib::commands::role_assessment::get_role_assessment_impl(&conn, id)
        .map_err(|e| anyhow::anyhow!(e))?
        .ok_or_else(|| anyhow::anyhow!("role assessment `{id}` not found"))?;

    output::blank();
    print_assessment(&ra);
    output::emit(&ra)
}

fn run_list(ctx: &ProjectContext, password_file: Option<&Path>, org: Option<&str>) -> Result<()> {
    output::header("Role assessments");
    let conn = vault::open_db(ctx, password_file)?;

    let list = app_lib::commands::role_assessment::list_role_assessments_impl(&conn, org)
        .map_err(|e| anyhow::anyhow!(e))
        .context("list_role_assessments_impl failed")?;

    if list.is_empty() {
        output::info("No role assessments.");
    }
    for ra in &list {
        output::blank();
        print_assessment(ra);
    }
    if !list.is_empty() {
        output::blank();
        output::success(&format!("{} assessment(s)", list.len()));
    }

    output::emit(&list)
}

fn run_status(
    ctx: &ProjectContext,
    password_file: Option<&Path>,
    id: &str,
    status: &str,
) -> Result<()> {
    output::header("Set role assessment status");
    output::kv("ID", id);
    output::kv("Status", status);

    let conn = vault::open_db(ctx, password_file)?;
    let now = vault::now_rfc3339();

    let ra = app_lib::commands::role_assessment::set_role_assessment_status_impl(
        &conn, id, status, &now,
    )
    .map_err(|e| anyhow::anyhow!(e))
    .context("set_role_assessment_status_impl failed")?;

    output::blank();
    print_assessment(&ra);
    output::blank();
    output::success(&format!("Status is now `{}`", ra.status));

    output::emit(&ra)
}

fn run_issue(
    ctx: &ProjectContext,
    password_file: Option<&Path>,
    id: &str,
    subject: &str,
    session: &str,
) -> Result<()> {
    output::header("Issue role credential");
    output::kv("Assessment", id);
    output::kv("Subject", subject);
    output::kv("Session", session);

    // Issuance signs, so this unlocks Stronghold rather than only the DB.
    let signer = vault::unlock(ctx, password_file)?;
    let now = vault::now_rfc3339();
    let subject_did = app_lib::crypto::did::Did(subject.to_string());

    let vc = app_lib::commands::role_assessment::issue_role_credential_impl(
        &signer.conn,
        &signer.signing_key,
        &signer.issuer_did,
        id,
        &subject_did,
        session,
        &now,
    )
    .map_err(|e| anyhow::anyhow!(e))
    .context("issue_role_credential_impl failed")?;

    output::blank();
    output::kv("Credential", vc.id.as_deref().unwrap_or("(no envelope id)"));
    output::kv("Issuer", vc.issuer.as_str());
    output::blank();
    output::success("Role credential issued and signed");

    output::emit(&vc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(clap::Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: RoleAssessmentCommand,
    }

    #[test]
    fn parses_org_create() {
        let cli = TestCli::parse_from(["test", "org", "create", "Acme", "--owner", "addr1"]);
        match cli.cmd {
            RoleAssessmentCommand::Org(OrgCommand::Create { name, owner, did }) => {
                assert_eq!(name, "Acme");
                assert_eq!(owner, "addr1");
                assert!(did.is_none());
            }
            _ => panic!("expected Org(Create)"),
        }
    }

    #[test]
    fn status_rejects_unknown_values() {
        // The impl also validates, but catching it at parse time gives a
        // usable error listing the valid values instead of a backend string.
        assert!(TestCli::try_parse_from(["test", "status", "ra1", "bogus"]).is_err());
        assert!(TestCli::try_parse_from(["test", "status", "ra1", "published"]).is_ok());
    }

    #[test]
    fn issue_requires_subject_and_session() {
        assert!(TestCli::try_parse_from(["test", "issue", "ra1"]).is_err());
        assert!(
            TestCli::try_parse_from(["test", "issue", "ra1", "--subject", "did:key:z6"]).is_err()
        );

        let cli = TestCli::parse_from([
            "test",
            "issue",
            "ra1",
            "--subject",
            "did:key:z6",
            "--session",
            "sess1",
        ]);
        match cli.cmd {
            RoleAssessmentCommand::Issue {
                id,
                subject,
                session,
            } => {
                assert_eq!(id, "ra1");
                assert_eq!(subject, "did:key:z6");
                assert_eq!(session, "sess1");
            }
            _ => panic!("expected Issue"),
        }
    }

    #[test]
    fn list_takes_optional_org_filter() {
        match TestCli::parse_from(["test", "list"]).cmd {
            RoleAssessmentCommand::List { org } => assert!(org.is_none()),
            _ => panic!("expected List"),
        }
        match TestCli::parse_from(["test", "list", "--org", "o1"]).cmd {
            RoleAssessmentCommand::List { org } => assert_eq!(org.as_deref(), Some("o1")),
            _ => panic!("expected List"),
        }
    }
}
