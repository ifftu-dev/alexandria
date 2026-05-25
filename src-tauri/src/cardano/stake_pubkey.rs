//! On-chain stake-address → public-key registration plumbing.
//!
//! Counterpart to `cardano/governance/validators/stake_pubkey_registration.ak`.
//! Provides the constants, address derivation, and Plutus Data
//! encode/decode the chain-side registry uses to discover and parse
//! registration UTxOs.
//!
//! Datum shape (mirrors the Aiken type):
//!
//! ```text
//! Constr 0
//!   [ Bytes(stake_key_hash : 28)
//!   , Bytes(public_key      : 32)
//!   , Int(valid_from        : POSIX seconds)
//!   , Int(valid_until       : POSIX seconds or 0 = open-ended)
//!   ]
//! ```
//!
//! Stake addresses are bech32-encoded `e1`-prefixed (mainnet) or
//! `e0`-prefixed (testnet) stake credentials. We accept both during
//! decode and emit whichever matches the [`Network`] passed in.

use crate::cardano::blockfrost::BlockfrostClient;
use crate::cardano::gov_tx_builder::inject_plutus_fields;
use crate::cardano::tx_builder::{parse_tx_hash, sign_raw_tx, TxBuildError, TTL_OFFSET};
use pallas_addresses::{Address as PallasAddress, StakePayload};
use pallas_codec::minicbor::data::{Tag, Type};
use pallas_crypto::hash::Hash;
use pallas_primitives::{conway::Tx, Fragment};
use pallas_traverse::ComputeHash;
use pallas_txbuilder::{BuildConway, Input, Output, StagingTransaction};
use pallas_wallet::PrivateKey;

/// Blake2b-224 hash of the compiled `stake_pubkey_registration`
/// validator, extracted from
/// `cardano/governance/plutus.json` at build time. Regenerate this
/// constant if the validator source changes.
pub const SCRIPT_HASH_HEX: &str = "fc20070d1e5379403add6acbf77b233b2f8240821c187b398525de28";

/// Which Cardano network the on-chain registry lives on. Preprod for
/// testing, Mainnet for production launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Mainnet,
    Preprod,
}

impl Network {
    fn header_byte(self) -> u8 {
        // Enterprise script address: type 7 (script, no stake part).
        // 0x71 = type 7 + network 1 (mainnet); 0x70 = type 7 + network 0 (testnet).
        match self {
            Network::Mainnet => 0x71,
            Network::Preprod => 0x70,
        }
    }
}

/// Build the bech32 enterprise script address for the registration
/// validator on the given network.
pub fn script_address(network: Network) -> Result<String, TxBuildError> {
    let hash_bytes = hex::decode(SCRIPT_HASH_HEX)
        .map_err(|e| TxBuildError::AddressParse(format!("script hash hex: {e}")))?;
    if hash_bytes.len() != 28 {
        return Err(TxBuildError::AddressParse(format!(
            "script hash must be 28 bytes, got {}",
            hash_bytes.len()
        )));
    }
    let mut addr_bytes = Vec::with_capacity(29);
    addr_bytes.push(network.header_byte());
    addr_bytes.extend_from_slice(&hash_bytes);
    let addr = PallasAddress::from_bytes(&addr_bytes)
        .map_err(|e| TxBuildError::AddressParse(format!("address bytes: {e}")))?;
    addr.to_bech32()
        .map_err(|e| TxBuildError::AddressParse(format!("bech32 encode: {e}")))
}

/// Convert a bech32 stake address (`stake1u…` mainnet,
/// `stake_test1u…` testnet) into the 28-byte stake-key hash that
/// appears in the on-chain datum. Returns `None` on invalid bech32 or
/// non-stake (script-credential, payment) addresses.
pub fn stake_key_hash_from_bech32(stake_address: &str) -> Option<Vec<u8>> {
    let parsed = PallasAddress::from_bech32(stake_address).ok()?;
    let stake = match parsed {
        PallasAddress::Stake(s) => s,
        _ => return None,
    };
    match stake.payload() {
        StakePayload::Stake(h) => Some(h.to_vec()),
        StakePayload::Script(_) => None,
    }
}

