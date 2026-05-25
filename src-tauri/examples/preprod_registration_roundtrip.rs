//! Live preprod smoke test for the stake-pubkey registry pipeline.
//!
//! Builds + submits one real `stake_pubkey_registration` transaction,
//! waits for confirmation, then runs the same `BlockfrostFetcher` the
//! production refresh task uses and checks that the new entry comes
//! back with a valid witness signature.
//!
//! Two key-source modes:
//!
//! 1. **BIP-39 wallet mode** — set `ALEXANDRIA_TEST_MNEMONIC` to a
//!    24-word phrase. Derives payment + stake keys via CIP-1852.
//! 2. **Raw `.skey` mode** — set `ALEXANDRIA_TEST_PAYMENT_SKEY` and
//!    `ALEXANDRIA_TEST_STAKE_SKEY` to paths of Cardano-CLI
//!    `PaymentSigningKeyShelley_ed25519` / `StakeSigningKeyShelley_ed25519`
//!    JSON files (cborHex of 32 raw bytes). Also requires
//!    `ALEXANDRIA_TEST_ADDRESS` (the bech32 payment address that holds
//!    the funding UTxOs).
//!
//! Exercises end-to-end:
//! - `cardano::stake_pubkey::build_registration_tx` (PR B-2)
//! - `BlockfrostClient::submit_tx`
//! - `BlockfrostClient::get_tx_cbor` + `tx_witnesses_include_stake_key`
//! - `p2p::registry_chain::BlockfrostFetcher::fetch` (PR B-1)
//!
//! Required env (one of the two modes above) plus:
//!   BLOCKFROST_PROJECT_ID    preprod project id
//!
//! Optional env:
//!   ALEXANDRIA_TEST_PUBKEY   32-byte hex pubkey to register. Defaults to
//!                            the wallet's own payment public key.
//!   ALEXANDRIA_TEST_VALID_SECS  Validity window length in seconds;
//!                               default 1 year.

use anyhow::{anyhow, bail, Context, Result};
use app_lib::cardano::blockfrost::BlockfrostClient;
use app_lib::cardano::stake_pubkey::{build_registration_tx, stake_address_from_key_hash, Network};
use app_lib::crypto::hash::blake2b_224;
use app_lib::crypto::wallet::wallet_from_mnemonic;
use app_lib::p2p::registry_chain::{BlockfrostFetcher, ChainFetcher};
use pallas_crypto::key::ed25519::SecretKeyExtended;
use pallas_wallet::PrivateKey;
use std::env;
use std::sync::Arc;
use std::time::Duration;

/// Loaded signing material, normalized between the two modes.
struct KeyMaterial {
    payment_address: String,
    stake_address: String,
    payment_key_hash: [u8; 28],
    stake_key_hash: [u8; 28],
    payment_key: PrivateKey,
    stake_key: PrivateKey,
    /// Convenience: the verifying-key bytes of the payment key. Used
    /// as the default `public_key` to register when caller doesn't
    /// override via `ALEXANDRIA_TEST_PUBKEY`.
    payment_vkey_bytes: [u8; 32],
}

fn read_skey_raw(path: &str) -> Result<[u8; 32]> {
    let json = std::fs::read_to_string(path).with_context(|| format!("read skey {path}"))?;
    let v: serde_json::Value = serde_json::from_str(&json)?;
    let cbor_hex = v
        .get("cborHex")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("{path}: missing cborHex"))?;
    let bytes = hex::decode(cbor_hex).with_context(|| format!("{path}: bad cborHex"))?;
    // Cardano-CLI .skey wraps the 32-byte secret as CBOR `58 20 <bytes>`.
    if bytes.len() != 34 || bytes[0] != 0x58 || bytes[1] != 0x20 {
        bail!("{path}: expected CBOR-tagged 32-byte key (58 20 …)");
    }
    Ok(bytes[2..].try_into().expect("checked length"))
}

