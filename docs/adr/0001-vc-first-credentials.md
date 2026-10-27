# 0001. Verifiable Credentials replace SkillProofs

Status: implemented
Date: 2026-04-24 (credential format amended 2026-10-09)

## Context

The first credential pipeline turned `evidence_records` into `skill_assessments`, aggregated them into `skill_proofs`, and optionally minted each proof as a Cardano NFT. It had its own evidence gossip topic and an aggregator module. In practice this meant:

- a second, Alexandria-specific credential format next to the W3C Verifiable Credentials layer the protocol spec already defined;
- an on-chain write for every proof, so credentials depended on Cardano being reachable and funded;
- reputation computed from evidence rows, not from anything a third party could verify.

Credentials must still verify if Alexandria stops existing (protocol spec §14.20), and must be checkable by ordinary VC tooling.

## Decision

Credentials are W3C Verifiable Credentials, and only W3C Verifiable Credentials.

| Aspect | Choice |
|---|---|
| Data model | VC Data Model 2.0 (`https://www.w3.org/ns/credentials/v2`) |
| Proof | `DataIntegrityProof`, cryptosuite `eddsa-jcs-2022` (Ed25519 over JCS-canonicalised JSON) |
| Subject and issuer identity | `did:key` (Ed25519), derived from the profile's BIP-39 mnemonic |
| Revocation and suspension | Bitstring Status List v1.0: bit *n* at byte `n / 8`, most significant bit first; lists of at least 131,072 bits, GZIP-compressed and multibase base64url-encoded |
| Status-list location | Dereferenceable at `{origin}/status-lists/{issuer}/{n}`, served by the identity's configured status host (Alexandria Cloud by default). With no host, the list is named by `urn:` and travels in exported bundles |
| Verification result | `accept`, `pending` or `reject`. A credential whose status list or issuer key is missing is `pending`, never accepted |
| Course completion | Self-issued locally at claim time from persisted graded submissions. An on-chain completion witness (`completion.ak`) is optional and never a gate |
| Reputation | Computed from verified credentials (`evidence::reputation::on_credential_accepted`) |

Cardano is limited to VC integrity anchoring (metadata-only transactions, label 1697, carrying a credential hash), the optional completion witness, and DAO governance.

The verifier lives in the I/O-free `crates/alexandria-verify` crate, published separately, so integrators verify credentials without the app.

Deleted: the `skill_proofs`, `skill_proof_evidence`, `evidence_records` and `skill_assessments` tables; the evidence aggregator; the `/alexandria/evidence/1.0` topic and handler; the `mint_skill_proof_nft` and `register_course_onchain` commands and their transaction builders.

## Consequences

- One credential format, verifiable offline and by any implementation of the W3C specs.
- Issuing a credential needs no network or ledger access. Revocation needs a reachable status list, so a bare credential with a status entry stays `pending` until its list is fetched or bundled.
- The original 2026-04-24 cutover signed credentials with a JWS. The 2026-10-09 amendment replaced it with Data Integrity `eddsa-jcs-2022` and made status lists dereferenceable. Pre-launch data was disposable, so there was no migration.
- Anything that used to read SkillProofs or evidence rows (governance and opinion gating, reputation, endorsements) reads credentials instead.
- Who may change a credential's status is a separate decision: [0003](0003-credential-authority.md).