/// Stake-address header byte: type 14 (stake key) + network nibble.
fn stake_header_byte(network: Network) -> u8 {
    match network {
        Network::Mainnet => 0xe1, // 0b1110_0001
        Network::Preprod => 0xe0, // 0b1110_0000
    }
}

/// Convert a 28-byte stake-key hash back into the bech32 stake
/// address for the given network. Constructs the raw 29-byte
/// representation (header + key hash) and lets pallas decode it back
/// into a [`StakeAddress`] so we don't need to touch the private
/// fields of the tuple struct.
pub fn stake_address_from_key_hash(
    key_hash: &[u8],
    network: Network,
) -> Result<String, TxBuildError> {
    if key_hash.len() != 28 {
        return Err(TxBuildError::AddressParse(format!(
            "stake key hash must be 28 bytes, got {}",
            key_hash.len()
        )));
    }
    let mut addr_bytes = Vec::with_capacity(29);
    addr_bytes.push(stake_header_byte(network));
    addr_bytes.extend_from_slice(key_hash);
    let addr = PallasAddress::from_bytes(&addr_bytes)
        .map_err(|e| TxBuildError::AddressParse(format!("stake address bytes: {e}")))?;
    addr.to_bech32()
        .map_err(|e| TxBuildError::AddressParse(format!("bech32 encode: {e}")))
}

/// Rust mirror of the on-chain `StakePubkeyRegistrationDatum`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StakePubkeyRegistrationDatum {
    pub stake_key_hash: [u8; 28],
    pub public_key: [u8; 32],
    pub valid_from: i64,
    /// `0` on-chain encodes "open-ended".
    pub valid_until: i64,
}

/// CBOR-encode the datum as Plutus Data:
/// `Constr 0 [bytes, bytes, int, int]`.
pub fn encode_datum(d: &StakePubkeyRegistrationDatum) -> Result<Vec<u8>, TxBuildError> {
    let mut buf = Vec::new();
    let mut enc = pallas_codec::minicbor::Encoder::new(&mut buf);
    enc.tag(Tag::new(121))
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    enc.array(4)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    enc.bytes(&d.stake_key_hash)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    enc.bytes(&d.public_key)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    enc.i64(d.valid_from)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    enc.i64(d.valid_until)
        .map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    Ok(buf)
}

/// Decode the Plutus Data CBOR back into a [`StakePubkeyRegistrationDatum`].
/// Tolerates either definite or indefinite-length encodings for the
/// outer array — both are valid CBOR.
pub fn decode_datum(bytes: &[u8]) -> Result<StakePubkeyRegistrationDatum, TxBuildError> {
    let mut dec = pallas_codec::minicbor::Decoder::new(bytes);
    let tag = dec.tag().map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    if tag.as_u64() != 121 {
        return Err(TxBuildError::Cbor(format!(
            "expected Constr 0 (tag 121), got tag {}",
            tag.as_u64()
        )));
    }
    // Read the array header — either definite [Type::Array] with len = 4
    // or indefinite [Type::ArrayIndef] terminated by a Break.
    let (definite_len, indefinite) = match dec.datatype() {
        Ok(Type::Array) => {
            let len = dec.array().map_err(|e| TxBuildError::Cbor(e.to_string()))?;
            (len, false)
        }
        Ok(Type::ArrayIndef) => {
            dec.array().map_err(|e| TxBuildError::Cbor(e.to_string()))?;
            (None, true)
        }
        other => {
            return Err(TxBuildError::Cbor(format!("expected array, got {other:?}")));
        }
    };
    if let Some(len) = definite_len {
        if len != 4 {
            return Err(TxBuildError::Cbor(format!(
                "expected 4 datum fields, got {len}"
            )));
        }
    }
    let stake_bytes = dec.bytes().map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    if stake_bytes.len() != 28 {
        return Err(TxBuildError::Cbor(format!(
            "stake_key_hash must be 28 bytes, got {}",
            stake_bytes.len()
        )));
    }
    let mut stake_key_hash = [0u8; 28];
    stake_key_hash.copy_from_slice(stake_bytes);

    let pk_bytes = dec.bytes().map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    if pk_bytes.len() != 32 {
        return Err(TxBuildError::Cbor(format!(
            "public_key must be 32 bytes, got {}",
            pk_bytes.len()
        )));
    }
    let mut public_key = [0u8; 32];
    public_key.copy_from_slice(pk_bytes);

    let valid_from = dec.i64().map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    let valid_until = dec.i64().map_err(|e| TxBuildError::Cbor(e.to_string()))?;

    if indefinite {
        // Drain the break byte (`0xff`); minicbor returns it via skip.
        dec.skip().map_err(|e| TxBuildError::Cbor(e.to_string()))?;
    }

    Ok(StakePubkeyRegistrationDatum {
        stake_key_hash,
        public_key,
        valid_from,
        valid_until,
    })
}

