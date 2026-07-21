# Enterprise Boundary

**Status:** Policy adopted; wiring on `feat/ee-boundary-wiring`
**Date:** 2026-07-21

Alexandria is open core. Everything in this repository is MIT Expat
except the `ee/` subtrees, which fall under the IFFTU Enterprise License
(see `LICENSE.md` and `src/ee/LICENSE.md` /
`src-tauri/src/ee/LICENSE.md`).

This document defines **what qualifies as enterprise**. Before this
existed, the license carved out a boundary that no written policy
described, so every new feature re-litigated the question. The rubric
below turns that argument into a checklist.

## The three tests

A feature may live under `ee/` only if it passes **all three**. Failing
any one test means it is core, and core means MIT.

| Test | Question | Core (MIT) if… |
|---|---|---|
| **Learner-value** | Does removing this degrade a solo learner's ability to learn, be assessed, or hold and present their own credentials? | Yes → MIT |
| **Multi-tenant** | Does it require server-side state about *other people*, organisation tenancy, or seat accounting? | No → MIT |
| **Graph-integrity** | Is it part of how a credential is produced, signed, verified, or independently re-derived? | Yes → MIT |

The tests are deliberately biased toward MIT. When a feature sits on the
line, it is core.

## Derived rules

These follow from the three tests. They are written down so they are not
re-argued per feature.

**Assessment engine internals are always MIT.** Item response theory,
adaptive delivery, attempt policy, grader plugins, Bloom levels,
aggregation and decay. A proprietary measurement engine makes a
credential impossible to check, and an uncheckable credential is worth
nothing to the employer who is being asked to trust it. The measurement
engine is the product's foundation, not its upsell.

**Verification is always MIT.** `domain/vc/verify.rs` and the grader
re-derivation path stay MIT permanently. What is sold is the *hosted,
rate-limited, SLA-backed endpoint* — the operation, not the algorithm.
Anyone must be able to verify an Alexandria credential offline with no
subscription and no network. This is a hard constraint, not a
preference: it is the reason the credential has value.

**Anything scoped by `org_id` is EE.** Cross-tenant queries, employer
consoles, seat metering, org-scoped analytics.

**The consent and publish client is MIT; the index that receives it is
EE.** A learner must be able to read the code that decides what leaves
their machine. The server that stores it is a different question.

**Learning content and credential issuance are never gated.** Per
`docs/vision.md`, learning content, credentials, and reputation data are
free permanently and unconditionally.

## Two constraints from `docs/vision.md`

Both bear directly on where the line falls, and both are easy to violate
by accident.

**"Learner data is never sold, and all queries respect learner-controlled
privacy settings"** (`vision.md:129`). Any enterprise product built on
learner data must be consent-gated at the source. There is no enterprise
tier that sees data a learner has not explicitly published, and no
enterprise contract that overrides a learner's privacy setting.

**"These operate through the same query system available to everyone"**
(`vision.md:129`). Enterprise access is *quantitative* — rate limits,
bulk operations, SLA, support, integrations — not a privileged query
surface with access to fields nobody else can see. If an enterprise
endpoint can answer a question the public one structurally cannot, that
is a boundary violation, not a feature.

Note also that Alexandria is structured as a non-profit
(`vision.md:124`). Enterprise revenue funds the mission; it does not
redefine it.

## Worked examples

| Feature | Verdict | Reasoning |
|---|---|---|
| Adaptive assessment / IRT | **MIT** | Fails learner-value and graph-integrity — it is how a score is produced |
| Attempt policy and cooldowns | **MIT** | Credential integrity; a learner-side rule |
| Artifact grader plugins (git repo, spreadsheet) | **MIT** | Graph-integrity — scores must be re-derivable by anyone |
| Credential verification logic | **MIT** | Graph-integrity, permanently |
| Talent-index consent UI and publish client | **MIT** | Learner must audit what leaves their device |
| Wire schema for the published record | **MIT** | Must be publicly auditable |
| Talent index service and storage | **EE** | Multi-tenant server-side state about other people |
| Employer capability search console | **EE** | Scoped by `org_id` |
| Hosted verification API (rate limits, keys, SLA) | **EE** | The hosted operation; the logic underneath stays MIT |
| Bulk verification | **EE** | Metered enterprise operation |
| Seat accounting and billing | **EE** | Definitionally multi-tenant |
| Skills-intelligence aggregates | **EE** | Cross-user server-side aggregation |
| Hosted AI oral-exam inference | **EE** | Metered server-side inference; learners retain every other assessment path |

## How the boundary is enforced

Policy is not enough; the build enforces it mechanically.

- The `ee` Cargo feature is **off by default**. A plain `cargo build`
  produces the community edition, and it is fully functional.
- Core references `ee` in exactly one place:
  `#[cfg(feature = "ee")] mod ee;` in `src-tauri/src/lib.rs`. Every
  other core→ee interaction goes through a trait defined in core with a
  no-op MIT default implementation — one seam per engine.
- The frontend resolves `@ee` to `src/ee` when `VITE_EE=1` and to the
  MIT `src/ee-stub` otherwise.
- CI gates every push: the no-feature build and test suite must pass,
  the `--features ee` build must pass, no unconditional `ee::` or `@ee`
  reference may appear outside an `ee/` tree, SPDX headers must be
  present, and both frontend builds must succeed.

The first gate is the one that matters. If core ever references `ee`
unconditionally, the community build stops compiling — the boundary
fails loudly rather than eroding quietly.

## Adding a feature

1. Run the three tests. If any says core, it is MIT. Stop.
2. If all three say enterprise, add it under `ee/` with the
   `LicenseRef-IFFTU-Enterprise` SPDX header.
3. If core needs to call it, use the existing trait seam for that
   engine, or add one. Do not add a second seam to an engine that
   already has one.
4. Add a row to the worked-examples table above.

If a feature seems to need a new gossip topic or a widened
`SYNCABLE_TABLES` to work, it is the wrong feature — see
`domain/sync.rs` and the privacy invariants it asserts.
