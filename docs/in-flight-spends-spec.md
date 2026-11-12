# In-flight spends: reserve wallet inputs and expire dead submissions

Status: active
Updated: 2026-11-10
Triage: ready-for-agent
Repos: alexandria
Blocked by: alexandria/docs/sponsored-anchoring-spec.md

Step 1 of the path in the Blockfrost dependency research (`docs/blockfrost-dependency-research.md`, section 4.1), widened after checking it against the code. The code this spec changes is the batched, sponsored anchoring work, so it builds on that spec's implementation, not on the code before it.

## Problem Statement

A node writes to Cardano from one wallet for three features: credential anchoring (integrity anchors, issuer-paid or sponsor-paid), username anchoring, and completion witness minting. Each feature picks the UTxOs it spends from a chain provider's view of the wallet address. That view only reflects confirmed blocks, so for a while after a transaction is submitted, the provider still lists the UTxOs it spent.

Nothing in the node remembers which UTxOs its own submitted transactions have spent. Every selector is deterministic (largest, smallest, or first that is large enough), so a builder that runs after a submission, in the same background pass or the next one, picks the same UTxO again. The node rejects that transaction as a double spend (`BadInputsUTxO`). This happened on a live preprod run. The usual trigger needs no unusual provider lag: a completion mint and an issuer-paid anchor in the same pass both want the wallet's largest UTxO, seconds apart.

What happens next depends on the feature:

- **Credential anchoring** records the rejection as an outcome-unknown submission. The anchor batch waits until its validity interval plus a margin has passed (about 90 minutes), then is released and spends one of the credential's five attempts. Repeated collisions can mark a credential's anchor failed, so the issuer has to retry it by hand.
- **Username anchoring and completion minting** have no expiry. A rejected transaction stays outcome-unknown forever. Recovery polls the provider for it on every pass, and the journal refuses to build a replacement. A learner's completion witness can never be minted, and the usernames in that batch are never anchored.

The same blind spot covers collateral. A sponsor-paid anchor and a completion mint each take a collateral UTxO from the wallet. If that UTxO is an input of another in-flight transaction, the new transaction is rejected the same way.

## Solution

The node keeps an exact record of what its in-flight transactions spend, and every builder steps around it:

- **Reserved outpoints.** Every journaled transaction stays in the submission journal as exact signed bytes. The inputs and collateral inputs of each one whose validity interval (plus a margin) has not yet passed are reserved. Reservations ignore submission status: during that window the transaction is either on chain, so its inputs are spent for good, or may still land. After the window it is one or the other forever, and the provider's view is accurate either way.
- **Every selector skips reserved outpoints.** This covers the issuer-paid anchor input, sponsor-paid anchor collateral, the sponsor pool UTxO, username anchor inputs, and completion mint input and collateral. When every eligible UTxO is reserved, the builder reports that the wallet is busy and the feature waits; it never submits a transaction it knows will be rejected.
- **The journal proves when a transaction can never land.** When the tip is past a journaled transaction's validity interval plus the margin, and the provider still has no receipt for it, the journal marks it expired. Ledger rules forbid including it later. An expired transaction no longer reserves anything, and its operation may get a replacement transaction. Credential anchoring, username anchoring and completion minting all use this one rule.

From a user's point of view, credentials, usernames and completion witnesses stop failing or sticking because of the node's own earlier transactions. When the wallet is briefly busy, they wait and say why.

## User Stories

