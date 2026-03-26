//// Proposal Validator
////
//// Manages the proposal lifecycle: Draft → Published → Approved/Rejected.
//// Per whitepaper §4.5:
////   - Any DAO member can submit a draft
////   - Committee approves drafts for voting
////   - Only members meeting the proposal's min skill level can vote
////   - Resolved after voting deadline by simple majority

use aiken/collection/list
use cardano/address.{Script}
use cardano/assets
use cardano/transaction.{InlineDatum, Output, OutputReference, Transaction}
use alexandria/types.{
  Approved, ApproveForVoting, CancelProposal, CastProposalVote, DaoDatum,
  Draft, ProposalDatum, ProposalRedeemer, Published, Rejected, ResolveProposal,
  SubmitDraft,
}
use alexandria/reputation
use alexandria/utils

validator proposal {
  spend(
    datum: Option<ProposalDatum>,
    redeemer: ProposalRedeemer,
    own_ref: OutputReference,
    tx: Transaction,
  ) {
    expect Some(old) = datum

    // Find own script hash
    expect Some(own_input) =
      list.find(tx.inputs, fn(i) { i.output_reference == own_ref })
    expect Script(own_hash) = own_input.output.address.payment_credential

    when redeemer is {
      SubmitDraft -> {
        // Must be Draft status
        expect old.status == Draft

        // Author must have signed
        let author_signed = list.has(tx.extra_signatories, old.author)

        // Author must prove DAO membership via reputation reference input
        let is_member =
          reputation.has_qualifying_reputation(
            tx.reference_inputs,
            old.reputation_policy,
            old.membership_subjects,
            old.min_vote_proficiency,
            old.author,
          )

        // DAO state token must exist as reference input
        let dao_exists =
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

        author_signed && is_member && dao_exists
      }

      ApproveForVoting { voting_duration } -> {
        expect old.status == Draft

        // Get DAO datum to check committee
        expect Some(dao_input) =
          list.find(
            tx.reference_inputs,
            fn(i) {
              assets.has_nft(
                i.output.value,
                old.dao_state_token_policy,
                old.dao_state_token_name,
              )
            },
          )
        expect InlineDatum(dao_raw) = dao_input.output.datum
        expect dao: DaoDatum = dao_raw

        // Committee quorum must have signed
        let quorum =
          utils.committee_quorum_signed(tx.extra_signatories, dao.committee)

        // Find continuing output
        let cont = find_continuing_output(tx.outputs, own_hash)
        expect InlineDatum(new_raw) = cont.datum
        expect new: ProposalDatum = new_raw

        // Status must transition to Published
        let status_ok = new.status == Published

        // Voting deadline must be set (lower bound of validity + duration)
        let deadline_set = new.voting_deadline > 0 && voting_duration > 0

        // Immutable fields preserved
        let preserved =
          new.proposal_id == old.proposal_id && new.author == old.author && new.category == old.category && new.min_vote_proficiency == old.min_vote_proficiency && new.content_cid == old.content_cid && new.votes_for == 0 && new.votes_against == 0

        quorum && status_ok && deadline_set && preserved
      }

      CancelProposal -> {
        // Can cancel from Draft or Published
        expect old.status == Draft || old.status == Published

        // Author or committee can cancel
        let author_cancels = list.has(tx.extra_signatories, old.author)

        // Also check committee via DAO reference input
        let committee_cancels =
          when
            list.find(
              tx.reference_inputs,
              fn(i) {
                assets.has_nft(
                  i.output.value,
                  old.dao_state_token_policy,
                  old.dao_state_token_name,
                )
              },
            )
          is {
            Some(dao_input) -> {
              expect InlineDatum(dao_raw) = dao_input.output.datum
              expect dao: DaoDatum = dao_raw
              utils.committee_quorum_signed(tx.extra_signatories, dao.committee)
            }
            None -> False
          }

        author_cancels || committee_cancels
      }

      CastProposalVote { vote_for } -> {
        expect old.status == Published

        // Must be before voting deadline
        let in_time = utils.before_deadline(tx.validity_range, old.voting_deadline)

        // Voter must have signed
        // We need at least one extra signatory who has qualifying reputation
        let voter_qualified =
          list.any(
            tx.extra_signatories,
            fn(signer) {
              reputation.has_qualifying_reputation(
                tx.reference_inputs,
                old.reputation_policy,
                old.membership_subjects,
                old.min_vote_proficiency,
                signer,
              )
            },
          )

        // Find continuing output
        let cont = find_continuing_output(tx.outputs, own_hash)
        expect InlineDatum(new_raw) = cont.datum
        expect new: ProposalDatum = new_raw

        // Vote tally must be incremented correctly
        let tally_ok =
          if vote_for {
            new.votes_for == old.votes_for + 1 && new.votes_against == old.votes_against
          } else {
            new.votes_for == old.votes_for && new.votes_against == old.votes_against + 1
          }

        // Status stays Published
        let status_ok = new.status == Published

        // Everything else preserved
        let preserved =
          new.proposal_id == old.proposal_id && new.author == old.author && new.voting_deadline == old.voting_deadline

        in_time && voter_qualified && tally_ok && status_ok && preserved
      }

      ResolveProposal -> {
        expect old.status == Published

        // Must be after voting deadline
        let after_deadline =
          utils.after_deadline(tx.validity_range, old.voting_deadline)

        // Find continuing output
        let cont = find_continuing_output(tx.outputs, own_hash)
        expect InlineDatum(new_raw) = cont.datum
        expect new: ProposalDatum = new_raw

        // Determine result
        let should_approve = utils.simple_majority(old.votes_for, old.votes_against)

        let status_ok =
          if should_approve {
            new.status == Approved
          } else {
            new.status == Rejected
          }

        // Tallies must not change
        let tallies_preserved =
          new.votes_for == old.votes_for && new.votes_against == old.votes_against

        after_deadline && status_ok && tallies_preserved
      }
    }
  }

  else(_ctx) {
    fail @"unsupported purpose"
  }
}

fn find_continuing_output(
  outputs: List<Output>,
  script_hash: ByteArray,
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
    None -> fail @"no continuing proposal output"
  }
}
