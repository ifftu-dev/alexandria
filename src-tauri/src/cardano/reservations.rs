//! Wallet outpoints this node's own in-flight transactions spend.
//!
//! A chain provider keeps listing a UTxO until the block that spends it is
//! indexed, so builders must step around what the submission journal says is
//! still in flight. [`super::submission::reserved_outpoints`] answers that
//! from the journal alone, with no network request.

use std::collections::BTreeSet;

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
}
