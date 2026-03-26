//// Alexandria Governance — Utility Functions
////
//// Shared helpers for deadline checks, committee quorum verification,
//// vote counting, and candidate ranking.

use aiken/collection/list
use aiken/crypto.{VerificationKeyHash}
use aiken/interval

/// True if the tx validity range is entirely before the deadline.
/// Use for actions that must happen BEFORE a deadline (e.g. casting votes).
pub fn before_deadline(validity_range: interval.Interval, deadline: Int) -> Bool {
  validity_range |> interval.is_entirely_before(deadline)
}

/// True if the tx validity range is entirely after the deadline.
/// Use for actions that must happen AFTER a deadline (e.g. finalizing).
pub fn after_deadline(validity_range: interval.Interval, deadline: Int) -> Bool {
  validity_range |> interval.is_entirely_after(deadline)
}

/// Committee quorum: strict majority (> 50%) of committee signed the tx.
pub fn committee_quorum_signed(
  extra_signatories: List<VerificationKeyHash>,
  committee: List<VerificationKeyHash>,
) -> Bool {
  let total = list.length(committee)
  let signed =
    list.count(committee, fn(m) { list.has(extra_signatories, m) })
  2 * signed > total
}

/// All committee members signed the tx.
pub fn committee_all_signed(
  extra_signatories: List<VerificationKeyHash>,
  committee: List<VerificationKeyHash>,
) -> Bool {
  list.all(committee, fn(m) { list.has(extra_signatories, m) })
}

/// Check a specific signer is in extra_signatories.
pub fn signed_by(
  extra_signatories: List<VerificationKeyHash>,
  signer: VerificationKeyHash,
) -> Bool {
  list.has(extra_signatories, signer)
}

/// Simple majority: for > against.
pub fn simple_majority(votes_for: Int, votes_against: Int) -> Bool {
  votes_for > votes_against
}

/// Supermajority: for >= 2/3 of total.
pub fn supermajority(votes_for: Int, votes_against: Int) -> Bool {
  3 * votes_for >= 2 * ( votes_for + votes_against )
}

/// Select the top N candidates by vote count (descending).
pub fn top_n_candidates(
  candidates: List<(VerificationKeyHash, Int)>,
  n: Int,
) -> List<VerificationKeyHash> {
  candidates
    |> sort_by_votes_desc
    |> list.take(n)
    |> list.map(fn(pair) { pair.1st })
}

fn sort_by_votes_desc(
  items: List<(VerificationKeyHash, Int)>,
) -> List<(VerificationKeyHash, Int)> {
  list.foldr(items, [], insert_desc)
}

fn insert_desc(
  item: (VerificationKeyHash, Int),
  sorted: List<(VerificationKeyHash, Int)>,
) -> List<(VerificationKeyHash, Int)> {
  when sorted is {
    [] ->
      [item]
    [head, ..tail] ->
      if item.2nd >= head.2nd {
        [item, ..sorted]
      } else {
        [head, ..insert_desc(item, tail)]
      }
  }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

test quorum_3_of_5() {
  committee_quorum_signed([#"aa", #"bb", #"cc"], [#"aa", #"bb", #"cc", #"dd", #"ee"])
}

test quorum_2_of_5_fails() {
  !committee_quorum_signed([#"aa", #"bb"], [#"aa", #"bb", #"cc", #"dd", #"ee"])
}

test simple_majority_passes() {
  simple_majority(51, 49)
}

test simple_majority_tie_fails() {
  !simple_majority(50, 50)
}

test supermajority_two_thirds() {
  supermajority(2, 1)
}

test supermajority_below_fails() {
  !supermajority(60, 40)
}

test top_n_selects_winners() {
  top_n_candidates([(#"cc", 10), (#"aa", 50), (#"bb", 30)], 2) == [#"aa", #"bb"]
}
