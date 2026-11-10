# Sponsored, batched credential anchoring

Status: active
Updated: 2026-11-10
Triage: ready-for-agent
Repos: alexandria

## Problem Statement

A credential issued in Alexandria should carry proof that it existed at a point in time. The proof is an integrity anchor on Cardano (protocol spec §11.3.1, §14.12.3). Today a node anchors each credential in its own transaction. The fee comes from one of two places:

- **The treasury**, by signing with a treasury key read from a local `.env`. This only works on a developer machine. A shipped app can't contain the key, because anyone who extracts it can drain the treasury. Holding the key in a hosted service instead would add the central server every Alexandria node is meant to work without.
- **The issuer's own wallet**, otherwise. Every learner would need ADA before their credentials could be anchored, and pay one fee per credential.

A credential that reaches someone else carries nothing that lets their verifier find its anchor. So received and exported credentials always show as "not anchored", even when the issuer anchored them.

The result is that anchoring works in development, fails in release builds, and never reaches the people who verify credentials.

## Solution

Anchoring keeps working in a shipped app without a server, a shipped secret, or a fee per credential.

- **Batching.** Each issuer's node collects the integrity hashes of the credentials it issued. It anchors one Merkle root over them per batch, in a single transaction.
- **Receipts.** Each credential gets an anchor receipt: its position in the batch, the sibling hashes up to the root, and the anchoring transaction. The receipt travels with the credential in exported bundles. Any verifier can then check the anchor by recomputing the root and looking up one transaction.
- **Sponsor script.** The treasury's funds sit at a Cardano script address (the anchor sponsor) instead of behind a key. Any node may spend a sponsor UTxO to pay its anchor fee, but only within rules the script enforces on-chain:
  - a fee cap;
  - the remaining value returns to the script;
  - a rate limit per UTxO.

  The issuer still signs every anchor, as today.
- **Fallback.** When no sponsor UTxO is available, the node falls back to funding the anchor from the issuer's wallet, as it does now.

Learners see the same anchor badge as now. "Pending" may now last until the next batch. Received and exported credentials can show "anchored" once their receipt verifies.

## User Stories

