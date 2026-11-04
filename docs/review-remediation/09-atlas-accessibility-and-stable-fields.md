# Atlas: accessibility, honest wording and stable fields

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

- **Accessible names.** Nodes, list items and previews build their accessible name from the skill name, its earned or not-yet-earned state, and its Bloom level number and name. The checkmark becomes decorative.
- **Focus indicator.** Add an on-map indicator, visible whatever the inspector state, showing the active prerequisites/dependents focus, its depth and its skill, with a clear action.
- **Prerequisite wording.** "Prerequisites satisfied" means at least one prerequisite, all earned. Skills with none show "No prerequisites" and get no readiness emphasis.
- **Stable field slots.**
  - Each subject field gets an explicit, stable atlas slot in the bundled taxonomy, exposed on the subject-field domain type in Rust and TypeScript.
  - Colour and region order come from the slot. New fields take the next unused slot.
  - The palette has at least as many distinct light/dark colours as there are bundled fields, plus a documented wrap rule.
  - Check whether the slot affects the taxonomy digest used for Cloud sync, and update the digest if it does.
- **Shared presenter.** One atlas presentation module owns field name, field colour and Bloom label for the sidebar graph and the explorer. Layout spacing becomes named constants, the skill-return shape gets a named type, and guards replace the tab, Bloom-rank and field-ID casts.

Covers spec stories 48–53 and 60–62.

## Acceptance

- Component Vitests assert the accessible names and the indicator with the inspector collapsed.
- A test adds a field and shows the existing fields keep their colours and regions.
- The desktop UI atlas specs pass on Chromium and WebKit.
- The Rust seed tests cover the slots.
