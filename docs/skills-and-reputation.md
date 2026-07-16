# Skills & Reputation

> Open skill graph, evidence-based credentials, and reputation system.

> **⚠️ Post-VC-first cutover (migration 040, 2026-04-24):** Every
> reference to `skill_proofs`, `evidence_records`, or
> `skill_assessments` in this doc describes the retired pipeline.
> Credentials are now W3C VCs auto-earned via a Cardano completion
> validator. Reputation computation has been repointed at credentials
> (distribution math unchanged). See [`vc-migration.md`](./vc-migration.md).

**Status:** Draft
**Audience:** Platform builders, education providers, governments, recruitment services, LLM-based tooling

---

## Abstract

This document specifies a machine-consumable framework for skill-based education, assessment, credentialing,
reputation, and hiring. It defines an open skill graph, a standardized evidence model, learner-owned credentials,
a verifiable reputation system, and a query model designed to replace proxy-based signals such as degrees and resumes.

---

## 1. Motivation

### 1.1 Education Failure Modes
- Low signal from credentials
- Opaque learning pathways
- Weak feedback loops

### 1.2 Recruitment Failure Modes
- Reliance on proxies
- Resume inflation
- Exclusion of capable but non-credentialed candidates

---

## 2. Design Goals (Normative)

The system MUST:
- Represent skills as atomic, assessable entities
- Produce verifiable, portable evidence of skill
- Support learner-owned credentials
- Be interoperable with ESCO and O*NET
- Be consumable by LLMs without ambiguity

---

## 3. Core Data Model

### 3.1 SubjectField

Top-level knowledge domains (e.g., Computer Science, Mathematics). Stored in the `subject_fields` table with optional `icon_emoji` for display.

### 3.2 Subject

Subdivisions of subject fields (e.g., Algorithms, Data Structures). FK to `subject_fields`.

### 3.3 Skill

Atomic, assessable competencies within a subject. Each skill has a `name`, `description`, and `bloom_level`. Skills are stored in the `skills` table with FK to `subjects`.

### 3.4 ProficiencyLevel

Ordered enumeration based on Bloom's revised taxonomy: `remember`, `understand`, `apply`, `analyze`, `evaluate`, `create`.

---

## 4. Skill Graph (Normative)

Skills form a directed acyclic graph (DAG) with explicit prerequisite edges stored in `skill_prerequisites` (composite PK: `skill_id`, `prerequisite_id`). Non-prerequisite relationships (e.g., "related to", "builds on") are stored in `skill_relations`.

Taxonomy updates are committee-gated via the governance system and propagated over the `/alexandria/taxonomy/1.0` GossipSub topic.

---

## 5. Content and Assessment Tagging

Course elements (`course_elements`) are tagged with skills via `element_skill_tags`, linking each content element to specific skills with a per-tag `weight`. This enables skill-specific progress tracking and evidence generation.

---

## 6. Evidence Model

The verifiable outcome of an assessment is now a **W3C Verifiable Credential** (see [`vc-migration.md`](./vc-migration.md) and `domain::vc`), not an `evidence_records` row. There are three issuance paths:

1. **Course completion** — `claim_course_completion` assembles completion leaves from the learner's graded `element_submissions`, verifies them against the course template (gradeable elements in order, ≥0.6 pass), computes a Merkle root, and **self-issues the credential locally at claim time**. A Cardano completion-witness mint is a best-effort on-chain *anchor* (treasury-funded when configured, learner still signs), not a hard requirement — the local observer path remains as a secondary confirmation. Courses with no gradeable elements issue a **content-only** credential at a baseline proficiency (`CONTENT_COMPLETION_SCORE = 0.3`) with a deterministic completion root and no on-chain witness. `get_course_completion_status` reports the still-unmet gradeable elements, driving the "why no credential yet" surface.
2. **Document bootstrap** — skills confirmed from an uploaded resume / transcript are self-issued as `SelfAssertion` credentials carrying a provenance tier (see §6.1).
3. **Assessment** — passing a dynamic, Sentinel-gated question-bank attempt issues an `AssessmentCredential` bound to the integrity session (see §6.2).

Each credential binds:
- subject DID, skill / proficiency scope
- the course, submissions, or assessment attempt it derives from
- the issuer DID and an Ed25519 detached-JWS signature

There is no `/alexandria/evidence/1.0` topic (it was removed in migration 040). Credentials cross the network via the `vc-did`, `vc-status`, and `vc-fetch/1.0` protocols.

### 6.1 Provenance tiers