1. As a learner, I want my completion credentials anchored without holding ADA, so that I get tamper-evident timestamps for free.
2. As a learner, I want my credentials anchored even when Alexandria's company and servers are gone, so that my record doesn't depend on any organisation.
3. As a learner, I want to see that a new credential is waiting for the next batch, so that I know "pending" is expected and not an error.
4. As a learner, I want to see when the batch holding my credential is confirmed on-chain, so that I know the timestamp is final.
5. As a learner, I want a link from a credential to its anchoring transaction on a block explorer, so that I can check it myself.
6. As a learner, I want a failed anchor to tell me why in plain words, so that I can fix it, for example by adding a small balance for collateral.
7. As a learner, I want to retry a failed anchor, so that a temporary chain or network problem doesn't leave my credential unanchored for good.
8. As a learner, I want my exported credential bundle to include the anchor receipts, so that an employer can verify the anchors offline from the bundle.
9. As a learner, I want credentials I receive from another issuer to show as anchored when their receipt verifies, so that I can see which credentials I hold are anchored.
10. As a learner, I want anchoring to never block issuance or verification, so that I can keep using my credentials while the chain is unreachable.
11. As an instructor who issues credentials, I want my node to batch everything I issue, so that one transaction covers many credentials.
12. As an issuer, I want to be a required signer on every anchor of my credentials, so that nobody else can claim to have anchored them on my behalf.
13. As an issuer, I want my node to anchor only the credentials I issued, so that holders can't anchor content under my name.
14. As an issuer, I want an anchor already submitted to never be rebuilt or resubmitted as a different transaction, so that a credential never ends up with two conflicting anchors.
15. As an issuer, I want my node to use the sponsor automatically when it can and my wallet only when it must, so that I rarely pay.
16. As an issuer, I want my node to skip sponsor UTxOs that are rate-limited and try another, so that nodes anchoring at the same time don't keep failing.
17. As an employer verifying a credential, I want the verifier to recompute the credential's integrity hash and fold its receipt up to the anchored root, so that I don't have to trust the holder's claim that it's anchored.
18. As an employer, I want an anchor whose root, issuer or signer doesn't match to be treated as "not anchored", so that a forged or borrowed receipt proves nothing.
19. As an employer, I want a credential with no receipt to still verify by signature, with "anchored" left false, so that anchoring stays an extra assurance and not a gate.
20. As an employer whose policy requires anchors, I want a credential whose anchor lookup failed to come back pending rather than rejected, so that a chain outage doesn't make me reject valid credentials.
21. As an integrator using `alexandria-verify`, I want receipt checking inside the I/O-free crate, so that I get identical anchor semantics without depending on the app.
22. As an integrator, I want the Merkle hashing rules written down exactly, so that I can implement receipt checking in another language.
23. As an Alexandria community member, I want the treasury to fund anchors through a public on-chain rule rather than a private key, so that nobody has to trust whoever holds the key.
24. As a community member, I want the most the sponsor can lose per day to be fixed by its parameters, so that a malicious node can only waste a capped amount.
25. As a community member, I want anyone to be able to top up the sponsor by sending ADA to its address, so that funding it needs nobody's permission.
26. As a founder, I want a multisig of founder keys to be able to reclaim or reconfigure the sponsor's funds, so that a bad parameter choice or a validator bug can be recovered from before on-chain governance is active.
27. As an operator, I want the sponsor's script hash, address, reference-script UTxO and parameters published in the network profile, so that every node reads the same values and none are hard-coded.
28. As a developer, I want the treasury-key anchor path deleted, so that no build can ever read a treasury secret for anchoring.
29. As a developer, I want the anchor queue to talk to the chain through a `Ledger` port, so that I can test batching, sponsor selection and fallback against a fake ledger, and later swap Blockfrost for direct relay submission.
30. As a developer, I want the anchor badge and the verifier to read receipts from one store, so that the UI and verification can't disagree.
31. As a learner with several profiles on one device, I want each profile's credentials batched under that profile's own issuer identity, so that one profile's anchors never name another profile.

## Implementation Decisions

### Anchor record (label 1697, version 2)

- The anchor stays a label-1697 transaction metadata record. Version 2 replaces the per-credential hash with a batch root: `{v: 2, root, leaf_count, issuer_did, anchored_from}`. `anchored_from` is the earliest issuance time in the batch.
- The protocol spec's `A(c) = (H(c), t_i, issuer_ref)` is generalised so the anchor commits to `H(c)` through a Merkle root. A batch of one is valid.
- Version 1 anchors are dropped without migration (pre-launch, disposable data).
- The issuer's payment key, which is also its `did:key` signing key, MUST be a required signer, however the transaction is funded. Verifiers check this, not just the `issuer_did` text in the metadata.
- The anchor record lives only in metadata, for both funding modes. The sponsor validator never inspects it, because Plutus scripts can't see metadata. That also means sponsored and self-funded anchors verify the same way.

### Merkle tree and receipt

- **Leaves:** the hex-decoded `H(c)` values. `H(c)` is BLAKE3 over the JCS-canonical credential with `proofValue` emptied, unchanged from today.
- **Hashing:** BLAKE3 with domain separation in the RFC 6962 style:
  - a leaf hashes as `BLAKE3(0x00 ‖ H(c))`;
  - an inner node hashes as `BLAKE3(0x01 ‖ left ‖ right)`.
- **Tree shape:** an odd node at a level moves up to the next level unchanged (RFC 6962 split). Leaves are ordered by issuance time, then by credential id.
- **Receipt shape:** `{v: 1, leaf_index, leaf_count, path: [hex], root, tx_hash, issuer_did}`. A receipt is a sidecar and is never embedded in the credential, so the credential's signature is unaffected.
- The spec gets a normative subsection with test vectors, so other implementations can match it.