1. As an issuer, I want my node never to submit a transaction that spends a UTxO my own in-flight transaction already spends, so that my credentials are not delayed by self-inflicted double spends.
2. As an issuer, I want a credential anchor batch to avoid every UTxO used by an in-flight completion mint or username anchor batch, so that features sharing my wallet do not collide.
3. As an issuer, I want an issuer-paid anchor and a completion mint built in the same background pass to spend different UTxOs, so that both can land.
4. As an issuer, I want a sponsor-paid anchor's collateral to be a UTxO no in-flight transaction spends, so that the anchor is not rejected for missing collateral.
5. As an issuer, I want my node not to pick a sponsor pool UTxO that my own in-flight anchor already spends, so that I do not race myself for the sponsor pool.
6. As an issuer, I want a double spend caused by my own node not to use up an anchor attempt, so that my credentials do not reach the failed state because of the node's own bookkeeping.
7. As an issuer, I want my credential's anchor status to say the wallet is busy with an in-flight transaction when that is why it waits, so that I know nothing is wrong.
8. As an issuer, I want an anchor batch whose transaction can never land to be released by the same rule the other features use, so that anchoring behaves consistently.
9. As a learner, I want a completion witness mint whose transaction can never land to get a replacement transaction, so that my completion witness is eventually minted.
10. As a learner, I want the replacement completion mint to carry exactly the same evidence as the original, so that a retry cannot change what the witness attests.
11. As a learner, I want my completion witness mint to wait, not fail, when the wallet has no free UTxO, so that it lands once the wallet's in-flight transactions settle.
12. As a username claimant, I want a username anchor batch whose transaction can never land to be built again, so that my username is eventually anchored.
13. As a username claimant, I want a username anchor batch to avoid UTxOs used by other in-flight transactions, so that it is not rejected.
14. As a node operator, I want recovery to stop polling the provider for transactions that can never land, so that dead transactions do not cost a request per pass forever.
15. As a node operator, I want reservations to need no extra network requests, so that the fix does not add load on any chain provider.
16. As a node operator, I want reservations to survive a restart, so that a node that crashed after submitting does not double spend when it comes back.
17. As a node operator, I want reservations to be released on their own once a transaction's validity interval has passed, so that the wallet never stays locked.
18. As a node operator, I want the node to decide expiry only when it knows the current tip, so that a provider outage never marks a transaction dead by mistake.
19. As a node operator, I want an expired transaction kept in the journal with its bytes, so that I can still see what was submitted and why it was replaced.
20. As a node operator, I want log lines that name the selected input and how many outpoints were reserved, so that I can diagnose a busy wallet.
21. As a node operator, I want a transaction that lands late but inside its validity window to be confirmed normally, so that expiry never discards a valid result.
22. As a node operator, I want a transaction whose script failed on chain to keep its outpoints reserved until its window passes, so that the collateral it consumed is never picked again too early.
23. As a node operator, I want the journal to refuse a transaction with no validity upper bound, so that every reservation has an end and every transaction can expire.
24. As a node operator running on a different chain provider later, I want reservations to depend only on the journal and not on any provider's mempool feature, so that the fix holds after the provider changes.
25. As a sponsor pool operator, I want nodes to stop spending sponsor UTxOs they already spent in flight, so that the pool sees fewer pointless rejections.
26. As a developer, I want username anchoring and completion minting to reach the chain through the `Ledger` port, so that all three features are tested against one fake ledger.
27. As a developer, I want the fake ledger to reject a transaction whose inputs or collateral conflict with an earlier accepted one, like a node's mempool, so that tests reproduce the live failure.
28. As a developer, I want the fake ledger to keep listing spent UTxOs until told to catch up, so that tests reproduce provider index lag.
29. As a developer, I want the fake ledger to let a submitted transaction never land, so that tests can drive expiry.
30. As a developer, I want one reservation query and one expiry rule in the submission journal, so that no feature keeps its own copy of either.
31. As a developer, I want every coin selector to take the reserved set as input, so that a new builder cannot forget to skip in-flight outpoints.
32. As a developer, I want the anchor queue's own expiry check replaced by the journal's expiry, so that only one expiry rule exists.
33. As a verifier, I want nothing about anchor records, receipts or the wire format to change, so that existing anchors and receipts stay valid.

## Implementation Decisions