/// Minimum ADA for the registration UTxO. Picked to clear the
/// post-Babbage min-UTxO floor for an output carrying an inline datum
/// (~3 ADA) without being wasteful.
pub const REGISTRATION_UTXO_LOVELACE: u64 = 3_000_000;

/// Minimum lovelace we require in the source UTxO to cover both the
/// registration output and the network fee with room to spare.
pub const MIN_FUNDING_LOVELACE: u64 = 5_000_000;

/// Result of [`build_registration_tx`].
#[derive(Debug)]
pub struct RegistrationTxResult {
    /// Fully signed transaction CBOR ready for `blockfrost.submit_tx`.
    pub tx_cbor: Vec<u8>,
    /// Hex-encoded transaction hash.
    pub tx_hash: String,
}

/// Build, sign, and return the CBOR for a stake-pubkey registration
/// transaction.
///
/// The tx:
/// - spends one wallet UTxO at `payment_address`
/// - creates a new output at the [`script_address`] for `network` with
///   the [`StakePubkeyRegistrationDatum`] as inline datum
/// - returns change to the payment address
/// - lists both `payment_key_hash` and `stake_key_hash` as disclosed
///   signers so the produced tx body carries vkey-witness expectations
/// - is signed by both the payment key (to authorise the input spend)
///   and the stake key (to authorise the binding claim)
///
/// Submission is the caller's responsibility — pass `tx_cbor` to
/// `BlockfrostClient::submit_tx`. Caller should also record the
/// returned hash in the local `stake_pubkey_registry` table with
/// `source = 'snapshot'` until the chain refresh promotes it to
/// `'chain'`.
#[allow(clippy::too_many_arguments)]
pub async fn build_registration_tx(
    blockfrost: &BlockfrostClient,
    payment_address: &str,
    payment_key_hash: &[u8; 28],
    payment_key: &PrivateKey,
    stake_key_hash: &[u8; 28],
    stake_key: &PrivateKey,
    public_key: &[u8; 32],
    valid_from: i64,
    valid_until: i64,
    network: Network,
) -> Result<RegistrationTxResult, TxBuildError> {
    // 1. Query chain state in parallel.
    let (utxos_res, params_res, tip_res) = tokio::join!(
        blockfrost.get_utxos(payment_address),
        blockfrost.get_protocol_params(),
        blockfrost.get_tip_slot(),
    );
    let utxos = utxos_res?;
    let params = params_res?;
    let tip_slot = tip_res?;
    if utxos.is_empty() {
        return Err(TxBuildError::NoUtxos);
    }
    let selected = BlockfrostClient::select_utxo(&utxos, MIN_FUNDING_LOVELACE).ok_or(
        TxBuildError::InsufficientFunds {
            needed: MIN_FUNDING_LOVELACE,
            available: utxos.iter().map(|u| u.lovelace()).sum(),
        },
    )?;

    // 2. Parse addresses.
    let pallas_addr = PallasAddress::from_bech32(payment_address)
        .map_err(|e| TxBuildError::AddressParse(e.to_string()))?;
    let script_addr_bech32 = script_address(network)?;
    let script_addr = PallasAddress::from_bech32(&script_addr_bech32)
        .map_err(|e| TxBuildError::AddressParse(e.to_string()))?;
    let input_tx_hash = parse_tx_hash(&selected.tx_hash)?;

    // 3. Fee — slightly inflated to absorb the second witness.
    let fee = params.calculate_min_fee(700).max(300_000);
    let input_lovelace = selected.lovelace();
    let needed = REGISTRATION_UTXO_LOVELACE + fee;
    if input_lovelace < needed {
        return Err(TxBuildError::InsufficientFunds {
            needed,
            available: input_lovelace,
        });
    }
    let change = input_lovelace - REGISTRATION_UTXO_LOVELACE - fee;

    // 4. Build the tx skeleton. We rely on `inject_plutus_fields` for
    //    the inline-datum insertion since pallas-txbuilder doesn't
    //    surface that field directly.
    let network_id: u8 = match network {
        Network::Mainnet => 1,
        Network::Preprod => 0,
    };
    let staging_tx = StagingTransaction::new()
        .input(Input::new(input_tx_hash, selected.tx_index))
        .output(Output::new(script_addr, REGISTRATION_UTXO_LOVELACE))
        .output(Output::new(pallas_addr, change))
        .disclosed_signer(Hash::<28>::from(*payment_key_hash))
        .disclosed_signer(Hash::<28>::from(*stake_key_hash))
        .fee(fee)
        .invalid_from_slot(tip_slot + TTL_OFFSET)
        .network_id(network_id);

    let built_tx = staging_tx
        .build_conway_raw()
        .map_err(|e| TxBuildError::Builder(e.to_string()))?;

    // 5. Inject the inline datum on output 0 (the script output).
    let datum = StakePubkeyRegistrationDatum {
        stake_key_hash: *stake_key_hash,
        public_key: *public_key,
        valid_from,
        valid_until,
    };
    let datum_cbor = encode_datum(&datum)?;
    let (tx_with_datum, _) =
        inject_plutus_fields(&built_tx.tx_bytes.0, &[], &[], &[], Some(&datum_cbor))?;

    // 6. Sign with the payment key (input authorisation) and the
    //    stake key (binding authority). `sign_raw_tx` appends one
    //    VKeyWitness per call. Callers pass `PrivateKey::Extended`
    //    for BIP32 wallet keys and `PrivateKey::Normal` for raw
    //    32-byte Shelley `.skey` files.
    let signed_once = sign_raw_tx(&tx_with_datum, payment_key)?;
    let signed_twice = sign_raw_tx(&signed_once, stake_key)?;

    // 7. Compute tx hash and return.
    let tx =
        Tx::decode_fragment(&signed_twice).map_err(|e| TxBuildError::TxDecode(e.to_string()))?;
    let tx_hash = hex::encode(tx.transaction_body.compute_hash().as_ref());

    Ok(RegistrationTxResult {
        tx_cbor: signed_twice,
        tx_hash,
    })
}

