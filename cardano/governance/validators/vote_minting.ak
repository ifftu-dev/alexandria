//// Vote Receipt Minting Policy
////
//// Mints unique receipt tokens when votes are cast, preventing double-voting.
//// Asset name = voter_key_hash (28 bytes) ++ target_type (1 byte) ++ target_id (4 bytes)
//// target_type: 0 = election, 1 = proposal
////
//// Parameterized by:
////   election_script_hash — the election validator
////   proposal_script_hash — the proposal validator

use aiken/builtin
use aiken/collection/dict
use aiken/collection/list
use aiken/crypto.{ScriptHash}
use cardano/address.{Script}
use cardano/assets.{PolicyId}
use cardano/transaction.{Input, Transaction}
use alexandria/types.{BurnReceipt, MintReceipt, VoteReceiptRedeemer}

/// Vote receipt asset name length: 28 (vkh) + 1 (type) + 4 (id) = 33 bytes
pub const vote_receipt_name_length = 33

validator vote_minting(
  election_script_hash: ScriptHash,
  proposal_script_hash: ScriptHash,
) {
  mint(
    redeemer: VoteReceiptRedeemer,
    own_policy: PolicyId,
    tx: Transaction,
  ) {
    let minted = assets.tokens(tx.mint, own_policy)

    when redeemer is {
      MintReceipt -> {
        // Each minted receipt must:
        // 1. Have quantity exactly 1
        // 2. Have valid asset name format
        // 3. The voter (embedded in asset name) must have signed
        // 4. A corresponding election or proposal script must be executing
        dict.foldr(
          minted,
          True,
          fn(name, qty, acc) {
            if !acc {
              False
            } else if qty != 1 {
              False
            } else if builtin.length_of_bytearray(name) != vote_receipt_name_length {
              False
            } else {
              let voter_hash = builtin.slice_bytearray(0, 28, name)
              let target_type = builtin.index_bytearray(name, 28)

              // Voter must have signed
              let voter_signed = list.has(tx.extra_signatories, voter_hash)

              // The appropriate governance script must be executing
              let script_executing =
                if target_type == 0 {
                  // Election vote — election script must have an input
                  has_script_input(tx.inputs, election_script_hash)
                } else if target_type == 1 {
                  // Proposal vote — proposal script must have an input
                  has_script_input(tx.inputs, proposal_script_hash)
                } else {
                  False
                }

              voter_signed && script_executing
            }
          },
        )
      }

      BurnReceipt -> {
        // All tokens must be burned (negative quantity)
        dict.foldr(minted, True, fn(_name, qty, acc) { acc && qty < 0 })
      }
    }
  }

  else(_ctx) {
    fail @"unsupported purpose"
  }
}

/// Check if any input comes from the given script hash.
fn has_script_input(
  inputs: List<Input>,
  script_hash: ScriptHash,
) -> Bool {
  list.any(
    inputs,
    fn(i) {
      when i.output.address.payment_credential is {
        Script(h) -> h == script_hash
        _ -> False
      }
    },
  )
}
