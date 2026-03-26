//// DAO State Token Minting Policy
////
//// Controls unique NFTs that identify each DAO's state UTxO.
//// One token per DAO, minted on creation, burned on dissolution.
////
//// Parameterized by:
////   authorized_admin — only this key can create/dissolve DAOs
////   dao_registry_hash — the DAO registry validator script hash

use aiken/collection/dict
use aiken/collection/list
use aiken/crypto.{ScriptHash, VerificationKeyHash}
use cardano/address.{Script}
use cardano/assets.{PolicyId}
use cardano/transaction.{Output, Transaction}

validator dao_minting(
  authorized_admin: VerificationKeyHash,
  dao_registry_hash: ScriptHash,
) {
  mint(_redeemer: Data, own_policy: PolicyId, tx: Transaction) {
    // Must be signed by authorized admin
    let admin_signed = list.has(tx.extra_signatories, authorized_admin)

    // Every minted token must be quantity +1 (create) or -1 (dissolve)
    let all_valid =
      assets.tokens(tx.mint, own_policy)
        |> dict.foldr(
            True,
            fn(token_name, qty, acc) {
              if !acc {
                False
              } else if qty == 1 {
                // Minting: must send the token to the DAO registry script
                token_sent_to_registry(
                  tx.outputs,
                  own_policy,
                  token_name,
                  dao_registry_hash,
                )
              } else {
                // Burning (-1) allowed, anything else rejected
                qty == -1
              }
            },
          )

    admin_signed && all_valid
  }

  else(_ctx) {
    fail @"unsupported purpose"
  }
}

/// Check that at least one output sends the token to the DAO registry script.
fn token_sent_to_registry(
  outputs: List<Output>,
  policy_id: PolicyId,
  token_name: ByteArray,
  registry_hash: ScriptHash,
) -> Bool {
  list.any(
    outputs,
    fn(o) {
      let at_registry =
        when o.address.payment_credential is {
          Script(h) -> h == registry_hash
          _ -> False
        }
      let has_token = assets.quantity_of(o.value, policy_id, token_name) == 1
      at_registry && has_token
    },
  )
}