- **Submission journal owns reservations.** It gains a query that returns the outpoints reserved at a given tip: the inputs and collateral inputs of every journaled transaction on this network whose validity upper bound plus the expiry margin is at or after that tip. The outpoints are decoded from the stored signed bytes. The journal schema is unchanged for this; the bytes already hold every input, the collateral and the validity bound.
- **Reservations ignore status.** Outcome-unknown, submitted, confirmed and failed-on-chain transactions all reserve until their window ends. Confirmed inputs are spent forever, and the provider index may lag the receipt, especially once more than one provider is in use. A script-failed transaction's regular inputs are not spent, but reserving them a little longer is harmless, and its collateral is spent.
- **Expired transactions reserve nothing.** Once the journal marks a transaction expired, its outpoints are released immediately.
- **Every journaled transaction has a validity upper bound.** The journal refuses to checkpoint signed bytes with no upper bound. All production builders already set one.
- **One expiry margin.** The margin the anchor queue uses today (1,800 slots past the validity bound) moves to the submission journal and becomes the only one. It covers provider catch-up and short rollbacks.
- **A new terminal status, `expired`.** Reconciliation takes the current tip. When the tip is known and past the transaction's validity bound plus the margin, and the provider has no receipt for it, the journal sets the status to `expired`. An unknown tip, a provider error or a timeout never expires anything. A receipt that arrives before expiry still confirms or fails the transaction as today. The status check constraint gains `expired`; the project is pre-launch, so the schema is edited in place, not migrated.
- **Replacing an expired operation.** The journal's rule that a journaled transaction is never rebuilt stays, with the exception the sponsored anchoring spec already allows: a new transaction is permitted only when the journal proves the old one can never land. The journal gets an operation that replaces an operation's transaction only when its current entry is `expired`. It keeps the expired bytes and hash as history, and checkpoints the new signed bytes atomically before sending, the same as a first submission.
- **Credential anchoring** uses the journal's `expired` status in place of its own tip comparison, and keeps its release behaviour: the batch is released, the attempt is counted, and the credentials return to the queue. Batch ids are transaction hashes, so a new batch never collides with the expired one.
- **Completion minting** rebuilds an expired mint from its preserved recovery context. The evidence (policy, course, subject key, leaves, root, mean score) must match the original exactly; only inputs, collateral, validity interval and fee change. The completion request's status follows the replacement.
- **Username anchoring** returns an expired batch's claims to pending. If the next batch has the same claims, and therefore the same operation id, it uses the replace-expired path.
- **Coin selection.** Each selector keeps its policy (largest for the issuer-paid anchor input, smallest sufficient for collateral, first sufficient for username anchoring, largest plus a separate collateral for completion minting) but filters out reserved outpoints first, through one shared helper that takes the provider's UTxO list and the reserved set. The sponsor pool selector filters reserved outpoints before its issuer-and-tip hash pick.
- **Wallet busy.** When filtering leaves no eligible UTxO, but the unfiltered list would have had one, the builder returns a distinct "wallet busy: inputs reserved by in-flight transactions" error. Each feature treats it as a wait, never as a failed attempt. Credential anchoring uses its existing unbuilt-retry delay and shows the reason as the anchor status message. Completion minting uses its existing reserved backoff. Username anchoring tries again next pass.
- **Read reservations when building.** A builder reads the reserved set in the same pass, just before selecting inputs. The background chain pass runs the features one after another, and each journals its transaction before sending, so a later feature in the same pass already sees the earlier one's inputs. The invariant: transactions spending the node wallet are built only inside the background chain pass. Any future path that builds outside it must take a per-wallet build lock first.
- **`Ledger` port reach.** Username anchoring and completion minting (builder, queue and recovery) become generic over the `Ledger` port instead of taking the Blockfrost client. The port already has every operation they need (UTxOs, protocol parameters, tip, evaluate, submit, receipt, transaction bytes); its surface does not change. The completion observer, relay registry and stake-pubkey registry reads stay on the Blockfrost client; they are read-only and belong to the research's step 3.
- **Anchor queue's identical-rebuild guard stays** as a defence. With reservations it should no longer trigger.
- **No wire or record change.** Anchor records, receipts, completion witness metadata and username anchor metadata are untouched.

## Testing Decisions