A `SkillClaim` may carry an optional `ProvenanceTier` (migration 068) that grades how the evidence was obtained, feeding the aggregation quality weight (§ below and `aggregation/config.rs`):

| Tier | Meaning | Example |
|------|---------|---------|
| `self_declared` | Unbacked self-claim | manual skill selection |
| `document_backed` | Backed by a self-authored document | a resume |
| `accredited_document` | Backed by an accredited institution's document | a university transcript |
| `issuer_signed` | Issued by a third party (issuer ≠ subject) | a formal credential |

Higher tiers carry more aggregation confidence. A `None` provenance reproduces the pre-068 behavior exactly (quality triple `(1,1,1)`), so the `calculation_version` bump `1.0 → 1.1` is the only observable change for legacy claims.

### 6.2 Dynamic assessments

Community-contributed, DAO-ratified **question banks** (migration 070; see [`protocol-specification.md`](./protocol-specification.md) §8.4) verify claimed skills:

- `assessment_start_attempt` draws a randomized, difficulty-stratified subset (per-attempt seed) and shuffles options; the answer key (`bank_questions.correct_indices`) is **never** included in the returned questions.
- Sentinel auto-activates for every attempt (the learner is told), binding the attempt to an integrity session.
- `assessment_grade` grades **host-side** against the locally-held key and, on pass, issues an `AssessmentCredential` bound to that integrity session, then recomputes derived skill states.

---

## 7. Credentials

Credentials are W3C Verifiable Credentials stored in the `credentials` table (the `skill_proofs` / `skill_proof_evidence` aggregation tables were dropped in migration 040):
- Subject- and skill-scoped, signed with Ed25519Signature2020 (detached JWS over RFC 8785 JCS bytes)
- Lifecycle (issue / suspend / reinstate / revoke) tracked via a RevocationList2020 status list (`credential_status_lists`)
- Credential **integrity** is anchored on Cardano with a metadata-only transaction (label 1697) that timestamps the canonical VC hash — no NFT mint, no on-chain credential content

The legacy native-script SkillProof NFT mint (CIP-25 metadata) was retired in migration 040. A CIP-68 soulbound **reputation snapshot** path exists (`submit_snapshot_tx`); its reference scripts (`REPUTATION_MINTING_REF_UTXO` / `SOULBOUND_REF_UTXO`) are deployed on preprod, so the mint is live.

---

## 8. Learner Ownership Model

All credentials are stored locally in the learner's SQLite database and iroh content store. The learner controls:
- Which credentials to allow other peers to fetch (`allow_credential_fetch` / `disallow_credential_fetch` over the `vc-fetch/1.0` protocol)
- Which credentials to disclose via selective-disclosure presentations (§18)
- Cross-device sync of learning state via explicit device pairing (AES-256-GCM-sealed `/alexandria/sync/1.0`)

No server holds or controls credential data.

---

## 9. Public-Interest Extensions

The framework supports:
- **Education**: Portable credentials across institutions
- **Recruitment**: Skill-based filtering with verifiable evidence
- **Government**: Workforce visibility and credential portability

---

## 10. LLM Compatibility (Normative)

