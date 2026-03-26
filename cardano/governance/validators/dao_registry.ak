//// DAO Registry Validator
////
//// Manages DAO state UTxOs. Each DAO has one UTxO at this script address
//// identified by a unique state token NFT.
////
//// Parameterized by:
////   authorized_admin — platform admin key for initial DAO creation

use aiken/builtin
use aiken/collection/list
use aiken/crypto.{VerificationKeyHash}
use cardano/address.{Script}
use cardano/transaction.{InlineDatum, OutputReference, Transaction}
use alexandria/types.{
  CreateDao, DaoDatum, DaoRedeemer, InstallCommittee, SubjectDao,
  SubjectFieldDao, UpdateDao, ElectionDatum, Finalized,
}
use alexandria/state_token
use alexandria/utils

validator dao_registry(authorized_admin: VerificationKeyHash) {
  spend(
    datum: Option<DaoDatum>,
    redeemer: DaoRedeemer,
    own_ref: OutputReference,
    tx: Transaction,
  ) {
    // Find own script hash
    expect Some(own_input) =
      list.find(tx.inputs, fn(i) { i.output_reference == own_ref })
    expect Script(own_hash) = own_input.output.address.payment_credential

    when redeemer is {
      CreateDao -> {
        // Only the authorized admin can create DAOs
        let admin_signed = list.has(tx.extra_signatories, authorized_admin)

        // A state token must be minted in this tx
        expect Some(dao) = datum
        let token_minted =
          state_token.mints_exactly_one(
            tx.mint,
            dao.state_token_policy,
            // State token name = "dao" ++ scope_id
            get_dao_token_name(dao),
          )

        // Output must go back to this script
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
        expect [_continuing] = continuing_outputs

        admin_signed && token_minted
      }

      UpdateDao -> {
        expect Some(old_dao) = datum

        // Requires committee quorum
        let quorum =
          utils.committee_quorum_signed(tx.extra_signatories, old_dao.committee)

        // State token must be preserved
        let token_name = get_dao_token_name(old_dao)
        let preserved =
          state_token.state_token_preserved(
            tx.inputs,
            tx.outputs,
            old_dao.state_token_policy,
            token_name,
          )

        // Find continuing output and verify immutable fields
        let cont_output =
          state_token.find_state_output(
            tx.outputs,
            old_dao.state_token_policy,
            token_name,
          )
        expect InlineDatum(new_raw) = cont_output.datum
        expect new_dao: DaoDatum = new_raw

        // Scope and state token policy must not change
        let immutable_ok =
          new_dao.scope == old_dao.scope && new_dao.state_token_policy == old_dao.state_token_policy

        // Output must be at the same script
        let at_script = state_token.output_to_script(cont_output, own_hash)

        quorum && preserved && immutable_ok && at_script
      }

      InstallCommittee { election_ref } -> {
        expect Some(old_dao) = datum

        // Find the election reference input
        expect Some(election_input) =
          list.find(
            tx.reference_inputs,
            fn(i) { i.output_reference == election_ref },
          )

        // Parse election datum — must be finalized
        expect InlineDatum(election_raw) = election_input.output.datum
        expect election: ElectionDatum = election_raw
        expect election.phase == Finalized

        // Election must belong to this DAO
        let token_name = get_dao_token_name(old_dao)
        let election_matches_dao =
          election.dao_state_token_policy == old_dao.state_token_policy && election.dao_state_token_name == token_name

        // State token must be preserved
        let preserved =
          state_token.state_token_preserved(
            tx.inputs,
            tx.outputs,
            old_dao.state_token_policy,
            token_name,
          )

        // Find continuing output
        let cont_output =
          state_token.find_state_output(
            tx.outputs,
            old_dao.state_token_policy,
            token_name,
          )
        expect InlineDatum(new_raw) = cont_output.datum
        expect new_dao: DaoDatum = new_raw

        // Committee must change, everything else preserved
        let scope_preserved = new_dao.scope == old_dao.scope
        let policy_preserved =
          new_dao.state_token_policy == old_dao.state_token_policy
        let at_script = state_token.output_to_script(cont_output, own_hash)

        // New term starts now (within validity range)
        let new_term_valid =
          new_dao.term_end == new_dao.term_start + new_dao.election_interval

        election_matches_dao && preserved && scope_preserved && policy_preserved && at_script && new_term_valid
      }
    }
  }

  else(_ctx) {
    fail @"unsupported purpose"
  }
}

/// Build the state token asset name for a DAO from its datum.
fn get_dao_token_name(dao: DaoDatum) -> ByteArray {
  when dao.scope is {
    SubjectFieldDao { scope_id } -> builtin.append_bytearray(#"64616f", scope_id)
    SubjectDao { scope_id } -> builtin.append_bytearray(#"64616f", scope_id)
  }
}
