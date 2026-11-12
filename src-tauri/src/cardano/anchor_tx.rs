//! Build the transaction that anchors one batch of credentials (protocol spec
//! §11.3.1). Distinct from the NFT mint path (§12.3, §19).
//!
//! The anchor record is label-1697 metadata, version 2:
//!
//! ```text
//! { 1697: { "root": <hex>, "leaf_count": <n>, "issuer_did": <did:key:z…>,
//!           "anchored_from": <ISO 8601>, "v": 2 } }
//! ```
//!
//! `root` is the Merkle root over the batch's integrity hashes
//! (`alexandria_verify::vc::anchor`). The issuer's payment key, which is also
//! its `did:key`, is a required signer however the fee is paid.
//!
//! Two ways to pay:
//!
//! - **Sponsor.** Spend one UTxO of the `anchor_sponsor` script (the
//!   Alexandria treasury) through its reference script. The script lets the
//!   fee out, up to its cap, and requires the rest back at the script with
//!   `next_allowed` pushed one window past this transaction's lower bound.
//!   The issuer's wallet only supplies collateral, which the ledger takes
//!   only if the script fails.
//! - **Issuer.** A metadata-only transaction from the issuer's wallet.
//!
//! The script never reads the metadata, so both kinds verify the same way.

use pallas_addresses::Address as PallasAddress;
use pallas_codec::utils::KeyValuePairs;
use pallas_crypto::hash::Hash;
use pallas_primitives::{Metadatum, MetadatumLabel};
use pallas_txbuilder::{BuildConway, ExUnits, Input, Output, ScriptKind, StagingTransaction};

use crate::cardano::cost_models::PLUTUS_V3_COST_MODEL;
use crate::cardano::ledger::{Ledger, ANCHOR_RECORD_VERSION};
use crate::cardano::plutus_data;
use crate::cardano::plutus_spend::{plutus_fee, EST_MEM, EST_STEPS, MIN_COLLATERAL_LOVELACE};
use crate::cardano::reservations::{select_unreserved, ReservedOutpoints, WalletBusy};
use crate::cardano::script_refs::ALEXANDRIA_ANCHOR_LABEL;
use crate::cardano::tx_builder::{self, MIN_UTXO_LOVELACE, TTL_OFFSET};
use crate::cardano::types::UTxO;
use crate::crypto::did::Did;
use crate::crypto::wallet::Wallet;
use crate::network_profile::AnchorSponsorProfile;

/// Marks an error the wallet's balance can fix. The UI offers the funding hint
/// for errors that carry it.
pub const NEEDS_FUNDS: &str = "needs funds";

/// Least lovelace a sponsor UTxO must keep after paying a fee, so the
/// continuing output (address, inline datum) stays above the ledger minimum.
pub const SPONSOR_MIN_CONTINUING_LOVELACE: u64 = 2_000_000;

/// The version-2 anchor record for one batch.
#[derive(Debug, Clone)]
pub struct AnchorRecord {
    pub root: String,
    pub leaf_count: u64,
    pub issuer_did: Did,
    /// Earliest issuance time in the batch.
    pub anchored_from: String,
}

/// Who pays the fee.
pub enum Funding<'a> {
    Sponsor {
        profile: &'a AnchorSponsorProfile,
        utxo: &'a UTxO,
    },
    Issuer,
}

impl Funding<'_> {
    pub fn label(&self) -> &'static str {
        match self {
            Funding::Sponsor { .. } => "sponsor",
            Funding::Issuer => "issuer",
        }
    }
}

/// Why an anchor transaction was not built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnchorBuildError {
    /// Every UTxO that would do is spent by this node's own in-flight
    /// transactions: a wait, which clears when they land or expire.
    #[error(transparent)]
    WalletBusy(#[from] WalletBusy),
    /// The wallet holds no UTxO that would do, reserved or not; funding it
    /// fixes this. Shown with the [`NEEDS_FUNDS`] marker.
    #[error("{marker}: {0}", marker = NEEDS_FUNDS)]
    NeedsFunds(String),
    #[error("{0}")]
    Other(String),
}

