//// Anchor Sponsor Validator
////
//// Holds treasury ADA so any node can pay its credential-anchor tx fee.
//// See `docs/sponsored-anchoring-spec.md` ("Anchor sponsor validator").
//// The validator does not inspect metadata or the anchored content.
////
//// Parameters (applied at deploy, in this order):
////   fee_cap           — max lovelace a single `Sponsor` spend may consume
////   window            — rate-limit window per sponsor UTxO (POSIX ms)
////   max_validity      — max validity-interval width (POSIX ms)
////   reclaim_keys      — reclaim multisig key hashes
////   reclaim_threshold — signatures from `reclaim_keys` needed to `Reclaim`
////
//// Rules for `Sponsor` (all must hold):
////   0. `window > max_validity` (fail closed otherwise: the rate limit
////      relies on it).
////   1. Exactly one tx input is locked by this script.
////   2. Exactly one output is locked by this script; it goes to the own
////      input's exact address and carries an inline `SponsorDatum`.
////   3. The lovelace leaving the script is at most `fee_cap` and at most
////      the tx fee (it pays the fee, never another output), and the output
////      keeps exactly the input's non-ADA assets.
////   4. The validity range has finite bounds, is at most `max_validity`
////      wide, and its lower bound is at or after `datum.next_allowed`.
////   5. The output datum's `next_allowed` is `lower + window`.
////   6. At least one key signed the tx.
////
//// Rules for `Reclaim`:
////   At least `reclaim_threshold` (>= 1) distinct `reclaim_keys` signed.

use aiken/collection/list
use aiken/crypto.{VerificationKeyHash}
use aiken/interval.{Finite, Interval}
use alexandria/sponsor.{Reclaim, Sponsor, SponsorDatum, SponsorRedeemer}
use cardano/address.{Address, Script}
use cardano/assets
use cardano/transaction.{
  InlineDatum, Input, Output, OutputReference, Transaction,
}

validator anchor_sponsor(
  fee_cap: Int,
  window: Int,
  max_validity: Int,
  reclaim_keys: List<VerificationKeyHash>,
  reclaim_threshold: Int,
) {
  spend(
    datum: Option<SponsorDatum>,
    redeemer: SponsorRedeemer,
    own_ref: OutputReference,
    tx: Transaction,
  ) {
    when redeemer is {
      Sponsor -> {
        // 0. The rate limit only holds if the window outlasts any interval.
        expect window > max_validity
        expect Some(old) = datum

        expect Some(own_input) = list.find(
          tx.inputs,
          fn(i) { i.output_reference == own_ref },
        )
        let own_address = own_input.output.address
        expect Script(own_hash) = own_address.payment_credential

        // 1. Exactly one input locked by this script.
        let script_inputs = list.count(
          tx.inputs,
          fn(i) { locked_by(i.output.address, own_hash) },
        )
        expect script_inputs == 1

        // 2. Exactly one output locked by this script, at the same address.
        expect [cont] = list.filter(
          tx.outputs,
          fn(o) { locked_by(o.address, own_hash) },
        )
        expect cont.address == own_address
        expect InlineDatum(new_raw) = cont.datum
        expect new: SponsorDatum = new_raw

        // 3. What leaves the script pays the fee and nothing else: at most
        //    `fee_cap`, and no more than the tx's fee, so no lovelace can
        //    reach an output the spender owns. Native assets untouched.
        let in_value = own_input.output.value
        let spent = assets.lovelace_of(in_value) - assets.lovelace_of(
          cont.value,
        )
        let ada_ok = spent <= fee_cap && spent <= tx.fee
        let assets_ok = assets.without_lovelace(cont.value) == assets.without_lovelace(
          in_value,
        )

        // 4. Finite, narrow validity range starting at or after next_allowed.
        expect Some((lower, upper)) = finite_bounds(tx.validity_range)
        let range_ok = upper - lower <= max_validity && lower >= old.next_allowed

        // 5. Next spend no earlier than one window after this one.
        let next_ok = new.next_allowed == lower + window

        // 6. Someone signed (the node's required signer).
        let signed = !list.is_empty(tx.extra_signatories)

        ada_ok && assets_ok && range_ok && next_ok && signed
      }

      Reclaim -> {
        expect reclaim_threshold >= 1
        let signed = list.count(
          list.unique(reclaim_keys),
          fn(k) { list.has(tx.extra_signatories, k) },
        )
        signed >= reclaim_threshold
      }
    }
  }

  else(_ctx) {
    fail @"anchor_sponsor: unsupported purpose"
  }
}

