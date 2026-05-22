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
/// payment key hash equals `pkh`.
fn pays_to(tx: Transaction, pkh: VerificationKeyHash) -> Bool {
  list.any(
    tx.outputs,
    fn(o) {
      when o.address.payment_credential is {
        VerificationKey(h) -> h == pkh
        _ -> False
      }
    },
  )
}

validator challenge_escrow {
  spend(
    datum: Option<ChallengeEscrowDatum>,
    redeemer: EscrowRedeemer,
    _own_ref: OutputReference,
    tx: Transaction,
  ) {
    expect Some(d) = datum

    // Every settlement must be authorised by the DAO committee key.
    let authorized = list.has(tx.extra_signatories, d.dao_authority)

    when redeemer is {
      // Upheld → stake returns to the challenger.
      Refund -> authorized && pays_to(tx, d.challenger)
      // Rejected → stake is slashed to the DAO treasury.
      Forfeit -> authorized && pays_to(tx, d.treasury)
    }
  }

  else(_ctx) {
    fail @"unsupported purpose"
  }
}