impl From<String> for AnchorBuildError {
    fn from(error: String) -> Self {
        Self::Other(error)
    }
}

/// A signed anchor transaction.
pub struct AnchorTx {
    pub signed_cbor: Vec<u8>,
    pub tx_hash: String,
}

/// Build and sign the anchor transaction for `record` at `tip_slot`. Wallet
/// inputs and collateral are chosen among UTxOs outside `reserved`; when only
/// reserved ones would do, the error is [`AnchorBuildError::WalletBusy`].
pub async fn build_anchor_tx<L: Ledger>(
    record: &AnchorRecord,
    funding: Funding<'_>,
    wallet: &Wallet,
    ledger: &L,
    tip_slot: u64,
    reserved: &ReservedOutpoints,
) -> Result<AnchorTx, AnchorBuildError> {
    let (utxos, params) = tokio::join!(
        ledger.utxos(&wallet.payment_address),
        ledger.protocol_params(),
    );
    let (wallet_utxos, params) = (utxos?, params?);
    let metadata = build_anchor_metadata(record);
    let payment_key = tx_builder::extended_private_key(&wallet.payment_key_extended)
        .map_err(|e| format!("payment key: {e}"))?;
    // The issuer's `did:key` key is the required signer, so a verifier can
    // tie the anchor to the DID with nothing but the DID. It is not the
    // wallet's payment key, which only witnesses the inputs it pays from.
    let did_key = pallas_wallet::PrivateKey::Normal(pallas_crypto::key::ed25519::SecretKey::from(
        wallet.signing_key.to_bytes(),
    ));
    let issuer_hash = did_key_hash(wallet);

    let unsigned = match funding {
        Funding::Issuer => {
            let fee = tx_builder::estimate_fee(&params, 1);
            let needed = fee + MIN_UTXO_LOVELACE;
            let selected =
                select_unreserved(&wallet_utxos, reserved, |u| select_ada_only(u, needed))?
                    .ok_or_else(|| {
                        AnchorBuildError::NeedsFunds(format!(
                            "no sponsor available, and the wallet has no ADA-only UTxO holding {needed} lovelace"
                        ))
                    })?;
            let address = PallasAddress::from_bech32(&wallet.payment_address)
                .map_err(|e| format!("bad wallet address: {e}"))?;
            let built = StagingTransaction::new()
                .input(Input::new(
                    parse_hash(&selected.tx_hash)?,
                    selected.tx_index,
                ))
                .output(Output::new(address, selected.lovelace() - fee))
                .disclosed_signer(issuer_hash)
                .fee(fee)
                .invalid_from_slot(tip_slot + TTL_OFFSET)
                .network_id(0)
                .build_conway_raw()
                .map_err(|e| format!("build: {e}"))?;
            let (with_metadata, _) = tx_builder::inject_metadata(&built.tx_bytes.0, metadata)
                .map_err(|e| format!("inject_metadata: {e}"))?;
            with_metadata
        }
        Funding::Sponsor { profile, utxo } => {
            let plan = SponsorPlan::new(profile, utxo, tip_slot)?;
            let collateral = sponsor_collateral(&wallet_utxos, reserved)?.clone();
            // Execution units as `(mem, steps)`; `ExUnits` is not `Copy`.
            let build = |fee: u64, (mem, steps): (u64, u64)| -> Result<Vec<u8>, String> {
                plan.build(
                    fee,
                    ExUnits { mem, steps },
                    &collateral,
                    issuer_hash,
                    metadata.clone(),
                )
            };
            let estimate = |(mem, steps): (u64, u64), size: u64| {
                plutus_fee(params.min_fee_a, params.min_fee_b, size, mem, steps)
            };
            let est_units = (EST_MEM, EST_STEPS);
            let draft = build(
                estimate(est_units, 1_500).min(profile.fee_cap_lovelace),
                est_units,
            )?;
            let units = match ledger.evaluate(&draft).await {
                Ok(units) if !units.is_empty() => units
                    .into_iter()
                    .fold((0, 0), |(m, s), (um, us)| (m.max(um), s.max(us))),
                Ok(_) => est_units,
                Err(e) => {
                    return Err(AnchorBuildError::Other(format!(
                        "sponsor script evaluation failed: {e}"
                    )))
                }
            };
            let fee = estimate(units, draft.len() as u64 + 200);
            if fee > profile.fee_cap_lovelace {
                return Err(AnchorBuildError::Other(format!(
                    "anchor fee {fee} lovelace exceeds the sponsor's cap of {}",
                    profile.fee_cap_lovelace
                )));
            }
            build(fee, units)?
        }
    };

    let signed_cbor = tx_builder::sign_raw_tx_many(&unsigned, &[&payment_key, &did_key])
        .map_err(|e| format!("sign: {e}"))?;
    let tx_hash = tx_builder::compute_tx_hash(&signed_cbor).map_err(|e| format!("hash: {e}"))?;
    Ok(AnchorTx {
        signed_cbor,
        tx_hash,
    })
}

