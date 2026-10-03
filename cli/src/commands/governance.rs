use std::fs;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::io::Read;
#[cfg(unix)]
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use app_lib::crypto::did::{derive_did_key, parse_did_key, resolve_did_key};
use app_lib::domain::governance_certificate::*;
use app_lib::network_profile::NetworkProfile;
use clap::Subcommand;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;

use crate::output;

#[derive(Subcommand)]
pub enum GovernanceCommand {
    /// Generate seven founder key sets controlled by ONE demo operator.
    /// Never use this command to establish independently governed production trust.
    CreateDemoGenesis {
        #[arg(long)]
        network_profile: PathBuf,
        /// Credential issuer DID accepted by this demo's governance policy.
        #[arg(long)]
        issuer: String,
        /// A NEW private directory outside the repository; contains raw secret keys.
        #[arg(long)]
        private_dir: PathBuf,
        /// A NEW public file containing the fully signed canonical genesis.
        #[arg(long)]
        out: PathBuf,
    },
    /// Verify all founder signatures and canonical bytes without accessing secrets.
    VerifyGenesis {
        file: PathBuf,
        #[arg(long)]
        expected_dao_id: Option<String>,
    },
}

pub fn execute(command: &GovernanceCommand) -> Result<()> {
    match command {
        GovernanceCommand::CreateDemoGenesis {
            network_profile,
            issuer,
            private_dir,
            out,
        } => {
            let profile = NetworkProfile::parse(&fs::read(network_profile)?)?;
            resolve_did_key(&parse_did_key(issuer)?)
                .context("issuer must be a valid Ed25519 did:key")?;
            let envelope = demo_genesis(&profile.network_id, issuer)?;
            // The private directory is created exclusively with owner-only
            // permissions. Existing ceremonies and public files are never overwritten.
            persist_demo(&envelope, private_dir, out)?;
            report(&envelope.0.canonical_bytes()?, None)?;
            output::success(&format!("Public genesis: {}", out.display()));
            output::success(&format!(
                "Private founder keys (owner-only, unencrypted): {}",
                private_dir.display()
            ));
            Ok(())
        }
        GovernanceCommand::VerifyGenesis {
            file,
            expected_dao_id,
        } => {
            let mut bytes = Vec::new();
            fs::File::open(file)?
                .take(MAX_GOVERNANCE_GENESIS_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            report(&bytes, expected_dao_id.as_deref())
        }
    }
}

struct FounderKeys {
    identity: SigningKey,
    consensus: SigningKey,
    governance: SigningKey,
}

impl FounderKeys {
    fn member(&self) -> GenesisMember {
        GenesisMember {
            member_id: derive_did_key(&self.identity).0,
            identity_public_key_hex: hex::encode(self.identity.verifying_key().to_bytes()),
            consensus_public_key_hex: hex::encode(self.consensus.verifying_key().to_bytes()),
            governance_public_key_hex: hex::encode(self.governance.verifying_key().to_bytes()),
        }
    }
}

fn demo_genesis(
    network_id: &str,
    issuer: &str,
) -> Result<(FoundingGenesisEnvelope, Vec<FounderKeys>)> {
    let mut keys: Vec<_> = (0..GOVERNANCE_COMMITTEE_SIZE)
        .map(|_| FounderKeys {
            identity: SigningKey::generate(&mut OsRng),
            consensus: SigningKey::generate(&mut OsRng),
            governance: SigningKey::generate(&mut OsRng),
        })
        .collect();
    keys.sort_by_key(|keys| keys.member().member_id);
    let core = FoundingGenesisCore {
        genesis_version: GOVERNANCE_GENESIS_VERSION,
        protocol_version: GOVERNANCE_CERTIFICATE_VERSION,
        name: format!("Alexandria {network_id} — temporary single-operator demo governance"),
        scope: GenesisScope {
            scope_type: "network".into(),
            scope_id: network_id.into(),
        },
        members: keys.iter().map(FounderKeys::member).collect(),
        rules: GenesisRules {
            rules_version: "demo-1".into(),
            committee_size: GOVERNANCE_COMMITTEE_SIZE as u8,
            receipt_threshold: GOVERNANCE_QUORUM as u8,
            outcome_threshold: GOVERNANCE_QUORUM as u8,
            proposal_approval_numerator: 2,
            proposal_approval_denominator: 3,
            minimum_turnout_count: 1,
        },
        qualification_policy: GenesisQualificationPolicy {
            policy_version: "demo-1".into(),
            accepted_issuers: vec![issuer.into()],
            accepted_assessment_evidence: vec!["assessment-credential".into()],
        },
        activation: GenesisActivation {
            cometbft_chain_id: format!("alexandria-{network_id}-demo-1"),
            initial_epoch: 0,
            initial_height: 1,
            activation_time_unix: chrono::Utc::now().timestamp(),
        },
    };
    let bytes = core.acceptance_signing_bytes()?;
    let acceptances = keys
        .iter()
        .map(|keys| FoundingAcceptance {
            member_id: keys.member().member_id,
            identity_signature_hex: hex::encode(keys.identity.sign(&bytes).to_bytes()),
            consensus_signature_hex: hex::encode(keys.consensus.sign(&bytes).to_bytes()),
            governance_signature_hex: hex::encode(keys.governance.sign(&bytes).to_bytes()),
        })
        .collect();
    let envelope = FoundingGenesisEnvelope { core, acceptances };
    envelope.verify(None)?;
    Ok((envelope, keys))
}

#[cfg(unix)]
fn persist_demo(
    (envelope, keys): &(FoundingGenesisEnvelope, Vec<FounderKeys>),
    private_dir: &Path,
    out: &Path,
) -> Result<()> {
    if out.exists() {
        bail!("public output already exists");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        fs::DirBuilder::new()
            .mode(0o700)
            .create(private_dir)
            .context("create a new private directory (its parent must exist)")?;
        for (index, keys) in keys.iter().enumerate() {
            for (role, key) in [
                ("identity", &keys.identity),
                ("consensus", &keys.consensus),
                ("governance", &keys.governance),
            ] {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(private_dir.join(format!("founder-{}-{role}.secret", index + 1)))?;
                file.write_all(key.as_bytes())?;
                file.sync_all()?;
            }
        }
    }

    // The public manifest maps founder-N files to their canonical public DID.
    let bytes = envelope.canonical_bytes()?;
    fs::write(private_dir.join("public-genesis.json"), &bytes)?;
    let mut public = OpenOptions::new().write(true).create_new(true).open(out)?;
    public.write_all(&bytes)?;
    public.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn persist_demo(
    _ceremony: &(FoundingGenesisEnvelope, Vec<FounderKeys>),
    _private_dir: &Path,
    _out: &Path,
) -> Result<()> {
    bail!("demo key generation requires Unix owner-only file permissions")
}

fn report(bytes: &[u8], expected: Option<&str>) -> Result<()> {
    let (envelope, verified) = decode_and_verify_genesis(bytes, expected)?;
    let summary = serde_json::json!({
        "dao_id": verified.dao_id(),
        "envelope_blake3": blake3::hash(bytes).to_hex().to_string(),
        "name": envelope.core.name,
        "network_scope": envelope.core.scope,
        "founders": envelope.core.members.len(),
        "verified_signatures": envelope.acceptances.len() * 3,
    });
    output::success(&format!("Verified genesis {}", verified.dao_id()));
    output::emit(&summary)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_ceremony_uses_fresh_keys_and_all_signatures_bind_demo_terms() {
        let issuer = derive_did_key(&SigningKey::generate(&mut OsRng)).0;
        let (first, _) = demo_genesis("preprod", &issuer).unwrap();
        let (second, _) = demo_genesis("preprod", &issuer).unwrap();
        assert_ne!(first.core.members, second.core.members);
        assert!(first.core.name.contains("temporary single-operator demo"));
        let bytes = first.canonical_bytes().unwrap();
        decode_and_verify_genesis(&bytes, None).unwrap();
        let mut modified = first;
        modified.core.rules.minimum_turnout_count = 2;
        assert!(modified.verify(None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn secrets_are_owner_only_and_existing_ceremonies_are_never_overwritten() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("keys");
        let public = root.path().join("genesis.json");
        let issuer = derive_did_key(&SigningKey::generate(&mut OsRng)).0;
        let ceremony = demo_genesis("preprod", &issuer).unwrap();
        persist_demo(&ceremony, &private, &public).unwrap();
        assert_eq!(
            fs::metadata(&private).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(private.join("founder-1-identity.secret"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let original = fs::read(&public).unwrap();
        assert!(persist_demo(&ceremony, &private, &public).is_err());
        assert_eq!(fs::read(&public).unwrap(), original);
        assert!(persist_demo(&ceremony, &private, &root.path().join("other.json")).is_err());
    }
}
