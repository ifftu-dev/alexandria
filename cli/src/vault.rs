//! Vault access — the one place the CLI turns a password into an open
//! database and, when a command needs to sign, an issuer key.
//!
//! Both halves come from `app_lib` rather than being reimplemented here:
//! `Keystore::open` is a plain synchronous constructor with no Tauri state
//! involved, so the CLI can walk the same path the GUI walks
//! (`commands::credentials::load_issuer_key`) — Stronghold → mnemonic →
//! wallet → Ed25519 signing key → `did:key`.
//!
//! Issuing a credential from the CLI therefore produces a signature
//! indistinguishable from one the app would produce, which is the entire
//! point of sharing the impl functions.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags};

// `crypto::did` re-exports the pure DID primitives from `alexandria-verify`,
// so the CLI reaches them through `app_lib` rather than taking a second
// direct dependency on that crate.
use app_lib::crypto::did::{derive_did_key, Did};
use app_lib::crypto::keystore::Keystore;
use app_lib::crypto::wallet;
use ed25519_dalek::SigningKey;

use crate::context::ProjectContext;

/// Read the vault password from a file, or prompt for it.
///
/// The file form exists for CI and scripted runs; it is trimmed so a
/// trailing newline from `echo > pw.txt` does not silently produce a wrong
/// password.
pub fn get_password(password_file: Option<&Path>) -> Result<String> {
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

fn ensure_vault(ctx: &ProjectContext) -> Result<()> {
    if !ctx.has_vault() {
        bail!(
            "No vault found at {}.\n\
             Launch the app and create a wallet first.",
            ctx.vault_dir().display()
        );
    }
    Ok(())
}

/// Open the SQLCipher database using the key derived from `password`.
pub fn open_db_with_key(ctx: &ProjectContext, db_key: [u8; 32]) -> Result<Connection> {
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

/// Open the database for read/write commands that never sign anything.
///
/// Derives the database key directly from the salt file, which is cheaper
/// than unlocking Stronghold and is all a non-signing command needs.
pub fn open_db(ctx: &ProjectContext, password_file: Option<&Path>) -> Result<Connection> {
    ensure_vault(ctx)?;
    let password = get_password(password_file)?;
    let db_key = ctx.derive_db_key(&password)?;
    open_db_with_key(ctx, db_key)
}

/// An unlocked vault: the database plus the issuer identity to sign with.
pub struct Signer {
    pub conn: Connection,
    pub signing_key: SigningKey,
    pub issuer_did: Did,
}

/// Unlock Stronghold and derive the issuer signing key alongside an open
/// database.
///
/// Mirrors `app_lib::commands::credentials::load_issuer_key`. The database
/// key comes from the same `Keystore` here rather than from
/// `ProjectContext::derive_db_key`, so a single password unlock serves both
/// and the two derivations cannot drift apart.
pub fn unlock(ctx: &ProjectContext, password_file: Option<&Path>) -> Result<Signer> {
    ensure_vault(ctx)?;
    let password = get_password(password_file)?;
    unlock_with_password(ctx, &password)
}

/// Unlock with a password already in hand.
///
/// The TUI collects the password through its own masked field rather than a
/// `dialoguer` prompt, which cannot run while the alternate screen is active.
pub fn unlock_with_password(ctx: &ProjectContext, password: &str) -> Result<Signer> {
    ensure_vault(ctx)?;

    let keystore = Keystore::open(&ctx.vault_dir(), password)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("Failed to unlock the vault — wrong password?")?;

    let mnemonic = keystore
        .retrieve_mnemonic()
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("Failed to retrieve the wallet mnemonic from the vault")?;

    let w = wallet::wallet_from_mnemonic(&mnemonic)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("Failed to derive the wallet from its mnemonic")?;

    // `Wallet` implements `Drop` (zeroize), so the key bytes are copied out
    // rather than moved — same reason the GUI path clones here.
    let signing_key = SigningKey::from_bytes(&w.signing_key.to_bytes());
    let issuer_did = derive_did_key(&signing_key);

    let conn = open_db_with_key(ctx, keystore.derive_db_key())?;

    Ok(Signer {
        conn,
        signing_key,
        issuer_did,
    })
}

/// RFC 3339 "now", matching the timestamps the app writes.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