/// The collateral a sponsored anchor pledges from the issuer's wallet: the
/// smallest sufficient ADA-only UTxO outside `reserved`, since collateral is
/// forfeited whole if the script fails on chain and no collateral return is
/// set.
pub fn sponsor_collateral<'a>(
    wallet_utxos: &'a [UTxO],
    reserved: &ReservedOutpoints,
) -> Result<&'a UTxO, AnchorBuildError> {
    select_unreserved(wallet_utxos, reserved, |u| {
        smallest_ada_only(u, MIN_COLLATERAL_LOVELACE)
    })?
    .ok_or_else(|| {
        AnchorBuildError::NeedsFunds(format!(
            "a sponsored anchor needs collateral, an ADA-only UTxO of at least {MIN_COLLATERAL_LOVELACE} lovelace in the wallet"
        ))
    })
}

/// The shape of one sponsored spend, fixed before fees are known.
struct SponsorPlan<'a> {
    profile: &'a AnchorSponsorProfile,
    utxo: &'a UTxO,
    valid_from_slot: u64,
    invalid_from_slot: u64,
    next_allowed_ms: i64,
}

impl<'a> SponsorPlan<'a> {
    fn new(
        profile: &'a AnchorSponsorProfile,
        utxo: &'a UTxO,
        tip_slot: u64,
    ) -> Result<Self, String> {
        if !sponsor_utxo_is_spendable(profile, utxo, tip_slot) {
            return Err("the chosen sponsor UTxO is not spendable now".into());
        }
        // Lower bound at the tip: the script needs it at or after the
        // datum's `next_allowed`, and the ledger needs it in the past. The
        // interval is as wide as the script allows, in whole slots.
        let valid_from_slot = tip_slot;
        let invalid_from_slot = valid_from_slot + profile.max_validity_ms / 1000;
        let next_allowed_ms =
            i64::try_from(profile.slot_to_posix_ms(valid_from_slot) + profile.window_ms)
                .map_err(|_| "sponsor window out of range")?;
        Ok(Self {
            profile,
            utxo,
            valid_from_slot,
            invalid_from_slot,
            next_allowed_ms,
        })
    }

