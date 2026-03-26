//// Alexandria Governance — Reputation Token Helpers
////
//// Functions for verifying reputation token ownership and computing
//// voting weight from CIP-68 reference NFT datums.
////
//// Governance validators use these to check DAO membership and
//// determine how much voting power a participant has.

use aiken/builtin
use aiken/collection/dict
use aiken/collection/list
use aiken/crypto.{VerificationKeyHash}
use cardano/assets.{AssetName, PolicyId}
use cardano/transaction.{InlineDatum, Input}
use alexandria/types.{
  ProficiencyLevel, ReputationDatum, ReputationRole, SkillScore,
  cip68_reference_label, proficiency_met, proficiency_to_int,
}

/// Expected length of a reputation asset name: 16-byte subject UUID + 1-byte role
pub const reputation_asset_name_length = 17

/// Build the CIP-68 reference NFT asset name from a base asset name.
/// Prepends the (100) label prefix.
pub fn reference_asset_name(base_name: AssetName) -> AssetName {
  builtin.append_bytearray(cip68_reference_label, base_name)
}

/// Extract the subject_id (first 16 bytes) from a reputation asset name.
pub fn extract_subject_id(base_name: AssetName) -> ByteArray {
  builtin.slice_bytearray(0, 16, base_name)
}

/// Extract the role byte (last byte) from a reputation asset name.
pub fn extract_role_byte(base_name: AssetName) -> Int {
  builtin.index_bytearray(base_name, 16)
}

/// Parse a ReputationDatum from a reference input's inline datum.
/// The reference input must hold a (100)-prefixed token at the reputation policy.
/// Returns None if no matching reference input found.
pub fn find_reputation_datum(
  reference_inputs: List<Input>,
  reputation_policy: PolicyId,
  base_asset_name: AssetName,
) -> Option<ReputationDatum> {
  let ref_name = reference_asset_name(base_asset_name)
  list.find_map(
    reference_inputs,
    fn(input) {
      if assets.quantity_of(input.output.value, reputation_policy, ref_name) == 1 {
        when input.output.datum is {
          InlineDatum(raw_data) -> {
            expect datum: ReputationDatum = raw_data
            Some(datum)
          }
          _ -> None
        }
      } else {
        None
      }
    },
  )
}

/// Check if any reference input contains a reputation token for the given
/// policy, with a subject_id in the allowed list and at least the minimum
/// proficiency across any skill in the datum.
pub fn has_qualifying_reputation(
  reference_inputs: List<Input>,
  reputation_policy: PolicyId,
  membership_subjects: List<ByteArray>,
  min_proficiency: ProficiencyLevel,
  owner: VerificationKeyHash,
) -> Bool {
  // Look through all reference inputs for reputation tokens
  list.any(
    reference_inputs,
    fn(input) {
      // Get all tokens under the reputation policy
      let policy_tokens = assets.tokens(input.output.value, reputation_policy)

      // Check if any token matches our criteria
      dict.foldr(
        policy_tokens,
        False,
        fn(asset_name, quantity, acc) {
          if acc {
            True
          } else if quantity != 1 {
            False
          } else {
            // Must be a (100) reference NFT
            let is_ref =
              builtin.length_of_bytearray(asset_name) == 4 + reputation_asset_name_length && builtin.slice_bytearray(
                0,
                4,
                asset_name,
              ) == cip68_reference_label
            if !is_ref {
              False
            } else {
              // Extract the base name (after 4-byte label)
              let base = builtin.slice_bytearray(4, reputation_asset_name_length, asset_name)
              let subject_id = extract_subject_id(base)
              let subject_in_scope = list.has(membership_subjects, subject_id)
              if !subject_in_scope {
                False
              } else {
                // Parse datum to check owner and proficiency
                when input.output.datum is {
                  InlineDatum(raw_data) -> {
                    expect datum: ReputationDatum = raw_data
                    datum.owner == owner && has_qualifying_skill(
                      datum.skills,
                      min_proficiency,
                    )
                  }
                  _ -> False
                }
              }
            }
          }
        },
      )
    },
  )
}

/// Check if any skill in the list meets the minimum proficiency.
pub fn has_qualifying_skill(
  skills: List<SkillScore>,
  min_proficiency: ProficiencyLevel,
) -> Bool {
  list.any(skills, fn(s) { proficiency_met(s.proficiency, min_proficiency) })
}

/// Compute the total voting weight from a reputation datum's skill scores.
/// Weight = sum of (impact_score * proficiency_multiplier) for each skill.
/// Higher proficiency levels give more weight per unit of impact.
pub fn compute_voting_weight(datum: ReputationDatum) -> Int {
  list.foldr(
    datum.skills,
    0,
    fn(skill, acc) {
      let multiplier = proficiency_to_int(skill.proficiency) + 1
      acc + skill.impact_score * multiplier
    },
  )
}

/// Verify that a signer owns a qualifying reputation token.
/// Checks: (1) signer is in extra_signatories, (2) a reference input
/// contains a (100) reputation NFT with matching owner in datum and
/// skills meeting the minimum proficiency.
pub fn signer_has_reputation(
  extra_signatories: List<VerificationKeyHash>,
  reference_inputs: List<Input>,
  signer: VerificationKeyHash,
  reputation_policy: PolicyId,
  membership_subjects: List<ByteArray>,
  min_proficiency: ProficiencyLevel,
) -> Bool {
  list.has(extra_signatories, signer) && has_qualifying_reputation(
    reference_inputs,
    reputation_policy,
    membership_subjects,
    min_proficiency,
    signer,
  )
}