### `alexandria-verify` (minor version bump, breaking)

- `IntegrityAnchor` becomes the ledger observation of one anchor transaction:
  - `root`, `leaf_count`, `issuer_did` and `tx_hash`;
  - `required_signers` (hex key hashes);
  - `anchored_at`.
- A new `AnchorReceipt` type and a pure `receipt_root(leaf_hash, receipt)` function.
- `VerificationStore::integrity_anchor(credential_hash)` returns the receipt together with the observation. The default stays "missing".
- The verifier accepts the anchor only when all of these hold:
  - folding the receipt path from `H(c)` reproduces `root`;
  - the receipt's `root` and `tx_hash` match the observation;
  - `issuer_did` equals the credential issuer;
  - the issuer's Ed25519 key hash (blake2b-224 of the `did:key` public key) is among `required_signers`.
- Pending versus reject is unchanged from #304: `IntegrityAnchorUnavailable` is pending only when the policy requires an anchor. A mismatching anchor makes `anchored` false. On its own it never causes a rejection.

### `Ledger` port (node)

- A new trait owned by the anchoring module, with the operations anchoring needs:
  - fetch UTxOs at an address;
  - fetch protocol parameters and the tip;
  - evaluate execution units;
  - submit a transaction;
  - read an anchor transaction as an `IntegrityAnchor` (its metadata, required signers and confirmation).
- Production implementation: the existing Blockfrost client. Tests use an in-memory fake that holds UTxOs, records submissions, and confirms on demand.
- No other module moves onto the port in this spec.

### Anchor queue becomes a batch anchorer

- **Enqueue** keeps today's entry points and rules:
  - issuance, auto-issuance, snapshots and retry call `enqueue`;
  - only credentials whose issuer is a local identity are queued;
  - credentials are grouped per issuer DID.
- **Flush trigger.** A batch flushes when its oldest queued credential has waited longer than the batch window, or when the queue reaches the batch-size cap. Defaults are 6 hours and 256 credentials, both constants in the module.
- **One journal entry per batch.** A flush builds and journals one transaction for the batch. The submission journal's rule still holds: a journaled transaction is never rebuilt. Retry resubmits the same bytes, or starts a new batch only when the journal proves the old transaction can never land.
- **Statuses.** Per-credential status (`pending`, `submitted`, `confirmed`, `failed`) is derived from the credential's batch.
- **Receipts.** Receipts are computed at flush time and stored per credential once the batch confirms.
- **Funding order:**
  1. A sponsor UTxO that is not rate-limited, picked at random among the eligible ones to spread contention.
  2. Otherwise the issuer's own wallet, using today's metadata-only transaction.
  3. Otherwise the batch stays queued, with the reason recorded and shown to the user.
- **Collateral.** A sponsored transaction is a Plutus spend, so it needs collateral from the submitter's own key-locked UTxO. The node evaluates the script locally before submitting, so collateral is only at risk when evaluation and the ledger disagree. A node with no suitable collateral UTxO uses step 2 or 3 above.
- **Schema.** A new batch table holds the issuer, root, leaf count, journal id, transaction hash, status and timestamps. Credential anchor rows gain the batch id, the leaf index and the stored receipt. Data is replaced rather than migrated.
- **Treasury key path.** The treasury payer is no longer used for anchors. Other transaction builders keep it until they get their own spec.

### Anchor sponsor validator (Aiken, Plutus V3)

- A new spending validator next to the governance validators. It is parameterised by the fee cap, the rate-limit window, the maximum validity-interval width, and the reclaim multisig (an m-of-n of founder key hashes).
- **Datum:** `{next_allowed: POSIXTime}`.
- **Redeemers:** `Sponsor` and `Reclaim`.
- **`Sponsor` passes only if all of these hold:**
  - exactly one input comes from this script;
  - exactly one output goes back to this script's address, with no other assets added, and the lovelace leaving the script is at most the fee cap and at most the transaction's fee;
  - the transaction's validity interval has finite bounds, is no wider than the maximum width, and its lower bound is at or after the input datum's `next_allowed`;
  - the output datum's `next_allowed` equals that lower bound plus the window;
  - at least one key signed the transaction.
