//// Stake-Address → Public-Key Registration Validator
////
//// Acts as a known deposit address for on-chain registrations binding
//// a Cardano stake address to the Ed25519 public key it will use to
//// sign privileged gossip envelopes (taxonomy, governance, Sentinel
//// priors, plugin DAO attestations).
////
//// The script is intentionally immutable — every spend purpose
//// fails. Registrations cannot be retracted or amended in place; to
//// supersede a binding the issuer publishes a new UTXO with a fresh
//// `valid_from` so the older window is shadowed by the newer one.
////
//// Security model (off-chain verification):
////
////   1. The verifier scans UTXOs at this script address.
////   2. For each UTXO it parses the inline `StakePubkeyRegistrationDatum`.
////   3. It then queries the creating transaction and asserts that
////      `tx.extra_signatories` contains `datum.stake_key_hash`.
////   4. Only verified entries land in `stake_pubkey_registry`.
////
//// Because the script always fails on spend, attackers cannot
//// "garbage-collect" legitimate entries by spending the UTXO; the
//// only way to remove information is to roll back the chain.
////
//// Mirrors `docs/stake-pubkey-registry.md` and the Rust types in
//// `src-tauri/src/p2p/registry.rs`.

use cardano/transaction.{OutputReference, Transaction}
use alexandria/types.{StakePubkeyRegistrationDatum}

validator stake_pubkey_registration {
  spend(
    _datum: Option<StakePubkeyRegistrationDatum>,
    _redeemer: Data,
    _own_ref: OutputReference,
    _tx: Transaction,
  ) {
    // Registrations are immutable. Reissue with a new validity window
    // instead of trying to spend the UTXO.
    fail @"stake_pubkey_registration: entries are immutable"
  }

  else(_ctx) {
    fail @"stake_pubkey_registration: unsupported purpose"
  }
}