    fn build(
        &self,
        fee: u64,
        units: ExUnits,
        collateral: &UTxO,
        issuer: Hash<28>,
        metadata: KeyValuePairs<MetadatumLabel, Metadatum>,
    ) -> Result<Vec<u8>, String> {
        let (ref_hash, ref_index) = self
            .profile
            .reference()
            .ok_or("anchor sponsor reference input is malformed")?;
        let script_address = PallasAddress::from_bech32(&self.profile.address)
            .map_err(|e| format!("bad sponsor address: {e}"))?;
        let script_input = Input::new(parse_hash(&self.utxo.tx_hash)?, self.utxo.tx_index);
        let continuing = self.utxo.lovelace() - fee;
        let datum = plutus_data::encode_sponsor_datum(self.next_allowed_ms)
            .map_err(|e| format!("sponsor datum: {e}"))?;
        let redeemer =
            plutus_data::encode_sponsor_redeemer().map_err(|e| format!("redeemer: {e}"))?;
        let built = StagingTransaction::new()
            .input(script_input.clone())
            .add_spend_redeemer(script_input, redeemer, Some(units))
            .reference_input(Input::new(parse_hash(ref_hash)?, ref_index))
            .output(Output::new(script_address, continuing).set_inline_datum(datum))
            .collateral_input(Input::new(
                parse_hash(&collateral.tx_hash)?,
                collateral.tx_index,
            ))
            .language_view(ScriptKind::PlutusV3, PLUTUS_V3_COST_MODEL.to_vec())
            .disclosed_signer(issuer)
            .fee(fee)
            .valid_from_slot(self.valid_from_slot)
            .invalid_from_slot(self.invalid_from_slot)
            .network_id(0)
            .build_conway_raw()
            .map_err(|e| format!("build: {e}"))?;
        let (with_metadata, _) = tx_builder::inject_metadata(&built.tx_bytes.0, metadata)
            .map_err(|e| format!("inject_metadata: {e}"))?;
        Ok(with_metadata)
    }
}

/// Whether the sponsor script would let this UTxO pay a fee at `tip_slot`:
/// a plain lovelace UTxO (no reference script, no tokens) whose datum's
/// `next_allowed` has passed, holding enough for the fee cap and a valid
/// continuing output.
pub fn sponsor_utxo_is_spendable(
    profile: &AnchorSponsorProfile,
    utxo: &UTxO,
    tip_slot: u64,
) -> bool {
    let Some(next_allowed) = utxo
        .inline_datum
        .as_deref()
        .and_then(|datum| hex::decode(datum).ok())
        .and_then(|bytes| plutus_data::decode_sponsor_datum(&bytes))
    else {
        return false;
    };
    !utxo.has_reference_script()
        && utxo.amount.iter().all(|a| a.unit == "lovelace")
        && utxo.lovelace() >= profile.fee_cap_lovelace + SPONSOR_MIN_CONTINUING_LOVELACE
        && i128::from(next_allowed) <= i128::from(profile.slot_to_posix_ms(tip_slot))
}

/// Pick a spendable sponsor UTxO. Nodes choose by a hash of their issuer and
/// the tip, so concurrent nodes spread across the pool instead of racing for
/// the first entry. UTxOs this node's own in-flight transactions spend
/// (`reserved`) are left out before the pick.
///
/// `Ok(None)` when no sponsor UTxO is spendable at all (the caller falls back
/// to the issuer's wallet); [`WalletBusy`] when only this node's own
/// in-flight anchors keep the spendable ones (the caller waits for them).
pub fn select_sponsor_utxo<'a>(
    profile: &AnchorSponsorProfile,
    utxos: &'a [UTxO],
    tip_slot: u64,
    issuer: &Did,
    reserved: &ReservedOutpoints,
) -> Result<Option<&'a UTxO>, WalletBusy> {
    let spendable: Vec<&UTxO> = utxos
        .iter()
        .filter(|u| sponsor_utxo_is_spendable(profile, u, tip_slot))
        .collect();
    if spendable.is_empty() {
        return Ok(None);
    }
    let mut eligible: Vec<&UTxO> = spendable
        .into_iter()
        .filter(|u| !reserved.contains(u))
        .collect();
    if eligible.is_empty() {
        return Err(WalletBusy);
    }
    eligible.sort_by(|a, b| (&a.tx_hash, a.tx_index).cmp(&(&b.tx_hash, b.tx_index)));
    let mut seed = blake3::Hasher::new();
    seed.update(issuer.as_str().as_bytes());
    seed.update(&tip_slot.to_be_bytes());
    let digest = seed.finalize();
    let mut pick = [0; 8];
    pick.copy_from_slice(&digest.as_bytes()[..8]);
    let index = u64::from_be_bytes(pick) % eligible.len() as u64;
    Ok(eligible.get(index as usize).copied())
}

