//! Deploy the `completion_minting` validator as a CIP-33 reference
//! script on Cardano preprod.
//!
//! Mirrors `preprod_anchor.rs`: takes the mark2-style `PaymentSigningKeyShelley_ed25519`
//! treasury skey + bech32 address, queries Blockfrost for state, builds
//! a Conway tx that locks ~5 ADA at the deployer's address with the
//! compiled `completion_minting` script attached as `script_ref`,
//! signs, and submits.
//!
//! Bypasses `cardano-cli` so it works without a local node socket —
//! all chain queries go through Blockfrost.
//!
//! Usage:
//!   BLOCKFROST_PROJECT_ID=<preprod-id> \
//!       cargo run --manifest-path src-tauri/Cargo.toml \
//!       --example deploy_completion -- \
//!       --key /path/to/treasury.skey \
//!       --addr addr_test1...
//!
//! On success, prints the (tx_hash, output_index) pair to drop into
//! `script_refs::COMPLETION_MINTING_REF_UTXO`.

use std::env;

use anyhow::{anyhow, bail, Context, Result};
use app_lib::cardano::blockfrost::BlockfrostClient;
use app_lib::cardano::tx_builder::{self, MIN_UTXO_LOVELACE, TTL_OFFSET};
use pallas_addresses::Address as PallasAddress;
use pallas_codec::utils::{Bytes, CborWrap};
use pallas_primitives::conway::{
    NativeScript, PlutusScript, PostAlonzoTransactionOutput, PseudoScript, PseudoTransactionOutput,
    Tx, Value,
};
use pallas_primitives::Fragment;
use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
use pallas_wallet::PrivateKey;

/// 8 ADA — Conway era charges roughly `coins_per_utxo_byte` × full
/// output size (which includes the inlined script body). For a ~1.2KB
/// PlutusV3 payload preprod's min-UTxO works out to ~6.4 ADA; 8 ADA
/// gives comfortable headroom above the floor without locking up too
/// much tADA on a one-shot deploy.
const REF_SCRIPT_LOVELACE: u64 = 8_000_000;

