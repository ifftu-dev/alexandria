# Press feedback and motion rollout step 2

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md
Blocked by: alexandria/docs/review-remediation/07-motion-progress-slow-start-and-tokens.md

## Task

- **Shared press treatment.** Add one, built from the motion tokens, that controls opt into through a shared class or wrapper. Apply it to navigation items, tab bar items, list rows, search results, chips and icon buttons.
- **Rollout step 2.** Complete it through the shared policy, tokens and existing transition components (no new animation library):
  - persistent navigation selection indicators (sidebar, mobile tab bar, mode switcher)
  - search open/close and result entry
  - list enter/leave/move
  - home, goal and catalog card entry and press
  - goal add/remove confirmation
  - lesson step transitions in daily learning

Every transition honours Reduced and Off, keeps all information visible, and never delays input.

Covers spec stories 32–38 and 44.

## Acceptance

- The desktop UI suite covers press feedback on non-button controls and Reduced/Off behaviour on each step 2 surface, on Chromium and WebKit.
- Component Vitests cover the shared press treatment.
- There are no input delays: interactions stay immediate under the existing interaction tests.