/// The smallest ADA-only UTxO holding at least `needed` lovelace.
fn smallest_ada_only(utxos: &[UTxO], needed: u64) -> Option<&UTxO> {
    utxos
        .iter()
        .filter(|u| {
            !u.has_reference_script()
                && u.amount.iter().all(|a| a.unit == "lovelace")
                && u.lovelace() >= needed
        })
        .min_by_key(|u| u.lovelace())
}

/// Blake2b-224 of the wallet's `did:key` public key: the anchor's required
/// signer (`alexandria_verify::vc::anchor::issuer_key_hash` of its DID).
pub fn did_key_hash(wallet: &Wallet) -> Hash<28> {
    pallas_crypto::hash::Hasher::<224>::hash(wallet.signing_key.verifying_key().as_bytes())
}

/// The largest ADA-only UTxO holding at least `needed` lovelace.
///
/// Spending a UTxO that carries other assets would strand them, and
/// reference-script UTxOs must never be spent.
fn select_ada_only(utxos: &[UTxO], needed: u64) -> Option<&UTxO> {
    utxos
        .iter()
        .filter(|u| {
            !u.has_reference_script()
                && u.amount.iter().all(|a| a.unit == "lovelace")
                && u.lovelace() >= needed
        })
        .max_by_key(|u| u.lovelace())
}

fn parse_hash(hex_str: &str) -> Result<Hash<32>, String> {
    tx_builder::parse_tx_hash(hex_str).map_err(|e| format!("parse tx hash: {e}"))
}

