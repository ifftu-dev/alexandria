# Skills & Reputation

> Open skill graph, evidence-based credentials, and reputation system.

> **⚠️ Post-VC-first cutover (migration 040, 2026-04-24):** Every
> reference to `skill_proofs`, `evidence_records`, or
> `skill_assessments` in this doc describes the retired pipeline.
> Credentials are now W3C VCs auto-earned via a Cardano completion
> validator. Reputation computation will repoint at credentials
> (math unchanged). See [`vc-migration.md`](./vc-migration.md).

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

`evidence_records` bind assessments to verifiable outcomes:
- `skill_assessment_id`, `skill_id`, `proficiency_level`
- `score`, `difficulty`, `trust_factor`
- `course_id`, `instructor_address`, `integrity_session_id`, `integrity_score`
- optional `cid` and `signature`

Evidence is broadcast over the `/alexandria/evidence/1.0` GossipSub topic with Ed25519 signatures for authenticity.

IDs are deterministic: `hex(blake2b_256(parts.join("|")))`.

---

## 7. Skill Proofs (Credentials)

`skill_proofs` aggregate evidence into learner-owned credentials:
- Deterministic ID scoped to `(learner, skill_id, proficiency_level)`
- Confidence score derived from weighted evidence aggregation (`evidence/aggregator.rs`)
- Evidence linkage via `skill_proof_evidence`
- Optional `cid`, `nft_policy_id`, `nft_asset_name`, and `nft_tx_hash` columns for export/on-chain wrappers

Proofs can be wrapped as native-script NFTs on Cardano (Conway era) with CIP-25 metadata containing skill, proficiency level, confidence score, and evidence count. More advanced soulbound/CIP-68-style paths exist in the codebase but are not fully deployed yet.

---

## 8. Learner Ownership Model

All credentials are stored locally in the learner's SQLite database and iroh content store. The learner controls:
- Which proofs to mint on-chain (making them publicly verifiable)
- Which evidence to broadcast over P2P
- Cross-device sync of credentials via encrypted sync

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

- All evidence is Ed25519 signed with the learner's Cardano payment key
- Evidence can be challenged via stake-based challenges (5 ADA stake, 2/3 supermajority vote)
- Multi-party attestation requirements for high-stakes assessments
- Behavioral integrity scores from the Sentinel anti-cheat system lower `trust_factor` on flagged assessments
- Identity binding via TOFU (Trust On First Use) in the P2P validation pipeline

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

Stored in `reputation_assertions` with supporting evidence in `reputation_evidence`.

### 12.4 Instructor Impact

```
Impact(I, S, P) =
  Σ learners [ ΔConfidence × Attribution ]
```

Computed in `evidence/reputation.rs`. Impact deltas stored in `reputation_impact_deltas` with FK to individual evidence records.

Reputation snapshots can be anchored on-chain as CIP-68 soulbound tokens with CBOR-encoded datums (`reputation_snapshots` table).

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
| Skill proofs (legacy NFT pipeline) | `evidence::aggregator`, `commands::cardano::mint_skill_proof_nft` | Pre-existing |
| W3C-style Verifiable Credentials | `domain::vc`, `commands::credentials` | PR 4–5 |
| Deterministic aggregation engine (Q, M, U, C, T, L) | `aggregation::aggregate_skill_state` | PR 6 |
| Type weights, freshness, quality, independence (§14) | `aggregation::weights` | PR 6 |
| Anti-gaming (cluster cap, inflation z-score, §15) | `aggregation::antigaming` | PR 7 |
| Issuer clustering / pairwise dependence | `aggregation::independence` | PR 7 (per-DID v1; richer signals deferred) |
| Persisted derived-state cache (§16) | `commands::aggregation` (`derived_skill_states` table) | PR 13 |
| Recruiter / consumer query API (§17) | `get_derived_skill_state`, `list_derived_states`, `recompute_all` IPC | PR 13 |

The §26 worked example is reproduced end-to-end by the four
assertions in `tests/e2e_vc/aggregation.rs`:
`Q ≈ 0.846`, `C ≈ 0.514`, `L = 5`, `T = Q · C`.