Schemas are designed as authoritative inputs for LLM reasoning:
- Atomic skills with explicit definitions
- Structured proficiency levels (Bloom's taxonomy)
- Machine-readable evidence with confidence scores
- DAG-structured prerequisites

---

## 11. Security and Trust Considerations

- All credentials are Ed25519 signed by the issuer DID's key
- A **credential** can be challenged via stake-based challenges (5 ADA staked at the `challenge_escrow.ak` validator, 2/3 supermajority vote); on uphold the credential is revoked via its RevocationList2020 status list
- Multi-party completion-attestation requirements for high-stakes courses (`commands::attestation`)
- Behavioral integrity scores from the Sentinel anti-cheat system feed the trust signal on flagged assessments
- Identity binding via the persistent `stake_pubkey_registry` in the P2P validation pipeline (see [`docs/stake-pubkey-registry.md`](./stake-pubkey-registry.md))

---

## 12. Reputation System (Normative)

Reputation is a computed, skill-scoped, evidence-derived view. There are no global scores.

### 12.1 Purpose

Reputation evaluates instructional, assessment, or authorship impact within a specific scope.

### 12.2 Design Principles
- Evidence-derived only — no manual ratings
- Skill- and role-scoped — always `(subject, role, skill, proficiency_level)`
- Distribution-based with confidence bounds — no single number

### 12.3 ReputationAssertion

```
ReputationAssertion {
  actor_address       -- The actor being evaluated
  role                -- instructor / assessor / author / learner / mentor
  skill_id            -- Skill scope (nullable for broad role assertions)
  proficiency_level   -- Level scope
  score               -- Aggregated reputation score
  evidence_count      -- Supporting evidence count
  median_impact       -- Distribution median
  impact_p25/p75      -- Distribution bounds
  learner_count       -- Sample count
}
```

Stored in `reputation_assertions`, computed directly from the subject's non-revoked `credentials` (the `reputation_evidence` table was dropped in migration 040). The `median_impact`, `impact_p25`, `impact_p75`, `impact_variance`, and `learner_count` columns are persisted; a sample-size confidence (`learner_count / (learner_count + 5)`) is derived on read by `commands::reputation::get_reputation`.

### 12.4 Instructor Impact

```
Impact(I, S, P) =
  Σ learners [ ΔConfidence × Attribution ]
```

Computed in `evidence/reputation.rs` directly from the subject's non-revoked `credentials` (the `reputation_impact_deltas` table was dropped in migration 040 — there is no per-evidence delta store).

Reputation snapshots can be anchored on-chain as CIP-68 soulbound tokens with CBOR-encoded datums (`reputation_snapshots` table); the reference scripts (`REPUTATION_MINTING_REF_UTXO` / `SOULBOUND_REF_UTXO`) are deployed on preprod, so the mint is live.

---

## 13. Query & Consumption Model (Normative)

### 13.1 Skill Queries

```
SkillQuery {
  skill_id
  minimum_proficiency
  minimum_confidence?
}
```

### 13.2 Reputation-Aware Queries

```
ReputationConstraint {
  role
  skill_id
  proficiency_level
}
```

### 13.3 Consumption Rules
- Thresholds, not rankings
- Skill-first reasoning
- Explainable outputs

---

## 14. Public Skill Graphs & Learning Targets (Normative)

The skill graph and reputation are the core surface of the app — they
are promoted to the home screen and made shareable + actionable.

### 14.1 Owner graph & visibility

A user's skill graph is every skill they hold a non-revoked credential
for (the "earned" set, `credentials WHERE subject_did = self AND
skill_id IS NOT NULL AND revoked = 0`), wired together by the global
`skill_prerequisites` taxonomy edges among that set.

Each earned skill carries two owner-controlled flags, stored in the
**synced** `instructor.graph_prefs` setting
(`{ "<skill_id>": { "public": bool, "teaching": bool } }`):

- `public` — whether the skill is exposed to other peers. **Earned
  skills are public by default**; the owner flips individual skills
  private.
- `teaching` — a highlight marking skills the owner opts to instruct.
  Defaults `false`; orthogonal to `public`.

Editor: Skills → *My Graph* tab (`GraphVisibilityEditor.vue`), per-skill
toggles + bulk show/hide.

### 14.2 Viewing another user's graph (P2P)

Public graphs are fetched over the `/alexandria/graph-fetch/1.0`
request-response protocol (CBOR, §protocol-spec 6.5). The wire payload
(`PublicSkillGraph`) carries only public nodes; private skills never
leave the device. `teaching` nodes are flagged for highlight.

Because there is no DID→PeerId registry, `fetch_public_graph(did)`
broadcasts to the requester's connected peers and returns the first
peer that owns the DID; a self-DID request is served locally
(loopback), which also powers "preview my public graph". Handler:
`p2p::graph_fetch::handle_graph_fetch_request` (answers `NotOwner`
unless `identity.local_did` matches the requested `subject_did`).

UI: `/u/:id` (accepts a DID or @username) (`InstructorGraph.vue`) renders the DAG + a "Teaches"
list and a **Target this graph** action.

### 14.3 Targets & learning paths

A user may target any skill graph — an instructor's whole public graph,
or a single skill (rooted subtree). Goals are stored in the **synced**
`learner.targets` setting (array of `{ id, label, source_did?,
goal_skill_ids, kind?, source_key?, source_url?, resolution_provenance?,
taxonomy_version?, created_at }`); a user may hold many.

Beyond hand-picking skills, a goal can be **resolved** (`resolve_goal`,
`commands::goal_templates`) from a DAO-ratified **goal template** — a
nationalized exam, a K-12 board-grade curriculum, or a job role
(migration 069, seeded genesis set) — or from a job description: a public
JD link is fetched and stripped to text, or pasted text is parsed
on-device (`goals::jd_parser`) against skill names + `skills.synonyms`,
producing skill suggestions the user confirms. The resolved
`goal_skill_ids` then feed the same path pipeline below.

`compute_learning_path(goal_skill_ids)` (`commands::graph::compute_path`)
computes, against the user's earned set:

1. the transitive prerequisite closure of the goals (the *relevant* set),
2. a topological order by longest-prerequisite-chain depth (cycle-safe),
3. a per-skill status — `earned` / `available` (all direct prereqs
   earned) / `locked`, and
4. up to 3 published-course recommendations per unproven skill, matched
   against the JSON `courses.skill_ids` column.

Multiple goals merge into one deduped path (`combinedPath`). The home
screen shows progress rings + the next unlocked step per goal;
`/goals` lists them with the full path (`LearningPathView.vue`).

### 14.4 Commands & settings

| IPC command | Purpose |
|---|---|
| `get_my_skill_graph` | Owner's full graph incl. private (editor) |
| `fetch_public_graph(did)` | Public graph for a DID (loopback or P2P broadcast) |
| `compute_learning_path(goal_skill_ids)` | Topo path + course recs from earned set |

| Setting key | Scope | Shape |
|---|---|---|
| `instructor.graph_prefs` | sync | `{ skill_id: { public, teaching } }` |
| `learner.targets` | sync | `Goal[]` |
| `identity.local_did` | device | cached `did:key` (lets the swarm loop answer graph-fetch) |

---

## Appendix A: Threat Model

### A.1 Reputation Gaming
Mitigated via evidence-only reputation and distributions.

### A.2 Assessment Inflation
Mitigated via difficulty weighting, trust factor propagation, and Sentinel integrity scoring.

### A.3 Sybil Instructors
Mitigated via bounded attribution, evidence requirements, and IP colocation scoring in the P2P layer.

---

## Appendix B: Governance Model

### B.1 Public-Interest Stewardship
Governed by DAOs that mirror the knowledge taxonomy. One DAO per subject field or subject.

### B.2 Spec Evolution
All taxonomy changes are versioned (`taxonomy_versions`), committee-gated, and ratified via 2/3 supermajority.

### B.3 Compatibility
Old proofs remain valid across taxonomy versions. Backward-compatible parsing required.

---

## Appendix C: Why This Replaces Resumes

### Overview
Resumes are proxies; this system provides proof.

### For Policymakers
- Portable credentials
- Reduced inflation
- Workforce visibility

### For Recruiters
- Skill-based filtering
- Reduced bias
- Verifiable hiring

### Rigor
Atomic skills, explicit evidence, confidence-aware reputation.

---

## Implementation Status

The reputation aggregation pipeline described above (§12) is now
backed by the **Alexandria Credential & Reputation Protocol v1**
end-to-end. See `docs/architecture.md` §13 for the layered
implementation and `docs/protocol-specification.md` for the
implementation-status preamble.

| Component | Module | PR |
|-----------|--------|----|
| Skill proofs (legacy NFT pipeline) | retired in migration 040 (`evidence::aggregator` and `mint_skill_proof_nft` removed) | — |
| Local-first completion credentials (+ optional on-chain anchor, content-only fallback) | `commands::completion::claim_course_completion` | VC-first |
| Document-bootstrap self-assertions (resume/transcript) | `commands::skill_bootstrap` | mig 068 |
| Dynamic Sentinel-gated assessments (randomized draw, host-side grade) | `assessment::*`, `commands::assessment` | mig 070 |
| W3C-style Verifiable Credentials | `domain::vc`, `commands::credentials` | PR 4–5 |
| Deterministic aggregation engine (Q, M, U, C, T, L) | `aggregation::aggregate_skill_state` | PR 6 |
| Type weights, freshness, **provenance-weighted quality**, independence (§14) | `aggregation::weights`, `aggregation::config` (`quality_triple`, calc version 1.1) | PR 6 / mig 068 |
| Anti-gaming (cluster cap, inflation z-score, §15) | `aggregation::antigaming` | PR 7 |
| Issuer clustering / pairwise dependence | `aggregation::independence` | PR 7 (per-DID v1; richer signals deferred) |
| Persisted derived-state cache (§16) | `commands::aggregation` (`derived_skill_states` table) | PR 13 |
| Recruiter / consumer query API (§17) | `get_derived_skill_state`, `list_derived_states`, `recompute_all` IPC | PR 13 |

The §26 worked example is reproduced end-to-end by the four
assertions in `tests/e2e_vc/aggregation.rs`:
`Q ≈ 0.846`, `C ≈ 0.514`, `L = 5`, `T = Q · C`.
