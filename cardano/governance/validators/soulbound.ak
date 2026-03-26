//// Soulbound Spending Script
////
//// Locks CIP-68 reference NFTs (100) containing reputation data.
//// Enforces non-transferability: tokens can only be spent if the output
//// returns to the SAME script address with the SAME owner in the datum.
////
//// Parameterized by:
////   authorized_minter — the platform's signing key hash that can update/revoke

use aiken/collection/list
use aiken/crypto.{VerificationKeyHash}
use cardano/address.{Script}
use cardano/transaction.{InlineDatum, OutputReference, Transaction}
use alexandria/types.{ReputationDatum, RevokeReputation, SoulboundRedeemer, UpdateReputation}

validator soulbound(authorized_minter: VerificationKeyHash) {
  spend(
    datum: Option<ReputationDatum>,
    redeemer: SoulboundRedeemer,
    own_ref: OutputReference,
    tx: Transaction,
  ) {
    expect Some(old_datum) = datum

    when redeemer is {
      UpdateReputation -> {
        // Must be signed by authorized_minter OR the owner
        let authorized =
          list.has(tx.extra_signatories, authorized_minter) || list.has(
            tx.extra_signatories,
            old_datum.owner,
          )

        // Find our own input to get the script hash
        expect Some(own_input) =
          list.find(tx.inputs, fn(i) { i.output_reference == own_ref })
        expect Script(own_hash) = own_input.output.address.payment_credential

        // Find the continuing output at the same script address
        let continuing_outputs =
          list.filter(
            tx.outputs,
            fn(o) {
              when o.address.payment_credential is {
                Script(h) -> h == own_hash
                _ -> False
              }
            },
          )

        // Must have exactly one continuing output
        expect [continuing_output] = continuing_outputs

        // Parse the new datum
        expect InlineDatum(new_raw) = continuing_output.datum
        expect new_datum: ReputationDatum = new_raw

        // Owner must not change (soulbound)
        let owner_preserved = new_datum.owner == old_datum.owner

        // Subject and role must not change (these define the token identity)
        let identity_preserved =
          new_datum.subject_id == old_datum.subject_id && new_datum.role == old_datum.role

        authorized && owner_preserved && identity_preserved
      }

      RevokeReputation -> {
        // Only the authorized minter can revoke
        list.has(tx.extra_signatories, authorized_minter)
        // The minting policy handles burning the tokens
      }
    }
  }

  else(_ctx) {
    fail @"unsupported purpose"
  }
}
