use aiken/builtin
use aiken/collection/dict
use aiken/collection/list
use aiken/crypto.{ScriptHash, VerificationKeyHash}
use cardano/address.{Script}
use cardano/assets.{PolicyId}
use cardano/transaction.{InlineDatum, Output, Transaction}
use alexandria/types.{
  BurnReputation, MintReputation, ReputationDatum, ReputationMintRedeemer,
  cip68_reference_label, cip68_user_token_label,
}
use alexandria/reputation.{reputation_asset_name_length}

validator reputation_minting(
  authorized_minter: VerificationKeyHash,
  soulbound_script_hash: ScriptHash,
) {
  mint(
    redeemer: ReputationMintRedeemer,
    own_policy: PolicyId,
    tx: Transaction,
  ) {
    let minter_signed = list.has(tx.extra_signatories, authorized_minter)

    when redeemer is {
      MintReputation -> {
        let minted = assets.tokens(tx.mint, own_policy)

        // Must mint an even number of tokens (each pair = ref + user)
        let token_count = dict.size(minted)
        expect token_count > 0 && token_count % 2 == 0

        // Verify each token
        let all_pairs_valid =
          dict.foldr(
            minted,
            True,
            fn(name, qty, acc) {
              if !acc {
                False
              } else {
                let name_len = builtin.length_of_bytearray(name)
                if name_len != 4 + reputation_asset_name_length {
                  False
                } else {
                  let prefix = builtin.slice_bytearray(0, 4, name)
                  if prefix == cip68_reference_label {
                    if qty != 1 {
                      False
                    } else {
                      let base_name =
                        builtin.slice_bytearray(
                          4,
                          reputation_asset_name_length,
                          name,
                        )
                      ref_token_at_soulbound(
                        tx.outputs,
                        own_policy,
                        name,
                        base_name,
                        soulbound_script_hash,
                      )
                    }
                  } else if prefix == cip68_user_token_label {
                    qty == 1
                  } else {
                    False
                  }
                }
              }
            },
          )

        minter_signed && all_pairs_valid
      }

      BurnReputation -> {
        let all_burning =
          assets.tokens(tx.mint, own_policy)
            |> dict.foldr(True, fn(_name, qty, acc) { acc && qty < 0 })
        minter_signed && all_burning
      }
    }
  }

  else(_ctx) {
    fail @"unsupported purpose"
  }
}

fn ref_token_at_soulbound(
  outputs: List<Output>,
  policy_id: PolicyId,
  full_name: ByteArray,
  base_name: ByteArray,
  script_hash: ScriptHash,
) -> Bool {
  list.any(
    outputs,
    fn(o) {
      let at_script =
        when o.address.payment_credential is {
          Script(h) -> h == script_hash
          _ -> False
        }
      let has_token = assets.quantity_of(o.value, policy_id, full_name) == 1
      let has_datum =
        when o.datum is {
          InlineDatum(raw) -> {
            expect rep_datum: ReputationDatum = raw
            let datum_subject = builtin.slice_bytearray(0, 16, base_name)
            rep_datum.subject_id == datum_subject
          }
          _ -> False
        }
      at_script && has_token && has_datum
    },
  )
}
