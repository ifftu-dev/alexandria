//// Alexandria Governance — State Token Helpers
////
//// State tokens are unique NFTs that identify the "current" UTxO for
//// stateful on-chain entities (DAO, election, proposal). Prevents
//// UTxO confusion attacks.

use aiken/collection/list
use cardano/address.{Script}
use cardano/assets.{AssetName, PolicyId, Value}
use cardano/transaction.{Input, Output}

/// Verify a minting tx creates exactly one token of the given policy+name.
pub fn mints_exactly_one(
  mint_value: Value,
  policy_id: PolicyId,
  asset_name: AssetName,
) -> Bool {
  assets.quantity_of(mint_value, policy_id, asset_name) == 1
}

/// Verify a tx burns exactly one token of the given policy+name.
pub fn burns_exactly_one(
  mint_value: Value,
  policy_id: PolicyId,
  asset_name: AssetName,
) -> Bool {
  assets.quantity_of(mint_value, policy_id, asset_name) == -1
}

/// Find the input holding the state token. Fails if not found.
pub fn find_state_input(
  inputs: List<Input>,
  policy_id: PolicyId,
  asset_name: AssetName,
) -> Input {
  when
    list.find(
      inputs,
      fn(input) { assets.has_nft(input.output.value, policy_id, asset_name) },
    )
  is {
    Some(input) -> input
    None -> fail @"state token input not found"
  }
}

/// Find the output holding the state token. Fails if not found.
pub fn find_state_output(
  outputs: List<Output>,
  policy_id: PolicyId,
  asset_name: AssetName,
) -> Output {
  when
    list.find(
      outputs,
      fn(output) { assets.has_nft(output.value, policy_id, asset_name) },
    )
  is {
    Some(output) -> output
    None -> fail @"state token output not found"
  }
}

/// Verify state token continuity: exactly 1 in inputs, exactly 1 in outputs.
pub fn state_token_preserved(
  inputs: List<Input>,
  outputs: List<Output>,
  policy_id: PolicyId,
  asset_name: AssetName,
) -> Bool {
  let in_count =
    list.count(
      inputs,
      fn(i) { assets.has_nft(i.output.value, policy_id, asset_name) },
    )
  let out_count =
    list.count(
      outputs,
      fn(o) { assets.has_nft(o.value, policy_id, asset_name) },
    )
  in_count == 1 && out_count == 1
}

/// Verify an output is sent to the expected script address.
pub fn output_to_script(output: Output, expected_hash: ByteArray) -> Bool {
  when output.address.payment_credential is {
    Script(hash) -> hash == expected_hash
    _ -> False
  }
}