- **One seam: the `Ledger` port, with the in-memory fake ledger.** Tests drive each feature's existing background tick (credential anchor queue, completion queue and recovery, username anchoring) against a real journal on a migrated in-memory profile database. They do not test selectors or decoding helpers directly when a tick-level test can show the behaviour.
- **Fake ledger extensions:**
  - Mempool conflicts: a submission is rejected with a `BadInputsUTxO`-style error when any of its inputs or collateral inputs is an input of an earlier accepted submission.
  - Lagging index: UTxO listings keep returning spent UTxOs until the test calls a catch-up method that drops spent outpoints and adds outputs of included transactions.
  - Never-land: a submitted transaction can be marked dropped, so it is never included and receipts stay empty.
- **A good test asserts external behaviour:** what was submitted, which outpoints the submitted bytes spend, the journal status, and the user-visible status of credentials, completion requests and username claims. Never internal call order.
- **Required scenarios:**
  - A completion mint and then an issuer-paid anchor in the same pass, behind a lagging index: both are accepted and spend different UTxOs.
  - A sponsor-paid anchor's collateral avoids an outpoint that a pending completion mint spends.
  - The sponsor pool selector skips a sponsor UTxO this node's pending anchor spends.
  - With every eligible UTxO reserved: no submission, a wallet-busy reason, no attempt counted. After the window passes or the index catches up, the next pass submits.
  - Reservations survive a restart: a fresh journal over the same database still reserves the earlier transaction's outpoints.
  - Expiry: a dropped transaction becomes `expired` only after the tip passes its bound plus the margin, and never when the tip is unavailable. Its outpoints are released.
  - A receipt arriving inside the margin confirms the transaction; it does not expire.
  - A completion mint rejected as a double spend is expired, then replaced with identical evidence and new inputs. A replacement with different evidence is refused.
  - An expired username batch with the same claims replaces its journal entry; the expired bytes stay readable as history.
  - An expired anchor batch is released through the journal's status, counting one attempt, as today.
  - The journal refuses to checkpoint a transaction with no validity upper bound.
- **Prior art:** the anchor queue tests built on the fake ledger (funding helpers, ticking at a fixed tip, sponsor fallback); the submission journal tests (crash between checkpoint and send, outcome-unknown recovery, receipt reconciliation); the completion queue and recovery test files; the username anchoring timed-out-batch test. That test, and other transport-level cases that rely on the loopback HTTP fake chain, keep using it.

## Out of Scope

- Spending the node's own pending change outputs (transaction chaining). With reservations, a wallet with few ADA-only UTxOs can run only a few writes per validity window. Chaining, or splitting the wallet into more UTxOs, is a follow-up if that limit shows up in practice.
- Contention between different nodes for sponsor pool UTxOs. A node's journal knows only its own transactions. Expiry and the existing issuer-paid fallback recover from cross-node collisions; preventing them needs a fresher chain view.
- Provider endpoints in the network profile, failover, keyless backends, local script evaluation, direct relay submission and Mithril proofs (research steps 2 to 8).
- Moving the network name used by the journal and anchor queue from a constant into the network profile (research step 2).
- Routing the completion observer, relay registry, stake-pubkey registry, example programs and operator scripts through the `Ledger` port (research step 3).
- Typed `Ledger` errors. This spec recognizes "wallet busy" before submitting and does not need to classify provider errors.

## Further Notes

- The Blockfrost mempool endpoint is not a substitute: it lists only transactions submitted through Blockfrost and exists only on the hosted service. The journal is the source of truth that holds for every provider.
- Preprod slots are one second long, and builders set a validity window of about an hour. A dead transaction is therefore expired, and its inputs freed, about 90 minutes after it was built. That is also the longest a wallet-busy wait can last when no transaction lands.
- Accepted limitation: if a provider has no record of a transaction even though it is on chain, and stays wrong past the margin, the journal would expire a transaction that actually landed, and the replacement would be rejected as a double spend. The margin and the requirement for a known tip make this unlikely. Mithril inclusion proofs (research step 7) would remove the dependence on the provider.