/// `Some((lower, upper))` when both bounds of the range are finite.
fn finite_bounds(range: Interval) -> Option<(Int, Int)> {
  when (range.lower_bound.bound_type, range.upper_bound.bound_type) is {
    (Finite(lower), Finite(upper)) -> Some((lower, upper))
    _ -> None
  }
}

fn locked_by(address: Address, script_hash: ByteArray) -> Bool {
  when address.payment_credential is {
    Script(h) -> h == script_hash
    _ -> False
  }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

const test_fee_cap = 2_000_000

const test_window = 600_000

const test_max_validity = 300_000

const test_reclaim_keys: List<VerificationKeyHash> = [#"a1", #"a2", #"a3"]

const test_threshold = 2

const t_script_hash =
  #"5050505050505050505050505050505050505050505050505050505050"

const t_other_hash =
  #"6060606060606060606060606060606060606060606060606060606060"

const node_key = #"0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e"

const policy = #"70707070707070707070707070707070707070707070707070707070"

const t_next_allowed = 1_000_000

const t_ref = OutputReference { transaction_id: #"aa", output_index: 0 }

fn t_address() -> Address {
  address.from_script(t_script_hash)
}

fn sponsor_output(value: assets.Value, next: Int) -> Output {
  Output {
    address: t_address(),
    value,
    datum: InlineDatum(SponsorDatum { next_allowed: next }),
    reference_script: None,
  }
}

fn t_sponsor_input(value: assets.Value) -> Input {
  Input {
    output_reference: t_ref,
    output: sponsor_output(value, t_next_allowed),
  }
}

fn wallet_input() -> Input {
  Input {
    output_reference: OutputReference { transaction_id: #"bb", output_index: 1 },
    output: Output {
      address: address.from_verification_key(node_key),
      value: assets.from_lovelace(5_000_000),
      datum: transaction.NoDatum,
      reference_script: None,
    },
  }
}

fn range(lower: Int, upper: Int) -> Interval {
  interval.between(lower, upper)
}

/// A well-formed sponsored anchor tx: 50 ADA in, 1.5 ADA fee, 48.5 ADA back.
fn valid_tx() -> Transaction {
  let lower = t_next_allowed + 10
  Transaction {
    ..transaction.placeholder,
    inputs: [t_sponsor_input(assets.from_lovelace(50_000_000)), wallet_input()],
    outputs: [
      sponsor_output(assets.from_lovelace(48_500_000), lower + test_window),
    ],
    fee: 1_500_000,
    validity_range: range(lower, lower + 200_000),
    extra_signatories: [node_key],
  }
}

fn run(tx: Transaction, redeemer: SponsorRedeemer) -> Bool {
  anchor_sponsor.spend(
    test_fee_cap,
    test_window,
    test_max_validity,
    test_reclaim_keys,
    test_threshold,
    Some(SponsorDatum { next_allowed: t_next_allowed }),
    redeemer,
    t_ref,
    tx,
  )
}

test sponsor_accepts_valid_spend() {
  run(valid_tx(), Sponsor)
}

test sponsor_accepts_exactly_fee_cap() {
  let tx = valid_tx()
  let lower = t_next_allowed + 10
  run(
    Transaction {
      ..tx,
      outputs: [
        sponsor_output(assets.from_lovelace(48_000_000), lower + test_window),
      ],
      fee: 2_000_000,
    },
    Sponsor,
  )
}

/// The gap between the fee cap and the real fee must not reach the spender.
test sponsor_rejects_lovelace_beyond_the_fee() fail {
  let tx = valid_tx()
  let lower = t_next_allowed + 10
  run(
    Transaction {
      ..tx,
      outputs: [
        sponsor_output(assets.from_lovelace(48_000_000), lower + test_window),
        Output {
          address: address.from_verification_key(node_key),
          value: assets.from_lovelace(1_600_000),
          datum: transaction.NoDatum,
          reference_script: None,
        },
      ],
      fee: 400_000,
    },
    Sponsor,
  )
}

/// A spender may add to the fee from their own inputs; the script still
/// pays at most its cap.
test sponsor_accepts_fee_topped_up_by_the_spender() {
  let tx = valid_tx()
  let lower = t_next_allowed + 10
  run(
    Transaction {
      ..tx,
      outputs: [
        sponsor_output(assets.from_lovelace(48_500_000), lower + test_window),
        Output {
          address: address.from_verification_key(node_key),
          value: assets.from_lovelace(2_500_000),
          datum: transaction.NoDatum,
          reference_script: None,
        },
      ],
      fee: 4_000_000,
    },
    Sponsor,
  )
}

test sponsor_rejects_over_fee_cap() fail {
  let tx = valid_tx()
  let lower = t_next_allowed + 10
  run(
    Transaction {
      ..tx,
      outputs: [
        sponsor_output(assets.from_lovelace(47_999_999), lower + test_window),
      ],
    },
    Sponsor,
  )
}

test sponsor_rejects_no_continuing_output() fail {
  run(Transaction { ..valid_tx(), outputs: [] }, Sponsor)
}

test sponsor_rejects_two_continuing_outputs() fail {
  let tx = valid_tx()
  let lower = t_next_allowed + 10
  run(
    Transaction {
      ..tx,
      outputs: [
        sponsor_output(assets.from_lovelace(48_500_000), lower + test_window),
        sponsor_output(assets.from_lovelace(2_000_000), lower + test_window),
      ],
    },
    Sponsor,
  )
}

test sponsor_rejects_continuing_output_at_other_stake_address() fail {
  let tx = valid_tx()
  let lower = t_next_allowed + 10
  let moved = Output {
    ..sponsor_output(assets.from_lovelace(48_500_000), lower + test_window),
    address: t_address() |> address.with_delegation_key(node_key),
  }
  run(Transaction { ..tx, outputs: [moved] }, Sponsor)
}

test sponsor_rejects_extra_script_input() fail {
  let tx = valid_tx()
  let second = Input {
    output_reference: OutputReference { transaction_id: #"cc", output_index: 0 },
    output: sponsor_output(assets.from_lovelace(50_000_000), 0),
  }
  run(Transaction { ..tx, inputs: [second, ..tx.inputs] }, Sponsor)
}

test sponsor_ignores_other_script_inputs() {
  let tx = valid_tx()
  let foreign = Input {
    output_reference: OutputReference { transaction_id: #"dd", output_index: 0 },
    output: Output {
      ..sponsor_output(assets.from_lovelace(3_000_000), 0),
      address: address.from_script(t_other_hash),
    },
  }
  run(Transaction { ..tx, inputs: [foreign, ..tx.inputs] }, Sponsor)
}

test sponsor_rejects_before_next_allowed() fail {
  let lower = t_next_allowed - 1
  run(
    Transaction {
      ..valid_tx(),
      outputs: [
        sponsor_output(assets.from_lovelace(48_500_000), lower + test_window),
      ],
      validity_range: range(lower, lower + 200_000),
    },
    Sponsor,
  )
}

test sponsor_rejects_backdated_wide_interval() fail {
  // Lower bound back-dated to t_next_allowed, upper bound far in the future:
  // width exceeds max_validity.
  let lower = t_next_allowed
  run(
    Transaction {
      ..valid_tx(),
      outputs: [
        sponsor_output(assets.from_lovelace(48_500_000), lower + test_window),
      ],
      validity_range: range(lower, lower + test_max_validity + 1),
    },
    Sponsor,
  )
}

test sponsor_accepts_interval_at_max_width() {
  let lower = t_next_allowed
  run(
    Transaction {
      ..valid_tx(),
      outputs: [
        sponsor_output(assets.from_lovelace(48_500_000), lower + test_window),
      ],
      validity_range: range(lower, lower + test_max_validity),
    },
    Sponsor,
  )
}

test sponsor_rejects_infinite_upper_bound() fail {
  run(
    Transaction {
      ..valid_tx(),
      validity_range: interval.after(t_next_allowed + 10),
    },
    Sponsor,
  )
}

test sponsor_rejects_infinite_lower_bound() fail {
  run(
    Transaction {
      ..valid_tx(),
      validity_range: interval.before(t_next_allowed + 10),
    },
    Sponsor,
  )
}

test sponsor_rejects_wrong_next_allowed() fail {
  let lower = t_next_allowed + 10
  run(
    Transaction {
      ..valid_tx(),
      outputs: [
        sponsor_output(
          assets.from_lovelace(48_500_000),
          lower + test_window - 1,
        ),
      ],
    },
    Sponsor,
  )
}

test sponsor_rejects_assets_added() fail {
  let lower = t_next_allowed + 10
  let with_token = assets.from_lovelace(48_500_000)
    |> assets.add(policy, #"01", 1)
  run(
    Transaction {
      ..valid_tx(),
      outputs: [sponsor_output(with_token, lower + test_window)],
    },
    Sponsor,
  )
}

test sponsor_rejects_assets_removed() fail {
  let lower = t_next_allowed + 10
  let in_value = assets.from_lovelace(50_000_000)
    |> assets.add(policy, #"01", 1)
  run(
    Transaction {
      ..valid_tx(),
      inputs: [t_sponsor_input(in_value), wallet_input()],
      outputs: [
        sponsor_output(assets.from_lovelace(48_500_000), lower + test_window),
      ],
    },
    Sponsor,
  )
}

test sponsor_accepts_assets_preserved() {
  let lower = t_next_allowed + 10
  let token = assets.from_asset(policy, #"01", 1)
  run(
    Transaction {
      ..valid_tx(),
      inputs: [
        t_sponsor_input(assets.merge(assets.from_lovelace(50_000_000), token)),
        wallet_input(),
      ],
      outputs: [
        sponsor_output(
          assets.merge(assets.from_lovelace(48_500_000), token),
          lower + test_window,
        ),
      ],
    },
    Sponsor,
  )
}

test sponsor_rejects_no_signatories() fail {
  run(Transaction { ..valid_tx(), extra_signatories: [] }, Sponsor)
}

/// Runs a `Sponsor` spend under a custom `window`, with the continuing
/// datum set consistently (`lower + window`) so only the window guard can
/// reject it.
fn run_with_window(window: Int) -> Bool {
  let lower = t_next_allowed + 10
  anchor_sponsor.spend(
    test_fee_cap,
    window,
    test_max_validity,
    test_reclaim_keys,
    test_threshold,
    Some(SponsorDatum { next_allowed: t_next_allowed }),
    Sponsor,
    t_ref,
    Transaction {
      ..valid_tx(),
      outputs: [
        sponsor_output(assets.from_lovelace(48_500_000), lower + window),
      ],
    },
  )
}

test sponsor_accepts_window_just_above_max_validity() {
  run_with_window(test_max_validity + 1)
}

test sponsor_rejects_window_not_above_max_validity() fail {
  run_with_window(test_max_validity)
}

test reclaim_with_quorum_passes() {
  run(
    Transaction { ..transaction.placeholder, extra_signatories: [#"a1", #"a3"] },
    Reclaim,
  )
}

test reclaim_below_quorum_fails() fail {
  run(
    Transaction {
      ..transaction.placeholder,
      extra_signatories: [#"a2", node_key],
    },
    Reclaim,
  )
}

test reclaim_duplicate_signer_counted_once() fail {
  run(
    Transaction { ..transaction.placeholder, extra_signatories: [#"a1", #"a1"] },
    Reclaim,
  )
}

test reclaim_duplicate_param_key_counted_once() fail {
  anchor_sponsor.spend(
    test_fee_cap,
    test_window,
    test_max_validity,
    [#"a1", #"a1", #"a2"],
    2,
    Some(SponsorDatum { next_allowed: t_next_allowed }),
    Reclaim,
    t_ref,
    Transaction { ..transaction.placeholder, extra_signatories: [#"a1"] },
  )
}

test reclaim_rejects_zero_threshold() fail {
  anchor_sponsor.spend(
    test_fee_cap,
    test_window,
    test_max_validity,
    test_reclaim_keys,
    0,
    Some(SponsorDatum { next_allowed: t_next_allowed }),
    Reclaim,
    t_ref,
    transaction.placeholder,
  )
}
