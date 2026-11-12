//! Wallet outpoints this node's own in-flight transactions spend.
//!
//! A chain provider keeps listing a UTxO until the block that spends it is
//! indexed, so builders must step around what the submission journal says is
//! still in flight. [`super::submission::reserved_outpoints`] answers that
//! from the journal alone, with no network request.

use std::collections::BTreeSet;

use super::types::UTxO;

/// One transaction output, by the transaction that created it and its index.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Outpoint {
    /// Lowercase hex transaction hash.
    pub tx_hash: String,
    pub index: u64,
}

/// Inputs and collateral inputs of journaled transactions that may still land
/// or have just landed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReservedOutpoints {
    outpoints: BTreeSet<Outpoint>,
}

impl ReservedOutpoints {
    pub(crate) fn insert(&mut self, outpoint: Outpoint) {
        self.outpoints.insert(outpoint);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Outpoint> {
        self.outpoints.iter()
    }

    pub fn len(&self) -> usize {
        self.outpoints.len()
    }

    pub fn is_empty(&self) -> bool {
        self.outpoints.is_empty()
    }

    pub fn contains(&self, utxo: &UTxO) -> bool {
        // Hashes are compared as stored by the journal: lowercase hex.
        self.outpoints.contains(&Outpoint {
            tx_hash: utxo.tx_hash.to_ascii_lowercase(),
            index: utxo.tx_index,
        })
    }
}

/// Every UTxO the selection policy would take is spent by one of this node's
/// in-flight transactions. A wait, never a failed attempt: it clears when
/// those transactions land or expire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("wallet busy: inputs reserved by in-flight transactions")]
pub struct WalletBusy;

impl WalletBusy {
    /// Whether a builder's string error is this one.
    pub fn matches(error: &str) -> bool {
        error.starts_with(&WalletBusy.to_string())
    }
}

impl From<WalletBusy> for String {
    fn from(busy: WalletBusy) -> Self {
        busy.to_string()
    }
}

/// Apply a coin-selection policy to the UTxOs no in-flight transaction
/// spends. Every builder that spends the node wallet selects through this.
///
/// `Ok(None)` when the policy finds nothing even among reserved UTxOs (the
/// wallet is short of funds); [`WalletBusy`] when only reserved UTxOs would
/// have satisfied it.
pub fn select_unreserved<'a, F>(
    utxos: &'a [UTxO],
    reserved: &ReservedOutpoints,
    policy: F,
) -> Result<Option<&'a UTxO>, WalletBusy>
where
    F: Fn(&[UTxO]) -> Option<&UTxO>,
{
    let free: Vec<UTxO> = utxos
        .iter()
        .filter(|utxo| !reserved.contains(utxo))
        .cloned()
        .collect();
    if let Some(picked) = policy(&free) {
        log::debug!(
            "selected {}#{} ({} outpoints reserved by in-flight transactions)",
            picked.tx_hash,
            picked.tx_index,
            reserved.len()
        );
        return Ok(utxos
            .iter()
            .find(|utxo| utxo.tx_hash == picked.tx_hash && utxo.tx_index == picked.tx_index));
    }
    if policy(utxos).is_some() {
        log::info!(
            "wallet busy: {} outpoints reserved by in-flight transactions",
            reserved.len()
        );
        return Err(WalletBusy);
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cardano::fake_ledger::ada;

    fn reserving(utxos: &[&UTxO]) -> ReservedOutpoints {
        let mut reserved = ReservedOutpoints::default();
        for utxo in utxos {
            reserved.insert(outpoint(utxo));
        }
        reserved
    }

    fn outpoint(utxo: &UTxO) -> Outpoint {
        Outpoint {
            tx_hash: utxo.tx_hash.clone(),
            index: utxo.tx_index,
        }
    }

    fn picked(selected: Result<Option<&UTxO>, WalletBusy>) -> Result<Option<Outpoint>, WalletBusy> {
        selected.map(|utxo| utxo.map(outpoint))
    }

    fn largest(utxos: &[UTxO]) -> Option<&UTxO> {
        utxos
            .iter()
            .filter(|u| u.lovelace() >= 5_000_000)
            .max_by_key(|u| u.lovelace())
    }

    #[test]
    fn selection_applies_its_policy_to_unreserved_utxos_only() {
        let wallet = [
            ada(1, 0, 50_000_000),
            ada(2, 0, 20_000_000),
            ada(3, 0, 1_000_000),
        ];
        let none = ReservedOutpoints::default();
        assert_eq!(
            picked(select_unreserved(&wallet, &none, largest)),
            Ok(Some(outpoint(&wallet[0])))
        );
        let first_taken = reserving(&[&wallet[0]]);
        assert_eq!(
            picked(select_unreserved(&wallet, &first_taken, largest)),
            Ok(Some(outpoint(&wallet[1])))
        );
    }

    #[test]
    fn wallet_is_busy_only_when_reservations_hide_every_eligible_utxo() {
        let wallet = [
            ada(1, 0, 50_000_000),
            ada(2, 0, 20_000_000),
            ada(3, 0, 1_000_000),
        ];
        let eligible_taken = reserving(&[&wallet[0], &wallet[1]]);
        let busy = select_unreserved(&wallet, &eligible_taken, largest).unwrap_err();
        assert_eq!(
            busy.to_string(),
            "wallet busy: inputs reserved by in-flight transactions"
        );
        let message: String = busy.into();
        assert!(WalletBusy::matches(&message));
        assert!(!WalletBusy::matches("insufficient funds"));
        // Too little ADA is not a busy wallet, reserved or not.
        let poor = [ada(3, 0, 1_000_000)];
        assert_eq!(
            picked(select_unreserved(&poor, &reserving(&[&poor[0]]), largest)),
            Ok(None)
        );
    }
}
