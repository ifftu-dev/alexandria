//// Alexandria Completion Witness — minting policy
////
//// Mints a single "proof-of-completion" token per (learner, course)
//// when the learner has finished every required element of a
//// registered course. The resulting tx hash is recorded by the
//// off-chain observer in the `witness` block of the W3C Verifiable
//// Credential it auto-issues to the learner.
////
//// Rules for `MintCompletion`:
////   1. Exactly one token is minted under this policy.
////   2. The asset name is 32 bytes: learner's pkh (28) ++ course tag (4).
////   3. The learner signs the tx (pkh in `tx.extra_signatories`).
////   4. An output carries that token AND an inline `CompletionDatum`
////      whose:
////        - `subject_pubkey` is non-empty (32 bytes expected by
////          convention — we defer exact-length checks to off-chain).
////        - `completion_root` equals `merkle_root(element_leaves)`
////          reconstructed from the redeemer.
////        - `timestamp` falls within the tx validity range.
////
//// Rules for `BurnCompletion`:
////   1. Every token under this policy in `tx.mint` has a negative
////      quantity (pure burn, no mint).
////   2. The learner signs the tx.

use aiken/collection/dict
use aiken/collection/list
use aiken/interval
use cardano/assets.{PolicyId}
use cardano/transaction.{InlineDatum, Output, Transaction}
use alexandria/completion.{
  BurnCompletion, CompletionDatum, CompletionRedeemer, MintCompletion,
  extract_learner_pkh, has_valid_length, merkle_root,
}

validator completion_minting {
  mint(redeemer: CompletionRedeemer, own_policy: PolicyId, tx: Transaction) {
    let minted = assets.tokens(tx.mint, own_policy)

    when redeemer is {
      MintCompletion { element_leaves } -> {
        // 1. Exactly one token minted under this policy.
        expect dict.size(minted) == 1

        // Destructure the single (asset_name, quantity) entry.
        let pairs = dict.to_pairs(minted)
        expect [Pair(asset_name, qty)] = pairs
        expect qty == 1

        // 2. Asset name is the expected length.
        expect has_valid_length(asset_name)
        let learner_pkh = extract_learner_pkh(asset_name)

        // 3. Learner signed the tx.
        expect list.has(tx.extra_signatories, learner_pkh)

        // 4. There is an output that carries this token + inline datum.
        //    The datum's completion_root must match the reconstructed
        //    Merkle root, and the timestamp must fall in the validity
        //    range.
        let expected_root = merkle_root(element_leaves)
        list.any(
          tx.outputs,
          fn(o) { output_carries_valid_completion(
            o,
            own_policy,
            asset_name,
            expected_root,
            tx.validity_range,
          ) },
        )
      }

      BurnCompletion -> {
        // All minted entries under this policy must be negative (burn),
        // and the learner must authorize the burn. We require at least
        // one signatory to match any of the burning asset names.
        let all_burning =
          dict.foldr(minted, True, fn(_name, qty, acc) { acc && qty < 0 })
        let any_learner_signed =
          dict.foldr(
            minted,
            False,
            fn(name, _qty, acc) {
              if acc {
                True
              } else if has_valid_length(name) {
                list.has(tx.extra_signatories, extract_learner_pkh(name))
              } else {
                False
              }
            },
          )
        all_burning && any_learner_signed
      }
    }
  }

  else(_ctx) {
    fail @"completion: unsupported purpose"
  }
}

fn output_carries_valid_completion(
  output: Output,
  policy: PolicyId,
  asset_name: ByteArray,
  expected_root: ByteArray,
  validity_range: interval.Interval,
) -> Bool {
  let has_token = assets.quantity_of(output.value, policy, asset_name) == 1
  if !has_token {
    False
  } else {
    when output.datum is {
      InlineDatum(raw) -> {
        expect datum: CompletionDatum = raw
        datum.completion_root == expected_root && interval.contains(
          validity_range,
          datum.timestamp,
        )
      }
      _ -> False
    }
  }
}