fn load_keys() -> Result<KeyMaterial> {
    if let Ok(mnemonic) = env::var("ALEXANDRIA_TEST_MNEMONIC") {
        let wallet =
            wallet_from_mnemonic(&mnemonic).map_err(|e| anyhow!("wallet_from_mnemonic: {e}"))?;
        let payment_key = PrivateKey::Extended(unsafe {
            SecretKeyExtended::from_bytes_unchecked(wallet.payment_key_extended)
        });
        let stake_key = PrivateKey::Extended(unsafe {
            SecretKeyExtended::from_bytes_unchecked(wallet.stake_key_extended)
        });
        return Ok(KeyMaterial {
            payment_address: wallet.payment_address.clone(),
            stake_address: wallet.stake_address.clone(),
            payment_key_hash: wallet.payment_key_hash,
            stake_key_hash: wallet.stake_key_hash,
            payment_key,
            stake_key,
            payment_vkey_bytes: wallet.signing_key.verifying_key().to_bytes(),
        });
    }

    let payment_path = env::var("ALEXANDRIA_TEST_PAYMENT_SKEY").context(
        "set ALEXANDRIA_TEST_MNEMONIC or ALEXANDRIA_TEST_PAYMENT_SKEY + ALEXANDRIA_TEST_STAKE_SKEY",
    )?;
    let stake_path = env::var("ALEXANDRIA_TEST_STAKE_SKEY")
        .context("ALEXANDRIA_TEST_STAKE_SKEY required alongside the payment skey")?;
    let address = env::var("ALEXANDRIA_TEST_ADDRESS")
        .context("ALEXANDRIA_TEST_ADDRESS required (bech32 payment address holding the UTxO)")?;

    let payment_raw = read_skey_raw(&payment_path)?;
    let stake_raw = read_skey_raw(&stake_path)?;

    let payment_pk = PrivateKey::Normal(payment_raw.into());
    let stake_pk = PrivateKey::Normal(stake_raw.into());
    let payment_pub = payment_pk.public_key();
    let stake_pub = stake_pk.public_key();
    let payment_pub_bytes: [u8; 32] = payment_pub
        .as_ref()
        .try_into()
        .map_err(|_| anyhow!("payment vkey must be 32 bytes"))?;
    let stake_pub_bytes: [u8; 32] = stake_pub
        .as_ref()
        .try_into()
        .map_err(|_| anyhow!("stake vkey must be 32 bytes"))?;
    let payment_key_hash = blake2b_224(&payment_pub_bytes);
    let stake_key_hash = blake2b_224(&stake_pub_bytes);
    let stake_address = stake_address_from_key_hash(&stake_key_hash, Network::Preprod)
        .map_err(|e| anyhow!("derive stake address: {e}"))?;

    Ok(KeyMaterial {
        payment_address: address,
        stake_address,
        payment_key_hash,
        stake_key_hash,
        payment_key: payment_pk,
        stake_key: stake_pk,
        payment_vkey_bytes: payment_pub_bytes,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let project_id = env::var("BLOCKFROST_PROJECT_ID")
        .context("BLOCKFROST_PROJECT_ID env var required (preprod)")?;
    let valid_secs: i64 = env::var("ALEXANDRIA_TEST_VALID_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(365 * 24 * 3600);

    let km = load_keys()?;
    eprintln!("[smoke] payment_address: {}", km.payment_address);
    eprintln!("[smoke] stake_address  : {}", km.stake_address);

    // The Ed25519 public key we're registering. Defaults to the
    // payment key so the test is self-contained, but the registry
    // semantically binds a stake address to *whatever* gossip-envelope
    // key the operator chooses, so callers MAY substitute their
    // libp2p envelope pubkey instead.
    let pubkey: [u8; 32] = match env::var("ALEXANDRIA_TEST_PUBKEY") {
        Ok(hex_str) => {
            let bytes = hex::decode(hex_str.trim()).context("ALEXANDRIA_TEST_PUBKEY hex")?;
            bytes.try_into().map_err(|v: Vec<u8>| {
                anyhow!(
                    "ALEXANDRIA_TEST_PUBKEY must be 32 bytes (64 hex chars), got {}",
                    v.len()
                )
            })?
        }
        Err(_) => km.payment_vkey_bytes,
    };
    eprintln!("[smoke] binding pubkey: {}", hex::encode(pubkey));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    let valid_from = now - 60;
    // `valid_secs == 0` is the operator-facing escape hatch for an
    // open-ended binding: the on-chain datum stores `valid_until = 0`,
    // which the parser maps to `None` (no expiry). Anything else is a
    // bounded window `now + valid_secs`.
    let valid_until = if valid_secs == 0 { 0 } else { now + valid_secs };
    if valid_until == 0 {
        eprintln!("[smoke] window: [{valid_from}, open-ended)");
    } else {
        eprintln!(
            "[smoke] window: [{valid_from}, {valid_until}) ({} days)",
            (valid_until - valid_from) / 86_400
        );
    }

    let blockfrost = BlockfrostClient::new(project_id.clone())
        .map_err(|e| anyhow!("BlockfrostClient::new: {e}"))?;

    eprintln!("[smoke] building registration tx…");
    let tx_result = build_registration_tx(
        &blockfrost,
        &km.payment_address,
        &km.payment_key_hash,
        &km.payment_key,
        &km.stake_key_hash,
        &km.stake_key,
        &pubkey,
        valid_from,
        valid_until,
        Network::Preprod,
    )
    .await
    .map_err(|e| anyhow!("build_registration_tx: {e}"))?;
    eprintln!(
        "[smoke] built tx — hash {}, {} bytes",
        tx_result.tx_hash,
        tx_result.tx_cbor.len()
    );

    eprintln!("[smoke] submitting to Blockfrost preprod…");
    let submit_hash = blockfrost
        .submit_tx(&tx_result.tx_cbor)
        .await
        .map_err(|e| anyhow!("submit_tx: {e}"))?;
    if submit_hash != tx_result.tx_hash {
        eprintln!(
            "[smoke] WARN: submitted hash {} != locally computed {}",
            submit_hash, tx_result.tx_hash
        );
    }
    eprintln!(
        "✓ submitted https://preprod.cardanoscan.io/transaction/{}",
        submit_hash
    );

    // Wait for confirmation. Preprod blocks land ~20s; poll for up
    // to 4 minutes.
    eprintln!("[smoke] waiting for confirmation (up to 240s)…");
    let mut confirmed = false;
    for _ in 0..24 {
        tokio::time::sleep(Duration::from_secs(10)).await;
        if blockfrost
            .is_tx_confirmed(&submit_hash)
            .await
            .unwrap_or(false)
        {
            confirmed = true;
            break;
        }
        eprintln!("[smoke]   …still pending");
    }
    if !confirmed {
        bail!(
            "tx {} did not confirm within the polling window; check the explorer",
            submit_hash
        );
    }
    eprintln!("[smoke] tx confirmed on-chain");

    // Now drive the production fetcher and assert the entry comes
    // back with `stake_address == km.stake_address` and the
    // pubkey we just bound. This is the exact code path that the
    // refresh task runs in production.
    eprintln!("[smoke] running BlockfrostFetcher.fetch() …");
    let fetcher = BlockfrostFetcher::new(Arc::new(blockfrost), Network::Preprod);
    let entries = fetcher
        .fetch()
        .await
        .map_err(|e| anyhow!("fetcher.fetch: {e}"))?;
    eprintln!(
        "[smoke] fetcher returned {} witness-verified entries",
        entries.len()
    );

    let want_pubkey_hex = hex::encode(pubkey);
    let found = entries.iter().find(|e| {
        e.stake_address == km.stake_address
            && e.public_key_hex.eq_ignore_ascii_case(&want_pubkey_hex)
            && e.on_chain_tx == submit_hash
    });
    match found {
        Some(entry) => {
            println!();
            println!("✓ end-to-end smoke test PASSED");
            println!("  stake_address : {}", entry.stake_address);
            println!("  public_key    : {}", entry.public_key_hex);
            println!("  valid_from    : {}", entry.valid_from);
            println!(
                "  valid_until   : {}",
                entry
                    .valid_until
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "open-ended".into())
            );
            println!("  on_chain_tx   : {}", entry.on_chain_tx);
        }
        None => {
            eprintln!("[smoke] fetched entries:");
            for e in &entries {
                eprintln!(
                    "  - stake={} pubkey={} tx={}",
                    e.stake_address, e.public_key_hex, e.on_chain_tx
                );
            }
            bail!(
                "expected entry for stake={} pubkey={} tx={} not found — \
                 either the witness check rejected it (sign-with-stake-key bug) \
                 or Blockfrost hasn't indexed the UTxO yet",
                km.stake_address,
                want_pubkey_hex,
                submit_hash
            );
        }
    }

    Ok(())
}
