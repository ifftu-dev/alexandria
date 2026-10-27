# Shared graph model and profile-safe state

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/skill-graph-atlas-spec.md
Assignee: atlas-model-state

## Work

Implement the public graph-data adapter and shared data/view-state boundaries needed by the personal atlas. Follow acceptance criteria 1–3 and 7 in the parent spec. Read the current credential helpers, profile lifecycle, and existing sidebar refresh tests before choosing the interface.

- Carry existing skill/subject/field metadata and actual prerequisite references into presentation data.
- Compute current earned Bloom from active, subject-bound skill claims; never use `DerivedSkillState.level` as a Bloom rank. No earned claim means Ready. Preserve legacy helpers for existing consumers unless a behavior change is demonstrably needed.
- Expose field counts, earned/Ready nodes, and prerequisite-satisfaction emphasis independently of access. Handle unassigned fields, partially loaded links, absent DID, malformed levels, and duplicate credentials deliberately.
- Introduce shared profile-safe load/refresh and view-state persistence through composables. Refresh level-only credential changes; prevent stale async work from restoring a locked profile's graph. Retain compatibility with old consumers until ticket 03 switches them.
- Make layout/view state stable by skill and field ID, not achievement status. Provide a clear documented public API for ticket 02; keep the adapter UI-independent.

## Acceptance and verification

Use TDD at the approved graph-adapter and shared-state seams. Verify subject isolation, current levels, Ready semantics, field counts, prerequisite preservation, refresh and restoration. Run focused tests and strict typecheck; report existing unrelated failures separately. Commit signed-off conventional subjects and report the public API, head SHA, checks, and remaining integration requirements.

## Notes

- 2026-10-27: Created; all parent-spec test seams approved by the user. Claimed for isolated implementation.
