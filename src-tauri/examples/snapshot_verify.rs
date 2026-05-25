//! Offline verifier for `bootstrap_registry.json`. Smoke-checks that a
//! snapshot parses + meets the 2-of-3 multisig quorum against the
//! `SNAPSHOT_VERIFIERS` baked into the current build.
//!
//! Usage:
//!   cargo run --manifest-path src-tauri/Cargo.toml --example snapshot_verify -- \
//!       --in src-tauri/resources/bootstrap_registry.json

use anyhow::{anyhow, Context, Result};
use app_lib::p2p::registry::BootstrapSnapshot;
use std::env;
use std::fs;

fn parse_in() -> Result<String> {
    let argv: Vec<String> = env::args().collect();
    let mut i = 1usize;
    while i < argv.len() {
        if argv[i] == "--in" && i + 1 < argv.len() {
            return Ok(argv[i + 1].clone());
        }
        i += 1;
    }
    Err(anyhow!("--in <bootstrap.json> required"))
}

fn main() -> Result<()> {
    let path = parse_in()?;
    let bytes = fs::read(&path).with_context(|| format!("read {path}"))?;
    let snap =
        BootstrapSnapshot::parse_and_verify(&bytes).map_err(|e| anyhow!("verify failed: {e}"))?;
    println!(
        "OK: {} entries, {} signatures verified against SNAPSHOT_VERIFIERS",
        snap.entries.len(),
        snap.signatures.len()
    );
    Ok(())
}
