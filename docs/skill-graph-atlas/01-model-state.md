# Shared graph model and profile-safe state

Triage: done
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
- 2026-10-27: Completed in app-owned worktree `t3-85532d4f`, branch `t3/skill-atlas-model-state`, against approved review base `d894a10b`. Reconciled recovery drafts with real domain types and the existing backend listing contract. Adapter/state behavior tests and strict `vue-tsc -b --noEmit` pass; legacy Bloom, profile, and sidebar tests pass. No UI consumer or backend changes. Tickets 02/03 own rendering and integration.

## Public API handoff for tickets 02/03

`src/utils/skillAtlas.ts` exports `buildSkillAtlas({ skills, edges, activeCredentials, subjectDid, subjectFields? })` and its presentation types. The adapter is deterministic and has no Vue, IPC, clock, or localization dependency.

- `AtlasModel` exposes nodes, fields, original edges (including duplicates and unresolved endpoints), and distinct loaded-skill counts. Nodes and field membership lists use stable ID ordering, independent of achievements and exploration controls. Ticket 02 owns deterministic coordinates keyed by these IDs; the state layer does not run a layout engine.
- Each node retains `skill` metadata, `fieldId`, `status: 'earned' | 'ready'`, nullable `earnedBloom: { rank, name }`, nullable `intrinsicBloom`, deduplicated prerequisite/dependent relations, and `prerequisitesSatisfied`. Rank is the raw integer 0–5; display rank + 1 and localize the name using `common.bloom.*`. An earned node with null Bloom needs an earned-level-unavailable label, not Remember or Ready.
- Achievements require an exact nonempty subject DID and a nonempty skill ID. Valid ranks compete by maximum; malformed ranks never override valid ones. No derived aggregation band is used. `activeCredentials` means the caller's existing `list_credentials({ subject: did, activeOnly: true })` result: backend nonrevoked listing, not fresh verification or a new date/acceptance policy. Legacy credential helpers are unchanged.
- Relations expose `skillId`, `name`, `loaded`, and `earned`. Evidence for unloaded prerequisites remains usable without counting those skills as loaded nodes. Missing evidence or missing prerequisite references implied by `prerequisite_count` prevents satisfaction emphasis. Satisfaction never controls access.
- Null/empty field IDs become Unassigned (`id: null`, localized by consumers). Existing IDs remain separate from Unassigned even if their label is unavailable. `subjectFields` resolves labels, with loaded skill metadata as fallback. Counts are `total`, `earned`, and `ready`; further learning for earned skills remains a separate UI action.

`src/composables/useSkillAtlas.ts` exports the shared `useSkillAtlas()` singleton:

- Readonly refs: `model`, `phase`, `error`, `refreshing`, `isLoading`, `pageView`, `modalView`, `skillsTab`, `modalOpen`, and `feedback`.
- `ensureLoaded()` covers already-unlocked startup. `refresh('navigation' | 'credentials' | 'retry')` refreshes the entire snapshot, including level-only changes. Navigation is throttled at 1500 ms; concurrent initial loads share work and forced refreshes queue one trailing request. Callers await the trailing work. Initial failures expose phase `error`; refresh failures retain the last good model with phase `ready` and an error. Successful retry clears the error.
- `updateView(surface, patch)` and `resetView(surface)` preserve independent page/modal state: selection, field focus (with `unassignedFocused` distinct from overview), query, Bloom emphasis, neighborhood/depth, camera, and inspector collapse. Camera `{ x, y, zoom }` is a copied snapshot in renderer world units; null requests initial fit. Refresh never resets these controls. `setSkillsTab` and `setModalOpen` persist session navigation state. No automatic route navigation or modal-return reopening is implemented.
- `activateSurface('page' | 'modal')` returns an idempotent release function. Register only while that graph is visible; release on hide/unmount. Initial snapshots are silent. Later newly earned or strictly improved known ranks produce `feedback: { revision, earnedIds, improvedIds }` only while a surface and the document are visible. `acknowledgeFeedback(revision)` consumes exactly that revision. Feedback expires after three seconds and is discarded on remount, final surface release, document hiding, reset, or a subsequent snapshot. Consumers still own reduced-motion rendering.
- `reset()` clears private data, both views, tab/modal state, pending feedback, throttle, and request ownership. Synchronous profile/lock/identity watchers reset before delayed lock hooks; generation checks reject stale successes, errors, and trailing work. A new profile can load without waiting for old IPC. Identity change detection uses the actual `Identity.stake_address`; the subject DID comes from `get_local_did`.

`installSkillAtlasRefresh(router)` installs reference-counted credential-event and successful-navigation listeners plus document-visibility cleanup. It returns an idempotent teardown function. Ticket 03 should install it from the app owner and use `ensureLoaded()` at startup/surface entry. Existing consumers remain untouched until that ticket.

## Verification record

- Recovered the earlier subject-bound Bloom/malformed-rank tests and kept their behavior green.
- Observed red → green for listing-policy preservation, partially loaded prerequisite emphasis, field-label fallback, startup field loading, new-profile loading during stale IPC, credential events, hidden/expired/consumed feedback, identity invalidation, cancelled navigation, document hiding, and camera ownership.
- Shared-state tests use the real profile lifecycle, local API/session guard, event publisher, and memory router. Only the external Tauri IPC boundary and time/visibility are faked. Additional checks cover forced-refresh coalescing, failure/retry, missing DID, stale errors/queued work, and synchronous clearing before delayed lock hooks.
- Focused command: `npx vitest run src/utils/__tests__/skillAtlas.test.ts src/composables/useSkillAtlas.test.ts src/composables/__tests__/provenBloom.test.ts src/composables/__tests__/useProfiles.test.ts src/components/layout/SidebarSkillGraph.test.ts` — 33 tests pass.
- `npx vue-tsc -b --noEmit` passes. Rendering, browser validation, UI translations, and full application integration remain for tickets 02/03.
