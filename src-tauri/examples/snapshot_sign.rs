//! Offline multisig signer for `bootstrap_registry.json`.
//!
//! Loads a candidate snapshot (any number of existing signatures, may
//! be 0), signs the canonical bytes with the founder key in `--key`,
//! and writes the snapshot back with the new signature appended.
//!
//! Usage:
//!   cargo run --manifest-path src-tauri/Cargo.toml --example snapshot_sign -- \
//!       --in  src-tauri/resources/bootstrap_registry.json \
//!       --out src-tauri/resources/bootstrap_registry.json \
//!       --key ./founder_a.sk \
//!       --signer founder_a
//!
//! Run once per founder (offline, on each founder's machine), pointing
//! `--key` at that founder's `.sk` from `snapshot_keygen`. The
//! `--signer` label is advisory only — verification just counts how
//! many distinct keys in `SNAPSHOT_VERIFIERS` produced valid sigs.
//!
//! The signed file in `--out` is what ships in the release bundle.

use anyhow::{anyhow, Context, Result};
use app_lib::p2p::registry::{BootstrapSnapshot, SnapshotSignature};
use ed25519_dalek::{Signer, SigningKey};
use std::env;
use std::fs;

struct Args {
    in_path: String,
    out_path: String,
    key_path: String,
    signer: String,
}

fn parse_args() -> Result<Args> {
    let argv: Vec<String> = env::args().collect();
    let mut in_path = None;
    let mut out_path = None;
    let mut key_path = None;
    let mut signer = None;
    let mut i = 1usize;
    while i < argv.len() {
        match argv[i].as_str() {
            "--in" if i + 1 < argv.len() => {
                in_path = Some(argv[i + 1].clone());
                i += 2;
            }
            "--out" if i + 1 < argv.len() => {
                out_path = Some(argv[i + 1].clone());
                i += 2;
            }
            "--key" if i + 1 < argv.len() => {
                key_path = Some(argv[i + 1].clone());
                i += 2;
            }
            "--signer" if i + 1 < argv.len() => {
                signer = Some(argv[i + 1].clone());
                i += 2;
            }
            _ => i += 1,
        }
    }
    Ok(Args {
        in_path: in_path.ok_or_else(|| anyhow!("--in <bootstrap.json> required"))?,
        out_path: out_path.ok_or_else(|| anyhow!("--out <bootstrap.json> required"))?,
        key_path: key_path.ok_or_else(|| anyhow!("--key <founder.sk> required"))?,
        signer: signer.ok_or_else(|| anyhow!("--signer <name> required"))?,
    })
}

fn main() -> Result<()> {
    let args = parse_args()?;

    let bytes =
        fs::read(&args.in_path).with_context(|| format!("read snapshot {}", args.in_path))?;
    let mut snap: BootstrapSnapshot =
        serde_json::from_slice(&bytes).with_context(|| "parse snapshot JSON")?;

    let signed_bytes = snap
        .canonical_signed_bytes()
        .map_err(|e| anyhow!("canonical bytes: {e}"))?;

    let sk_raw = fs::read(&args.key_path).with_context(|| format!("read key {}", args.key_path))?;
    if sk_raw.len() != 32 {
        return Err(anyhow!(
            "key file must be 32 raw bytes, got {}",
            sk_raw.len()
        ));
    }
    let sk_arr: [u8; 32] = sk_raw
        .as_slice()
        .try_into()
        .expect("checked length 32 above");
    let sk = SigningKey::from_bytes(&sk_arr);
    let signature = sk.sign(&signed_bytes);

    // Drop any existing signature from this signer label before adding
    // the new one — re-signing should idempotently update, not duplicate.
    snap.signatures.retain(|s| s.signer != args.signer);
    snap.signatures.push(SnapshotSignature {
        signer: args.signer.clone(),
        sig_hex: hex::encode(signature.to_bytes()),
    });

    // Pretty-print so the diff is readable in code review.
    let out_json = serde_json::to_string_pretty(&snap)?;
    fs::write(&args.out_path, out_json + "\n")
        .with_context(|| format!("write {}", args.out_path))?;

    println!(
        "Signed {} entries with key derived from {}",
        snap.entries.len(),
        args.key_path
    );
    println!("Wrote snapshot to {}", args.out_path);
    println!(
        "Signature count now: {}/{}",
        snap.signatures.len(),
        app_lib::p2p::registry::SNAPSHOT_QUORUM
    );
    Ok(())
}