- **Why the rate limit holds:** the window must be longer than the maximum validity width. A node can therefore never back-date the lower bound far enough to spend the same UTxO twice in one window.
- **`Reclaim`** passes with m-of-n reclaim signatures and no other conditions. It is used to withdraw, re-split, or move funds to a redeployed validator.
- **Worst-case drain** is the number of pool UTxOs × windows per day × the fee cap. Fees go to the chain, not to whoever spends, so abuse can only waste the treasury's money; nobody profits from it.
- **Deployment:**
  - deploy as a CIP-33 reference script, as the other validators are;
  - split the pool into N UTxOs;
  - publish the script hash, address, reference UTxO and parameters in the network profile, and bump its revision.

### UI

- The anchor badge and detail section from #304 stay. Copy changes:
  - "pending" says the credential is waiting for the next batch;
  - "failed" shows the stored reason, including "add a small tADA balance to use as collateral";
  - "confirmed" links to the batch transaction.
- Received and imported credentials show anchor state from their verified receipt, instead of always showing none.
- All copy goes through i18n in all nine locales.

### Bundles

- Signed credential bundles gain `anchor_receipts`, next to `status_lists`.
- Offline bundle verification passes the receipts to the verifier. A ledger observation comes from the `Ledger` port when one is available, and is otherwise unavailable.

### Docs (in the same change set)

- Protocol spec: §11.3.1 (sponsor, batching, required signer), §14.12.3 (root form plus a normative Merkle and receipt subsection) and §14.13 (receipt checks).
- Architecture doc: anchoring section.
- `alexandria-verify` README.
- Governance validators README: the sponsor row.
- New ADR, "Treasury-sponsored batched anchoring".

## Testing Decisions

Good tests here check behaviour through a module's public interface: given credentials, a ledger state and a policy, what is anchored, what is stored, and what the verifier concludes. They don't assert internal function calls, SQL text or transaction byte layouts, except where the bytes are the contract (Merkle test vectors).

Four seams, all agreed with the maintainer:

1. **`alexandria-verify` through `verify()` and `VerificationStore`.**
   - Fixture stores return a receipt and observation.
   - Cases:
     - a valid receipt marks the credential anchored;
     - a wrong path, a wrong root, another issuer, or a missing required signer each leave it un-anchored;
     - a credential with no receipt still verifies by signature;
     - when the policy requires an anchor, an unavailable lookup is pending;
     - Merkle test vectors for 1, 2, 3, 5 and 256 leaves.
   - Prior art: the integrity-anchor tests added in #304 in the verify module, and the `alexandria-verify` fixture stores.
2. **Anchor queue through its public functions, against the fake `Ledger`.**
   - Cases:
     - credentials batch per issuer;
     - flush happens on the window and on the size cap;
     - sponsor UTxOs are picked and rate-limited ones skipped;
     - funding falls back to the wallet, then stays queued with a reason;
     - an outcome-unknown submission is resubmitted, never rebuilt;
     - receipts are stored on confirmation, and the listing state matches what the store hands the verifier.
   - Prior art: the existing anchor queue tests (`state_reads_only_credentials_this_node_queued`), the submission journal tests, and in-memory `Database` test setup.
3. **Sponsor validator via `aiken check`.**
   - Cases: an accepting `Sponsor` spend; rejection when over the fee cap, with no continuing output, with an extra script input, when spent too early, with a back-dated lower bound, with too wide an interval, with a wrong `next_allowed`, or with assets added; `Reclaim` with and without quorum.
   - Prior art: the governance validators' inline Aiken tests.
