# Integrate personal graph page, modal, and sidebar

Triage: done
Repos: alexandria
Part of: ../skill-graph-atlas-spec.md
Blocked by: 01-model-state.md, 02-explorer.md

## Work

Wire the shared data state and explorer into all personal surfaces, covering acceptance criteria 4–8.

- Replace the personal page and modal's divergent graph implementations with the shared explorer. Keep browse filters separate from graph controls so hidden filters cannot silently constrain the graph.
- Give the sidebar a matching compact presentation with identifiable fields, earned Bloom meanings, and explicit expand. Remove Locked wording only from the redesigned personal graph surfaces; do not alter the guided path or public instructor graph.
- Ensure graph selection/camera and the graph tab restore after Open skill and return; profile lock clears personal state. Preserve browse, credentials, goals, graph visibility, and existing detail routes.
- Integrate credential refresh without duplicate event listeners/requests or stale level displays. Reuse shared state and retain existing tests' meaningful refresh behavior.
- Maintain locale key parity and generated key declarations; avoid new raw interface strings. Remove obsolete personal-graph code only when no remaining consumer needs it.

## Acceptance and verification

Use TDD at the approved mounted-component and shared-state boundaries. Run focused/full frontend tests, strict typecheck, production build, and applicable i18n checks. Verify all three surfaces with the full bootstrap fixture, long labels/unassigned membership, both themes, reduced motion, and desktop/mobile sizing. Assess documentation needs, but do not modify unrelated documentation without permission. Report head SHA, validation, visual fixture access, and limitations for integration review.

## Completion

2026-10-27: `SkillAtlasHost.vue` owns refresh subscriptions and the modal independently of sidebar visibility. Page/modal share the explorer; sidebar uses the same model/palette. Explicit detail navigation restores the originating graph once on return. Learning actions open skill-filtered courses for earned and Ready targets; the production course-detail regression reaches lesson content and Enroll. Profile changes clear private graph state. The scoped locale-menu RTL fix prevents shifting the document. Full frontend and browser checks are recorded in the parent spec; native WebView validation remains outstanding.
