//// Challenge Stake Escrow
////
//// A credential challenger locks their stake (default 5 ADA) at this
//// script when opening a challenge. Settlement is authorised by the
//// DAO committee key recorded in the datum:
////
////   * `Refund`  — the challenge was upheld; the stake returns to the
////                 challenger. The spending tx MUST pay the challenger.
////   * `Forfeit` — the challenge was rejected; the stake is slashed to
////                 the DAO treasury. The spending tx MUST pay the
////                 treasury.
////
//// Because credential challenges are adjudicated off-chain (DAO
//// committee vote, gossiped), the validator cannot read the outcome
//// itself. It instead trusts the `dao_authority` key to settle in line
//// with the recorded vote, while still binding each redeemer to the
//// correct destination so the authority cannot redirect funds.

use aiken/collection/list
use aiken/crypto.{VerificationKeyHash}
use cardano/address.{VerificationKey}
use cardano/assets
use cardano/transaction.{Transaction, OutputReference}

/// Inline datum locked alongside the staked ADA.
pub type ChallengeEscrowDatum {
  /// Challenger's payment key hash — recipient on `Refund`.
  challenger: VerificationKeyHash,
  /// DAO treasury payment key hash — recipient on `Forfeit`.
  treasury: VerificationKeyHash,
  /// DAO committee key authorised to settle this escrow.
  dao_authority: VerificationKeyHash,
  /// Hash of the challenge id this stake backs (binds the UTxO to one
  /// challenge so it can't be replayed against another).
  challenge_id: ByteArray,
}

/// Settlement direction.
pub type EscrowRedeemer {
  Refund
  Forfeit
}

/// True iff at least one output pays a verification-key address whose
/// payment key hash equals `pkh` AND carries at least `lovelace`.
///
/// The amount check is the whole point. Without it, any output to the
/// right address satisfied the validator — one lovelace did — so the
/// authorised signer could settle a Refund by paying the challenger dust
/// and routing the rest of the stake to themselves, and the ledger would
/// accept it. The validator exists so that the operator does not have to
/// be trusted; checking only the address left them fully trusted.
fn pays_to(tx: Transaction, pkh: VerificationKeyHash, lovelace: Int) -> Bool {
  list.any(
    tx.outputs,
    fn(o) {
      when o.address.payment_credential is {
        VerificationKey(h) -> h == pkh && assets.lovelace_of(o.value) >= lovelace
        _ -> False
      }
    },
  )
}

/// The lovelace locked in the escrow input this script is spending.
///
/// Read from the input itself rather than the datum, so the amount the
/// recipient must receive is the amount that was actually locked — a datum
/// field could be written with any number at lock time.
fn locked_lovelace(tx: Transaction, own_ref: OutputReference) -> Int {
  expect Some(input) =
    list.find(tx.inputs, fn(i) { i.output_reference == own_ref })
  assets.lovelace_of(input.output.value)
}

validator challenge_escrow {
  spend(
    datum: Option<ChallengeEscrowDatum>,
    redeemer: EscrowRedeemer,
    own_ref: OutputReference,
    tx: Transaction,
  ) {
    expect Some(d) = datum

    // Every settlement must be authorised by the DAO committee key.
    let authorized = list.has(tx.extra_signatories, d.dao_authority)
    // ...and must pay the recipient the full stake, not merely *something*.
    let stake = locked_lovelace(tx, own_ref)

    when redeemer is {
      // Upheld → stake returns to the challenger.
      Refund -> authorized && pays_to(tx, d.challenger, stake)
      // Rejected → stake is slashed to the DAO treasury.
      Forfeit -> authorized && pays_to(tx, d.treasury, stake)
    }
  }

  else(_ctx) {
    fail @"unsupported purpose"
  }
}