/// Outcome of [`tx_witnesses_include_stake_key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WitnessCheck {
    /// The transaction's vkey witness set contains a key whose
    /// Blake2b-224 hash matches `stake_key_hash`.
    SignedByStakeKey,
    /// The witness set is present but does not contain a matching key.
    NoMatchingWitness,
    /// The witness set could not be decoded — we can't make a claim
    /// either way. Caller should treat this as `NoMatchingWitness`.
    Undecodable,
}

/// Verify that the raw CBOR of a Conway-era transaction contains a
/// vkey witness whose key hashes (Blake2b-224) to `stake_key_hash`.
///
/// This is the chain-side proof that the holder of the stake key
/// authorized the registration UTxO. Any entry whose creating tx
/// fails this check MUST be rejected by the registry sync path.
pub fn tx_witnesses_include_stake_key(tx_cbor: &[u8], stake_key_hash: &[u8; 28]) -> WitnessCheck {
    use crate::crypto::hash::blake2b_224;
    use pallas_codec::minicbor;
    use pallas_primitives::conway::Tx;

    let tx: Tx = match minicbor::decode(tx_cbor) {
        Ok(t) => t,
        Err(_) => return WitnessCheck::Undecodable,
    };
    let Some(witnesses) = tx.transaction_witness_set.vkeywitness.as_ref() else {
        return WitnessCheck::NoMatchingWitness;
    };
    for w in witnesses.iter() {
        let key_bytes: &[u8] = w.vkey.as_ref();
        if key_bytes.len() != 32 {
            continue;
        }
        let derived = blake2b_224(key_bytes);
        if &derived == stake_key_hash {
            return WitnessCheck::SignedByStakeKey;
        }
    }
    WitnessCheck::NoMatchingWitness
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_datum() -> StakePubkeyRegistrationDatum {
        StakePubkeyRegistrationDatum {
            stake_key_hash: [0xab; 28],
            public_key: [0xcd; 32],
            valid_from: 1_700_000_000,
            valid_until: 1_800_000_000,
        }
    }

    #[test]
    fn datum_round_trip() {
        let d = sample_datum();
        let bytes = encode_datum(&d).expect("encode");
        let back = decode_datum(&bytes).expect("decode");
        assert_eq!(d, back);
    }

    #[test]
    fn datum_decode_rejects_wrong_tag() {
        // Constr 1 instead of Constr 0.
        let bytes = b"\xd8\x7a\x84\x58\x1c";
        let _ = decode_datum(bytes);
        // we don't assert on specific err — just shouldn't decode as Constr 0
        // (tag 122 == d87a). Re-encode w/ tag 121 and confirm the diff.
        let mut buf = Vec::new();
        let mut enc = pallas_codec::minicbor::Encoder::new(&mut buf);
        enc.tag(Tag::new(122)).unwrap();
        enc.array(4).unwrap();
        enc.bytes(&[0u8; 28]).unwrap();
        enc.bytes(&[0u8; 32]).unwrap();
        enc.i64(0).unwrap();
        enc.i64(0).unwrap();
        let res = decode_datum(&buf);
        assert!(res.is_err(), "tag 122 must be rejected as not Constr 0");
    }

    #[test]
    fn datum_decode_rejects_wrong_stake_key_length() {
        let mut buf = Vec::new();
        let mut enc = pallas_codec::minicbor::Encoder::new(&mut buf);
        enc.tag(Tag::new(121)).unwrap();
        enc.array(4).unwrap();
        enc.bytes(&[0u8; 27]).unwrap(); // wrong: 27 bytes
        enc.bytes(&[0u8; 32]).unwrap();
        enc.i64(0).unwrap();
        enc.i64(0).unwrap();
        let res = decode_datum(&buf);
        assert!(res.is_err());
    }

    #[test]
    fn script_address_preprod_starts_with_addr_test1w() {
        let addr = script_address(Network::Preprod).expect("derive");
        assert!(addr.starts_with("addr_test1w"), "got {addr}");
    }

    #[test]
    fn script_address_mainnet_starts_with_addr1w() {
        let addr = script_address(Network::Mainnet).expect("derive");
        assert!(addr.starts_with("addr1w"), "got {addr}");
    }

    #[test]
    fn stake_key_hash_round_trip_preprod() {
        let key_hash = [0x33u8; 28];
        let bech =
            stake_address_from_key_hash(&key_hash, Network::Preprod).expect("encode stake addr");
        assert!(bech.starts_with("stake_test1"), "got {bech}");
        let back = stake_key_hash_from_bech32(&bech).expect("decode");
        assert_eq!(back, key_hash.to_vec());
    }

    #[test]
    fn stake_key_hash_round_trip_mainnet() {
        let key_hash = [0x77u8; 28];
        let bech =
            stake_address_from_key_hash(&key_hash, Network::Mainnet).expect("encode stake addr");
        assert!(bech.starts_with("stake1"), "got {bech}");
        let back = stake_key_hash_from_bech32(&bech).expect("decode");
        assert_eq!(back, key_hash.to_vec());
    }

    #[test]
    fn stake_key_hash_rejects_non_stake_address() {
        // A regular payment address bech32 — should not decode as stake.
        let payment = "addr_test1w_invalid";
        assert!(stake_key_hash_from_bech32(payment).is_none());
    }

    #[test]
    fn witness_check_returns_undecodable_for_garbage() {
        let res = tx_witnesses_include_stake_key(&[0xff, 0xff, 0xff], &[0u8; 28]);
        assert_eq!(res, WitnessCheck::Undecodable);
    }

    #[test]
    fn witness_check_returns_undecodable_for_empty() {
        let res = tx_witnesses_include_stake_key(&[], &[0u8; 28]);
        assert_eq!(res, WitnessCheck::Undecodable);
    }
}
