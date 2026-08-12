# Contributing to Alexandria

Thanks for wanting to work on this. Alexandria is learning infrastructure —
free for learners, permanently — and contributions are welcome.

## Licensing: read this first

Alexandria is **AGPL-3.0-or-later, all of it**. There is no proprietary subtree
in this repository and no contribution path that lands in one.

Copyleft is the point rather than an inconvenience: MIT would let someone take
this work, improve it, and keep the improvements closed. The AGPL keeps the
commons a commons — anyone may use, study, modify and sell it, but a modified
version offered to others must be published under the same terms. Nothing about
that restricts a learner.

The commercial Enterprise Edition is a *shell around* this application, not a
layer inside it: organisation identity and provisioning, policy and access
control, audit and compliance, hosted services the app talks to, and the
operational guarantees that go with them. It lives in a separate repository
and this one neither references nor depends on it. Alexandria is complete and
useful with the Enterprise Edition absent from the universe.

Two consequences worth stating plainly:

- **Nothing in the app is ever withheld from you.** No feature flag, no
  licence check, no build that unlocks more than the one you can compile
  yourself. Credential verification in particular is free, offline-capable
  and permanent.
- **Vendored third-party crates keep their own licences.** `crates/iroh-moq`
  and `crates/moq-media` are Copyright (C) 2025 N0, INC under MIT OR
  Apache-2.0, as is `crates/live`. Contributions there follow upstream's terms.
- **Every path here accepts external contributions.** There is no directory
  you are barred from.

If you are unsure whether an idea belongs in the app or in the commercial
shell, the test is: *would a single individual with no employer ever want
this?* If yes, it belongs here. See `docs/enterprise-boundary.md`.

### Sign your commits (DCO)

Every commit must carry a `Signed-off-by:` line:

```
git commit -s -m "your message"
```

That line is the [Developer Certificate of Origin 1.1](https://developercertificate.org/):
you are asserting you wrote the change, or have the right to submit it under
the AGPL. It is not a copyright assignment and it does not take anything from
you.

### Why a contributor agreement is coming

Being straight about this, because discovering it later feels like a bait and
switch.

The commercial Enterprise Edition is a separate, proprietary service. It links
code from this repository. That is only lawful because Alexandria Pvt. Ltd.
currently holds the copyright in all of it — a copyright holder is not bound by
the licence it grants everyone else. The moment a contribution lands here whose
copyright the company does not hold, that code becomes AGPL-only for every
purpose, including ours, and the Enterprise Edition can never use it.

So substantive external contributions will need a contributor agreement, and it
is not drafted yet. Until it exists:

- Small fixes — typos, bugs, docs, tests — are welcome now under the DCO alone.
- If you are planning something larger, open an issue first so we can tell you
  where the agreement stands rather than have you write code we cannot merge.

What the agreement will and will not do: it will let the company license *your
contribution* under terms other than the AGPL, which is what keeps the
Enterprise Edition lawful. It will not assign your copyright away, it will not
let anyone take this application proprietary, and it changes nothing about the
AGPL rights everyone else has in the result.

## Before you push

Run the local mirror of CI:

```bash
./scripts/check.sh          # everything
./scripts/check.sh --fast   # skip the Rust test suite (slowest step)
```

This runs the same gates CI does: `cargo fmt --check`, `cargo clippy -D
warnings`, the Rust test suite, both build configurations, the enterprise
boundary guard, the Tauri command guard, i18n parity, `vue-tsc`, and vitest.

## Code style

Enforced by CI on every push and pull request:

- **Rust** — `cargo fmt --check` and `cargo clippy -- -D warnings` must pass.
- **Vue / TypeScript** — `vue-tsc -b --noEmit` must pass in strict mode
  (`noUnusedLocals`, `noUnusedParameters`, `noFallthroughCasesInSwitch`,
  `noUncheckedSideEffectImports`).

Match the surrounding code — its naming, its comment density, its idiom.
Comments here tend to explain *why*, not *what*.

## Things that are load-bearing

A few invariants are enforced by tests because breaking them silently would
be bad. If a change makes one of these fail, that is a signal to reconsider
the change, not to update the test:

- **Privacy.** Published profiles must not carry account-private fields;
  guardian data never rides gossip or device sync; `SYNCABLE_TABLES` is
  pinned at exactly three tables; a private profile queried by username
  answers `NotOwner`, not `Private`.
- **Answer keys never leave the host.** Assessment grading happens
  backend-side; the client never receives `correct_indices`.
- **Grader determinism.** `wasm_runtime.rs::grader_config()` and the ABI v1
  contract are frozen. Every historical score must stay re-derivable —
  third-party re-derivation is a product, not just a nicety. Extend via
  manifest budgets and new plugins, never by changing the runtime config.
- **The community build never depends on `ee/`.** A checkout with both
  `ee/` directories deleted must build, typecheck, and pass tests.

## Commits and pull requests

- Keep unrelated changes in separate commits.
- Explain *why* in the commit body when the reason isn't obvious from the
  diff.
- If your change touches documented behaviour, say so in the PR — docs
  updates are usually wanted, but we would rather discuss than have them
  drift.
