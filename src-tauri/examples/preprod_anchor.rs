//! Preprod live-submission smoke test for PR 16's credential anchor.
//!
//! Validates `cardano::anchor_tx::build_anchor_metadata` +
//! `tx_builder::inject_metadata` end-to-end by submitting a real
//! metadata-only tx to Cardano preprod under `ALEXANDRIA_ANCHOR_LABEL`
//! (1697). Uses the mark2 treasury key (a raw Shelley Ed25519 key,
//! not a BIP32 extended key), so it bypasses `Wallet` and signs
//! directly via `PrivateKey::Normal`.
//!
//! Usage:
//!   BLOCKFROST_PROJECT_ID=<preprod-id> \
//!       cargo run --manifest-path src-tauri/Cargo.toml \
//!       --example preprod_anchor -- \
//!       --key /path/to/treasury.skey \
//!       --addr addr_test1... \
//!       [--credential-hash <hex>]
//!
//! On success, prints the submitted tx hash. Look for it on
//! https://preprod.cardanoscan.io/transaction/<hash> — metadata
//! should appear under label 1697.

use std::env;

use anyhow::{anyhow, bail, Context, Result};
use app_lib::cardano::anchor_tx::build_anchor_metadata;
use app_lib::cardano::blockfrost::BlockfrostClient;
use app_lib::cardano::tx_builder::{self, MIN_UTXO_LOVELACE, TTL_OFFSET};
use app_lib::crypto::did::Did;
use pallas_addresses::Address as PallasAddress;
use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
use pallas_wallet::PrivateKey;

#[derive(Default)]
struct Args {
    key_path: Option<String>,
    address: Option<String>,
    credential_hash: Option<String>,
    issuer_did: Option<String>,
}

