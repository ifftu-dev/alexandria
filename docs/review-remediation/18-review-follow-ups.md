# Review follow-ups

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

What the post-implementation review of tickets 01–16 left open, plus the tooling gaps CI exposed.

**Backend standards.**

- Guardian AI requests: the request type lives in `domain/` and its status is a closed enum, mirrored in `src/types/index.ts`.
- The profile session travels as a type of its own, not a bare string.
- The guardian decision and its peer-to-peer approval payload, and the notification payload built in two places, each become one shape.
- Push: the permission mapping exists once, and the push issuer comes from `Issuer::from_keystore`.
- `NotificationError::Profile` is split or renamed so a database failure is not called a profile failure.
- The notification vocabulary snapshot is owned by the notifications module, not AI chat; the AI access prose names `AccessDenied` correctly.
- The notification permission type no longer shares its name with the browser's `NotificationPermission`.

**Frontend standards.**

- Durations in components come from the motion tokens (story 47).
- One settle-in helper replaces the copies in the layout, settings, guardian and instructor pages.
- One motion list primitive replaces the repeated `TransitionGroup` blocks.
- The selection-indicator class no longer collides with `[data-selectable]`.
- The search palette renders results from one template.
- The notifications inbox list enters, leaves and moves like the other lists.

**Skill atlas.** Stronger emphasis goes only to skills whose real prerequisites are earned (story 51).

**Tooling and docs.**

- `scripts/check.sh` also runs the release workflow's Python tests.
- The WebKit press-feedback check stops flaking.
- `src/AGENTS.md` and `src-tauri/src/AGENTS.md` note the progress-marker guard and how synced settings are followed.

Ticket 17's device checks stay with a person.

## Acceptance

- Each item has a test at the module interface or the desktop UI suite, red before and green after, or is a pure refactor covered by existing tests.
- `scripts/check.sh` and the desktop UI suite pass.
- Text the webview sees is unchanged, or the change is listed here.
