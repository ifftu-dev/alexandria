//// Election Validator
////
//// Manages election lifecycle: Nomination → Voting → Finalized.
//// Committee members of a DAO open elections at term end.
//// Nominees prove eligibility via reputation reference inputs.
//// Voters prove membership via reputation reference inputs.
//// Results are finalized after the voting deadline.

use aiken/collection/list
use cardano/address.{Script}
use cardano/assets
use cardano/transaction.{InlineDatum, Output, OutputReference, Transaction}
use alexandria/types.{
  AcceptNomination, ElectionDatum, ElectionRedeemer, FinalizeElection,
  Finalized, Nomination, Nominee, OpenElection, StartVoting, Voting,
}
use alexandria/reputation
use alexandria/state_token
use alexandria/utils

validator election {
  spend(
    datum: Option<ElectionDatum>,
    redeemer: ElectionRedeemer,
    own_ref: OutputReference,
    tx: Transaction,
  ) {
    expect Some(old) = datum

    // Find own script hash
    expect Some(own_input) =
      list.find(tx.inputs, fn(i) { i.output_reference == own_ref })
    expect Script(own_hash) = own_input.output.address.payment_credential

    when redeemer is {
      OpenElection -> {
        // The DAO's state token must be present as a reference input,
        // proving this election is authorized by the DAO
        let dao_present =
          list.any(
            tx.reference_inputs,
            fn(i) {
              assets.has_nft(
                i.output.value,
                old.dao_state_token_policy,
                old.dao_state_token_name,
              )
            },
          )

        // Must start in Nomination phase with empty nominees
        let valid_initial =
          old.phase == Nomination && list.length(old.nominees) == 0

        dao_present && valid_initial
      }

      AcceptNomination { nominee_index } -> {
        // Must be in Nomination phase
        expect old.phase == Nomination

        // Must be before nomination deadline
        let in_time = utils.before_deadline(tx.validity_range, old.nomination_end)

        // Find continuing output
        let cont =
          find_continuing_output(tx.outputs, own_hash, old)
        expect InlineDatum(new_raw) = cont.datum
        expect new: ElectionDatum = new_raw

        // Get the nominee being accepted
        expect Some(nominee) = list.at(old.nominees, nominee_index)

        // Nominee must have signed the tx
        let nominee_signed =
          list.has(tx.extra_signatories, nominee.key_hash)

        // Nominee must prove reputation via reference input
        let has_reputation =
          reputation.has_qualifying_reputation(
            tx.reference_inputs,
            old.reputation_policy,
            old.membership_subjects,
            old.nominee_min_proficiency,
            nominee.key_hash,
          )

        // New datum must mark this nominee as accepted
        let nominee_updated =
          check_nominee_accepted(old.nominees, new.nominees, nominee_index)

        // Everything else must stay the same
        let rest_preserved =
          new.phase == old.phase && new.election_id == old.election_id && new.seats == old.seats && new.nomination_end == old.nomination_end && new.voting_end == old.voting_end

        in_time && nominee_signed && has_reputation && nominee_updated && rest_preserved
      }

      StartVoting -> {
        // Must be in Nomination phase
        expect old.phase == Nomination

        // Must be after nomination deadline
        let after_nom = utils.after_deadline(tx.validity_range, old.nomination_end)

        // Find continuing output
        let cont =
          find_continuing_output(tx.outputs, own_hash, old)
        expect InlineDatum(new_raw) = cont.datum
        expect new: ElectionDatum = new_raw

        // Phase must transition to Voting
        let phase_ok = new.phase == Voting

        // Only accepted nominees should remain
        let accepted_only =
          list.all(new.nominees, fn(n) { n.accepted })

        // Must have at least as many accepted nominees as seats
        let enough_nominees = list.length(new.nominees) >= new.seats

        // Rest preserved
        let rest_ok =
          new.election_id == old.election_id && new.seats == old.seats && new.voting_end == old.voting_end

        after_nom && phase_ok && accepted_only && enough_nominees && rest_ok
      }

      FinalizeElection { winners } -> {
        // Must be in Voting phase
        expect old.phase == Voting

        // Must be after voting deadline
        let after_vote = utils.after_deadline(tx.validity_range, old.voting_end)

        // Find continuing output
        let cont =
          find_continuing_output(tx.outputs, own_hash, old)
        expect InlineDatum(new_raw) = cont.datum
        expect new: ElectionDatum = new_raw

        // Phase must transition to Finalized
        let phase_ok = new.phase == Finalized

        // Winners count must equal seats
        let correct_count = list.length(winners) == old.seats

        // All winners must be accepted nominees
        let all_nominees =
          list.all(
            winners,
            fn(w) { list.any(old.nominees, fn(n) { n.key_hash == w && n.accepted }) },
          )

        after_vote && phase_ok && correct_count && all_nominees
      }
    }
  }

  else(_ctx) {
    fail @"unsupported purpose"
  }
}

/// Find the continuing output at the election script with the same DAO reference.
fn find_continuing_output(
  outputs: List<Output>,
  script_hash: ByteArray,
  old: ElectionDatum,
) -> Output {
  when
    list.find(
      outputs,
      fn(o) {
        when o.address.payment_credential is {
          Script(h) -> h == script_hash
          _ -> False
        }
      },
    )
  is {
    Some(o) -> o
    None -> fail @"no continuing election output"
  }
}

/// Verify that exactly one nominee at the given index is marked accepted.
fn check_nominee_accepted(
  old_nominees: List<Nominee>,
  new_nominees: List<Nominee>,
  index: Int,
) -> Bool {
  list.length(old_nominees) == list.length(new_nominees) && list.indexed_foldr(
    new_nominees,
    True,
    fn(i, new_nom, acc) {
      expect Some(old_nom) = list.at(old_nominees, i)
      if i == index {
        // This nominee should now be accepted, key_hash unchanged
        acc && new_nom.accepted && new_nom.key_hash == old_nom.key_hash
      } else {
        // All other nominees unchanged
        acc && new_nom == old_nom
      }
    },
  )
}
