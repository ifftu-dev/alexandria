//! Offline Ed25519 keypair generator for bootstrap-snapshot founder keys.
//!
//! Generates one keypair, writes the 32-byte raw secret to `--out` (mode
//! 0600), and prints the matching public key hex to stdout. Run on an
//! air-gapped machine for each founder slot.
//!
//! Usage:
//!   cargo run --manifest-path src-tauri/Cargo.toml \
//!       --example snapshot_keygen -- --out ./founder_a.sk
//!
//! Then paste the printed hex into
//! `src-tauri/src/p2p/registry.rs::SNAPSHOT_VERIFIERS` and commit. The
//! `.sk` files themselves MUST NOT enter version control.

use anyhow::{anyhow, Context, Result};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use std::env;
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

fn parse_out_path() -> Result<String> {
    let argv: Vec<String> = env::args().collect();
    let mut i = 1usize;
    while i < argv.len() {
        if argv[i] == "--out" && i + 1 < argv.len() {
            return Ok(argv[i + 1].clone());
        }
        i += 1;
    }
    Err(anyhow!("usage: --out <path-to-write-secret-key>"))
}

fn main() -> Result<()> {
    let out_path = parse_out_path()?;
    let mut rng = OsRng;
    let sk = SigningKey::generate(&mut rng);
    let vk = sk.verifying_key();

    // Write the 32-byte raw secret with strict permissions.
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    opts.mode(0o600);
    let mut f = opts
        .open(&out_path)
        .with_context(|| format!("create {out_path} (file must not pre-exist)"))?;
    f.write_all(&sk.to_bytes())
        .context("write secret key bytes")?;
    f.sync_all().ok();

    println!("WROTE  {} (32 bytes, mode 0600)", out_path);
    println!("PUBKEY {}", hex::encode(vk.to_bytes()));
    println!();
    println!("Paste PUBKEY into SNAPSHOT_VERIFIERS in src-tauri/src/p2p/registry.rs.");
    println!("Keep the .sk file offline. Required to sign bootstrap_registry.json.");
    Ok(())
}