#[derive(Default)]
struct Args {
    key_path: Option<String>,
    address: Option<String>,
    plutus_json: Option<String>,
    validator_title: Option<String>,
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
            "--plutus-json" if i + 1 < argv.len() => {
                out.plutus_json = Some(argv[i + 1].clone());
                i += 2;
            }
            "--title" if i + 1 < argv.len() => {
                out.validator_title = Some(argv[i + 1].clone());
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
    let plutus_json_path = args
        .plutus_json
        .unwrap_or_else(|| "cardano/governance/plutus.json".into());
    let validator_title = args
        .validator_title
        .unwrap_or_else(|| "completion.completion_minting.mint".into());

    let project_id = env::var("BLOCKFROST_PROJECT_ID")
        .context("BLOCKFROST_PROJECT_ID env var must be set to a preprod project id")?;

    // 1. Load the compiled validator bytes from plutus.json. These
    //    are the raw Plutus V3 program bytes that go on-chain as the
    //    `script_ref` field's PlutusV3Script payload.
    let plutus_json = std::fs::read_to_string(&plutus_json_path)
        .with_context(|| format!("read plutus.json at {plutus_json_path}"))?;
    let plutus: serde_json::Value = serde_json::from_str(&plutus_json)?;
    let compiled_hex = plutus
        .get("validators")
        .and_then(|vs| vs.as_array())
        .and_then(|vs| {
            vs.iter()
                .find(|v| v.get("title").and_then(|t| t.as_str()) == Some(validator_title.as_str()))
        })
        .and_then(|v| v.get("compiledCode").and_then(|c| c.as_str()))
        .ok_or_else(|| anyhow!("validator '{validator_title}' not found in {plutus_json_path}"))?;
    let compiled_bytes =
        hex::decode(compiled_hex).with_context(|| "compiledCode is not valid hex")?;
    eprintln!(
        "[deploy_completion] {} bytes of Plutus V3 bytecode loaded from {}",
        compiled_bytes.len(),
        plutus_json_path
    );

    // 2. Load the Shelley payment skey (raw 32-byte ed25519 inside a
    //    CBOR `5820 …` envelope, identical to mark2's treasury keys).
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

    // 3. Query chain state.
    let blockfrost = BlockfrostClient::new(project_id.clone())
        .map_err(|e| anyhow!("BlockfrostClient::new: {e}"))?;
    eprintln!("[deploy_completion] querying chain state for {address}");
    let (utxos_res, params_res, tip_res) = tokio::join!(
        blockfrost.get_utxos(&address),
        blockfrost.get_protocol_params(),
        blockfrost.get_tip_slot(),
    );
    let utxos = utxos_res.map_err(|e| anyhow!("get_utxos: {e}"))?;
    let params = params_res.map_err(|e| anyhow!("get_protocol_params: {e}"))?;
    let tip_slot = tip_res.map_err(|e| anyhow!("get_tip_slot: {e}"))?;
    if utxos.is_empty() {
        bail!("no UTxOs at {address}");
    }
    // Reference-script outputs need a fatter UTxO than ordinary anchors;
    // budget for the ~1.2KB script body plus fee + change minimum.
    let min_input = REF_SCRIPT_LOVELACE + MIN_UTXO_LOVELACE;
    let selected = BlockfrostClient::select_utxo(&utxos, min_input)
        .ok_or_else(|| anyhow!("no UTxO with ≥ {min_input} lovelace"))?;
    eprintln!(
        "[deploy_completion] selected UTxO {}#{} ({} lovelace)",
        selected.tx_hash,
        selected.tx_index,
        selected.lovelace()
    );

    // 4. Skeleton tx: one input → reference-script UTxO + change.
    //    pallas-txbuilder doesn't surface `script_ref` on Output, so
    //    we attach it post-build with the same decode-modify-reencode
    //    trick used by `tx_builder::inject_metadata`.
    let pallas_addr =
        PallasAddress::from_bech32(&address).map_err(|e| anyhow!("bad address: {e}"))?;
    // Plutus V3 reference scripts cost more to include. Bump the fee
    // floor accordingly — ~600 lovelace per byte of script body plus
    // the usual base + tx-size component.
    let fee_floor = (compiled_bytes.len() as u64) * 600 + 350_000;
    let fee = tx_builder::estimate_fee(&params, 1).max(fee_floor);
    let input_lovelace = selected.lovelace();
    let needed = REF_SCRIPT_LOVELACE + fee + MIN_UTXO_LOVELACE;
    if input_lovelace < needed {
        bail!(
            "insufficient funds: need {} lovelace (ref={} + fee={} + change={}), have {}",
            needed,
            REF_SCRIPT_LOVELACE,
            fee,
            MIN_UTXO_LOVELACE,
            input_lovelace
        );
    }
    let change = input_lovelace - REF_SCRIPT_LOVELACE - fee;
    let input_tx_hash =
        tx_builder::parse_tx_hash(&selected.tx_hash).map_err(|e| anyhow!("parse tx hash: {e}"))?;

    // BLAKE2b-224 of the 32-byte pubkey for `disclosed_signer`.
    let pk_bytes: [u8; 32] = pub_key.as_ref().try_into().unwrap();
    let key_hash_28 = blake2b_224(&pk_bytes);

    let staging = StagingTransaction::new()
        .input(Input::new(input_tx_hash, selected.tx_index))
        // Output 0: ref-script UTxO at the deployer's own address.
        .output(Output::new(pallas_addr.clone(), REF_SCRIPT_LOVELACE))
        // Output 1: change.
        .output(Output::new(pallas_addr, change))
        .disclosed_signer(pallas_crypto::hash::Hash::<28>::from(key_hash_28))
        .fee(fee)
        .invalid_from_slot(tip_slot + TTL_OFFSET)
        .network_id(0);

    let built = staging
        .build_conway_raw()
        .map_err(|e| anyhow!("build_conway_raw: {e}"))?;

    // 5. Inject `script_ref` on output 0.
    let with_script_ref = inject_script_ref_on_output_zero(&built.tx_bytes.0, &compiled_bytes)?;

    let signed_cbor = tx_builder::sign_raw_tx(&with_script_ref, &private_key)
        .map_err(|e| anyhow!("sign: {e}"))?;
    let local_tx_hash =
        tx_builder::compute_tx_hash(&signed_cbor).map_err(|e| anyhow!("compute_tx_hash: {e}"))?;

    eprintln!("[deploy_completion] signed tx:");
    eprintln!("  local tx hash: {}", local_tx_hash);
    eprintln!("  payload size : {} bytes", signed_cbor.len());
    eprintln!("  fee          : {} lovelace", fee);
    eprintln!(
        "  ref-script   : output #0, {} lovelace",
        REF_SCRIPT_LOVELACE
    );

    eprintln!("\n[deploy_completion] submitting via Blockfrost…");
    let submit_hash = blockfrost
        .submit_tx(&signed_cbor)
        .await
        .map_err(|e| anyhow!("submit_tx: {e}"))?;

    println!("\n✓ submitted");
    println!("  tx_hash: {submit_hash}");
    println!("  output_index: 0");
    println!("  view: https://preprod.cardanoscan.io/transaction/{submit_hash}");
    println!();
    println!("Update src-tauri/src/cardano/script_refs.rs:");
    println!("  pub const COMPLETION_MINTING_REF_UTXO: (&str, u64) = (\"{submit_hash}\", 0);");

    Ok(())
}

/// Decode the Conway tx, set `script_ref` on output index 0 to the
/// supplied PlutusV3 script bytes, then re-encode. Mirrors the
/// decode-modify-reencode pattern used by
/// `tx_builder::inject_metadata` and `gov_tx_builder::inject_plutus_fields`.
fn inject_script_ref_on_output_zero(
    tx_bytes: &[u8],
    compiled_script_bytes: &[u8],
) -> Result<Vec<u8>> {
    let mut tx = Tx::decode_fragment(tx_bytes).map_err(|e| anyhow!("decode tx: {e}"))?;

    let plutus_v3 = PlutusScript::<3>(Bytes::from(compiled_script_bytes.to_vec()));
    let script_ref: PseudoScript<NativeScript> = PseudoScript::PlutusV3Script(plutus_v3);
    let cbor_wrapped = CborWrap(script_ref);

    let outputs = &mut tx.transaction_body.outputs;
    let first = outputs
        .first_mut()
        .ok_or_else(|| anyhow!("tx has no outputs"))?;
    match first {
        PseudoTransactionOutput::Legacy(_) => {
            // Promote to PostAlonzo so we can attach script_ref.
            let legacy = match first {
                PseudoTransactionOutput::Legacy(l) => l.clone(),
                _ => unreachable!(),
            };
            let post = PostAlonzoTransactionOutput {
                address: legacy.address,
                value: match legacy.amount {
                    pallas_primitives::alonzo::Value::Coin(c) => Value::Coin(c),
                    pallas_primitives::alonzo::Value::Multiasset(c, _) => Value::Coin(c),
                },
                datum_option: None,
                script_ref: Some(cbor_wrapped),
            };
            *first = PseudoTransactionOutput::PostAlonzo(post);
        }
        PseudoTransactionOutput::PostAlonzo(p) => {
            p.script_ref = Some(cbor_wrapped);
        }
    }

    tx.encode_fragment()
        .map_err(|e| anyhow!("re-encode tx: {e}"))
}

/// BLAKE2b-224 → 28-byte Cardano payment key hash (pkh).
fn blake2b_224(bytes: &[u8]) -> [u8; 28] {
    use blake2::digest::{Update, VariableOutput};
    let mut hasher = blake2::Blake2bVar::new(28).expect("blake2-224");
    hasher.update(bytes);
    let mut out = [0u8; 28];
    hasher.finalize_variable(&mut out).expect("finalize");
    out
}