/// The `{ 1697: { … } }` auxiliary data for `record`.
pub fn build_anchor_metadata(record: &AnchorRecord) -> KeyValuePairs<MetadatumLabel, Metadatum> {
    let text = |k: &str, v: &str| (Metadatum::Text(k.into()), Metadatum::Text(v.into()));
    let inner = Metadatum::Map(KeyValuePairs::from(vec![
        text("root", &record.root),
        (
            Metadatum::Text("leaf_count".into()),
            Metadatum::Int(i64::try_from(record.leaf_count).unwrap_or(i64::MAX).into()),
        ),
        text("issuer_did", record.issuer_did.as_str()),
        text("anchored_from", &record.anchored_from),
        (
            Metadatum::Text("v".into()),
            Metadatum::Int(ANCHOR_RECORD_VERSION.into()),
        ),
    ]));
    KeyValuePairs::from(vec![(ALEXANDRIA_ANCHOR_LABEL, inner)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cardano::types::AmountEntry;

    pub(crate) fn utxo(index: u64, lovelace: u64, token: bool, script: bool) -> UTxO {
        let mut amount = vec![AmountEntry {
            unit: "lovelace".into(),
            quantity: lovelace.to_string(),
        }];
        if token {
            amount.push(AmountEntry {
                unit: "ab".repeat(28),
                quantity: "1".into(),
            });
        }
        UTxO {
            tx_hash: "00".repeat(32),
            tx_index: index,
            amount,
            reference_script_hash: script.then(|| "cd".repeat(28)),
            inline_datum: None,
        }
    }

    #[test]
    fn issuer_funding_spends_the_largest_ada_only_utxo_and_never_a_script_or_token() {
        let utxos = [
            utxo(0, 90_000_000, false, true),
            utxo(1, 80_000_000, true, false),
            utxo(2, 6_000_000, false, false),
            utxo(3, 20_000_000, false, false),
        ];
        assert_eq!(select_ada_only(&utxos, 5_300_000).unwrap().tx_index, 3);
        assert!(select_ada_only(&utxos, 30_000_000).is_none());
        assert!(select_ada_only(&utxos[..2], 1).is_none());
    }

    #[test]
    fn collateral_risks_the_smallest_sufficient_utxo() {
        let utxos = [
            utxo(0, 500_000_000, false, false),
            utxo(1, 4_000_000, false, false),
            utxo(2, 6_000_000, false, false),
            utxo(3, 9_000_000, true, false),
        ];
        assert_eq!(smallest_ada_only(&utxos, 5_000_000).unwrap().tx_index, 2);
    }

    /// Live preprod anchor of a one-credential batch. Ignored by default.
    /// The wallet needs a little tADA (fee, or collateral when sponsored).
    /// With `ALEXANDRIA_ANCHOR_SPONSOR` set to an `anchor_sponsor` profile
    /// object (JSON), a spendable sponsor UTxO pays instead of the wallet:
    ///   BLOCKFROST_PROJECT_ID=preprod… ALEXANDRIA_TEST_MNEMONIC="…" \
    ///   [ALEXANDRIA_ANCHOR_SPONSOR='{…}'] cargo test -p alexandria-node \
    ///     live_anchor_preprod -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_anchor_preprod() {
        let pid = std::env::var("BLOCKFROST_PROJECT_ID").expect("BLOCKFROST_PROJECT_ID");
        let mnemonic = std::env::var("ALEXANDRIA_TEST_MNEMONIC").expect("ALEXANDRIA_TEST_MNEMONIC");
        let sponsor: Option<AnchorSponsorProfile> = std::env::var("ALEXANDRIA_ANCHOR_SPONSOR")
            .ok()
            .map(|json| serde_json::from_str(&json).expect("anchor sponsor JSON"));
        let wallet = crate::crypto::wallet::wallet_from_mnemonic(&mnemonic).unwrap();
        let issued_at = crate::clock::now_rfc3339();
        let leaf = *blake3::hash(issued_at.as_bytes()).as_bytes();
        let record = AnchorRecord {
            root: hex::encode(alexandria_verify::vc::anchor::merkle_root(&[leaf])),
            leaf_count: 1,
            issuer_did: crate::crypto::did::derive_did_key(&wallet.signing_key),
            anchored_from: issued_at,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let outcome = rt.block_on(async {
            let bf = crate::cardano::blockfrost::BlockfrostClient::new(pid).unwrap();
            let tip = bf.tip_slot().await?;
            let funding = match &sponsor {
                Some(profile) => {
                    let utxos = bf.utxos(&profile.address).await?;
                    let utxo = select_sponsor_utxo(
                        profile,
                        &utxos,
                        tip,
                        &record.issuer_did,
                        &ReservedOutpoints::default(),
                    )
                    .map_err(|e| e.to_string())?
                    .ok_or("no spendable sponsor UTxO")?
                    .clone();
                    Some((profile, utxo))
                }
                None => None,
            };
            let built = match &funding {
                Some((profile, utxo)) => build_anchor_tx(
                    &record,
                    Funding::Sponsor { profile, utxo },
                    &wallet,
                    &bf,
                    tip,
                    &ReservedOutpoints::default(),
                )
                .await
                .map_err(|e| e.to_string())?,
                None => build_anchor_tx(
                    &record,
                    Funding::Issuer,
                    &wallet,
                    &bf,
                    tip,
                    &ReservedOutpoints::default(),
                )
                .await
                .map_err(|e| e.to_string())?,
            };
            bf.submit(&built.signed_cbor).await
        });
        println!(
            "LIVE_ANCHOR_RESULT: {outcome:?} root={} issuer={}",
            record.root,
            record.issuer_did.as_str()
        );
        outcome.expect("anchor accepted on preprod");
    }
}