fn parse_args() -> Args {
    let mut out = Args::default();
    let argv: Vec<String> = env::args().collect();
    let mut i = 1usize;
    while i < argv.len() {
        match argv[i].as_str() {
            "--key" if i + 1 < argv.len() => {
                out.key_path = Some(argv[i + 1].clone());
                i += 2;
            }
            "--addr" if i + 1 < argv.len() => {
                out.address = Some(argv[i + 1].clone());
                i += 2;
            }
            "--credential-hash" if i + 1 < argv.len() => {
                out.credential_hash = Some(argv[i + 1].clone());
                i += 2;
            }
            "--issuer-did" if i + 1 < argv.len() => {
                out.issuer_did = Some(argv[i + 1].clone());
                i += 2;
            }
            _ => i += 1,
        }
    }
    out
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args();
    let key_path = args
        .key_path
        .ok_or_else(|| anyhow!("--key <path/to/treasury.skey> is required"))?;
    let address = args
        .address
        .ok_or_else(|| anyhow!("--addr <addr_test1…> is required"))?;
    let credential_hash = args
        .credential_hash
        .unwrap_or_else(|| "pr16-preprod-smoke".into());
    let issuer_did = Did(args
        .issuer_did
        .unwrap_or_else(|| "did:key:zPR16PreprodSmoke".into()));
    let issued_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let project_id = env::var("BLOCKFROST_PROJECT_ID")
        .context("BLOCKFROST_PROJECT_ID env var must be set to a preprod project id")?;

    // Load the Shelley Ed25519 key. Format: Cardano CLI .skey JSON
    // with `cborHex` = 5820 + 32 raw bytes (total 68 hex chars).
    let skey_json = std::fs::read_to_string(&key_path)
        .with_context(|| format!("read key file at {key_path}"))?;
    let parsed: serde_json::Value = serde_json::from_str(&skey_json)?;
    let cbor_hex = parsed
        .get("cborHex")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing cborHex in {key_path}"))?;
    let cbor_bytes = hex::decode(cbor_hex).context("bad hex in cborHex")?;
    if cbor_bytes.len() != 34 || cbor_bytes[0] != 0x58 || cbor_bytes[1] != 0x20 {
        bail!("expected CBOR-tagged 32-byte payment key (58 20 …); got {cbor_hex}");
    }
    let raw_key: [u8; 32] = cbor_bytes[2..].try_into().unwrap();
    let private_key = PrivateKey::Normal(raw_key.into());
    let pub_key = private_key.public_key();
    eprintln!(
        "[preprod_anchor] payment pubkey: {}",
        hex::encode(pub_key.as_ref())
    );

    let blockfrost = BlockfrostClient::new(project_id.clone())
        .map_err(|e| anyhow!("BlockfrostClient::new: {e}"))?;

    // Pull UTxOs + chain tip + protocol params.
    eprintln!("[preprod_anchor] querying chain state for {address}");
    let (utxos_res, params_res, tip_res) = tokio::join!(
        blockfrost.get_utxos(&address),
        blockfrost.get_protocol_params(),
        blockfrost.get_tip_slot(),
    );
    let utxos = utxos_res.map_err(|e| anyhow!("get_utxos: {e}"))?;
    let params = params_res.map_err(|e| anyhow!("get_protocol_params: {e}"))?;
    let tip_slot = tip_res.map_err(|e| anyhow!("get_tip_slot: {e}"))?;
    if utxos.is_empty() {
        bail!("no UTxOs at {address} — wallet is empty?");
    }
    let selected = BlockfrostClient::select_utxo(&utxos, MIN_UTXO_LOVELACE)
        .ok_or_else(|| anyhow!("no UTxO with ≥ {MIN_UTXO_LOVELACE} lovelace"))?;
    eprintln!(
        "[preprod_anchor] selected UTxO {}#{} with {} lovelace",
        selected.tx_hash,
        selected.tx_index,
        selected.lovelace()
    );

    // Build the tx body. One input → one output (less fee), no mint.
    let pallas_addr =
        PallasAddress::from_bech32(&address).map_err(|e| anyhow!("bad address: {e}"))?;
    let fee = tx_builder::estimate_fee(&params, 1);
    let input_lovelace = selected.lovelace();
    if input_lovelace < fee + MIN_UTXO_LOVELACE {
        bail!(
            "insufficient funds: need {} lovelace, have {}",
            fee + MIN_UTXO_LOVELACE,
            input_lovelace
        );
    }
    let change = input_lovelace - fee;
    let input_tx_hash =
        tx_builder::parse_tx_hash(&selected.tx_hash).map_err(|e| anyhow!("parse tx hash: {e}"))?;

    // For PrivateKey::Normal, the 28-byte key hash is BLAKE2b-224 of
    // the 32-byte public key bytes.
    let pk_bytes: [u8; 32] = pub_key.as_ref().try_into().unwrap();
    let key_hash_28 = blake2b_224(&pk_bytes);
    let staging = StagingTransaction::new()
        .input(Input::new(input_tx_hash, selected.tx_index))
        .output(Output::new(pallas_addr, change))
        .disclosed_signer(pallas_crypto::hash::Hash::<28>::from(key_hash_28))
        .fee(fee)
        .invalid_from_slot(tip_slot + TTL_OFFSET)
        .network_id(0); // preprod

    let built = staging
        .build_conway_raw()
        .map_err(|e| anyhow!("build_conway_raw: {e}"))?;

    // Inject the label-1697 metadata via our own builder — this is
    // the code PR 16 added.
    let metadata = build_anchor_metadata(&credential_hash, &issuer_did, &issued_at);
    let (with_metadata, _hash_after_inject) =
        tx_builder::inject_metadata(&built.tx_bytes.0, metadata)
            .map_err(|e| anyhow!("inject_metadata: {e}"))?;

    let signed_cbor =
        tx_builder::sign_raw_tx(&with_metadata, &private_key).map_err(|e| anyhow!("sign: {e}"))?;
    let local_tx_hash =
        tx_builder::compute_tx_hash(&signed_cbor).map_err(|e| anyhow!("compute_tx_hash: {e}"))?;

    eprintln!("[preprod_anchor] signed tx:");
    eprintln!("  local tx hash : {}", local_tx_hash);
    eprintln!("  payload size  : {} bytes", signed_cbor.len());
    eprintln!("  fee           : {} lovelace", fee);
    eprintln!("  change out    : {} lovelace", change);
    eprintln!("  metadata label: 1697 (ALEXANDRIA_ANCHOR_LABEL)");
    eprintln!("  anchor payload:");
    eprintln!("    credential_hash = {credential_hash}");
    eprintln!("    issuer_did      = {}", issuer_did.as_str());
    eprintln!("    issued_at       = {issued_at}");

    eprintln!("\n[preprod_anchor] submitting to Blockfrost preprod…");
    let submit_hash = blockfrost
        .submit_tx(&signed_cbor)
        .await
        .map_err(|e| anyhow!("submit_tx: {e}"))?;

    println!("\n✓ submitted");
    println!("  tx_hash: {submit_hash}");
    println!("  view:    https://preprod.cardanoscan.io/transaction/{submit_hash}");
    println!("  metadata: https://preprod.cardanoscan.io/transaction/{submit_hash}?tab=metadata");
    if submit_hash != local_tx_hash {
        eprintln!(
            "  note: local hash differs from chain hash — this is expected when \
             Blockfrost normalizes the CBOR"
        );
    }

    Ok(())
}

/// BLAKE2b-224 → 28-byte Cardano payment key hash. Local
/// implementation to keep the example free of extra deps (blake2 is
/// already a transitive dep via pallas / iroh).
fn blake2b_224(bytes: &[u8]) -> [u8; 28] {
    use blake2::digest::{Update, VariableOutput};
    let mut hasher = blake2::Blake2bVar::new(28).expect("blake2-224");
    hasher.update(bytes);
    let mut out = [0u8; 28];
    hasher.finalize_variable(&mut out).expect("finalize");
    out
}