4. **Offline bundle verification.**
   - Cases: an exported bundle round-trips its receipts; a tampered receipt leaves the credential un-anchored; a bundle without receipts verifies as before.
   - Prior art: the bundle export and verification tests (`export_bundle_includes_credentials_and_status_lists`).

There are also frontend unit tests for the badge and detail copy (prior art: the credential detail component tests and the anchor-badge mapping). An ignored live preprod test submits a sponsored batch, like `live_treasury_anchor_preprod`, which it replaces.

## Out of Scope

- **Replacing Blockfrost.** Submitting through `pallas-network` to public Cardano relays, and reading anchors with Mithril-certified transaction proofs, are a follow-up spec. The `Ledger` port exists so that swap can be made without touching this design.
- **Governance control of sponsor parameters.** Until committee outcome certificates are active, parameters change only by founder-multisig reclaim and redeploy.
- **Eligibility gating.** Limiting sponsor use to registered issuers (an identity token, or a DAO-maintained allowlist). The fee cap and rate limit bound abuse instead.
- **Sharing one root across nodes.** Nodes gossiping hashes so that one elected node anchors for many.
- **Pushing receipts from issuer to holder** over P2P for credentials issued to another subject. In this spec, receipts reach holders through bundles. Self-issued completion credentials, where issuer and holder are the same node, need nothing more.
- **Funding username registration and other transaction builders** that still use the treasury key or the learner's wallet.
- **Covering collateral for nodes with no ADA**, for example through an onboarding grant. This waits on a sybil-resistance decision.
- **Mainnet parameters.** This spec targets preprod.

## Further Notes

- This replaces the treasury-key anchoring in #304. If #304 merges first, this spec deletes that path. The `alexandria-verify` bump in #304 has not been published, so it can be folded into this one.
- Deploying and funding the sponsor needs the preprod deployer key and the founder reclaim keys. That step is for a human holding those keys. Everything before it can be built and tested against the fake ledger and `aiken check`.
- A sponsored transaction costs more than a metadata-only one because it runs a script: roughly 0.3–0.6 tADA against 0.17–0.2. Batching makes the cost per credential negligible, but parameters should be sized from measured fees, not these estimates.
- Third-party fee payment on Cardano (CIP-118 nested transactions, "Babel fees") would remove the collateral requirement if it ships. The design doesn't depend on it.

## Notes

### 2026-11-10: implemented, sponsor not yet deployed

Built as specified, with one correction. The wallet's `did:key` is not its Cardano payment key (the did:key is derived from the extended key's first 32 bytes as an Ed25519 seed). So the anchor's required signer is the `did:key` key itself, witnessing alongside the payment key that pays or lends collateral. A verifier can then check the signer from the DID alone.

Still open:

- **Deploy the sponsor.** Build `anchor_sponsor` with the pinned Aiken compiler; v1.1.24 changes every existing validator's hash, so its blueprint was not committed. Apply the parameters, deploy the reference script, split and fund the pool, then add `anchor_sponsor` to the network profile. This needs the deployer and founder keys.
- **Live preprod run.** `live_anchor_preprod` (ignored) needs a funded test mnemonic, and a deployed sponsor for the sponsored path.
- Protocol spec §3.1 still says the payment key is the `did:key` signing material; the implementation differs (see above). That predates this spec.

### 2026-11-10: review fixes

- The sponsor's first draft let a spender keep the difference between the fee cap and the real fee. It now bounds the lovelace leaving the script by the transaction's fee too.
- A batch that cannot be built or paid for stays queued with its reason, without spending an attempt. Only failures on chain and expiry count.
- A credential may hold several receipts; a wrong imported one never displaces the right one. Unconfirmed lookups back off, so they cannot starve the rest.
- Collateral risks the smallest sufficient UTxO.
- The slot-to-POSIX offset comes from the network profile (`anchor_sponsor.slot_offset_secs`).
