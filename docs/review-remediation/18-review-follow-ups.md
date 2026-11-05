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

## Completion evidence (atlas) — 2026-11-05

What changed:

- **Ready next (story 51).** A not-yet-earned skill is ready next when it has at least one prerequisite and every prerequisite it counts is loaded and earned. Skills with no prerequisites, with partly earned prerequisites, or with a prerequisite that is unloaded or uncounted are not ready next. `prerequisitesSatisfied` now also needs every prerequisite loaded and the loaded count to equal `prerequisite_count`, so an earned but unloaded prerequisite no longer satisfies it. Ready next skills get a stronger map node (field-coloured border, a wider field edge and a field tint) and a tinted list row, with a decorative ◎. Their accessible name says "ready next" ("Sorting Algorithms, not yet earned, ready next, learning-outcome Bloom level 3 Apply"), and so do the evidence line, the inspector status and inspector relations. Every unearned skill is still counted as Ready. The class is in the node's `v-memo` dependencies, and the label comes from the cached node text.
- **Durations.** `src/components/skills/**` takes its transition durations from the `--duration-*` tokens.

Text the webview sees:

- New: "Ready next", "{name}, not yet earned, ready next[, learning-outcome Bloom level {level}]" and "Inspect {name}, not yet earned, ready next". The atlas legend's last sentence now reads "Every other skill is Ready; ◎ Ready next marks those whose prerequisites you have all earned." All are translated by hand into the nine locales; the other locales keep their "Ready" word (for example "Lista para empezar").

Red → green:

- `skillAtlas.test.ts`: ready next only with every counted prerequisite loaded and earned (none, partial, unloaded, uncounted, over-counted and already earned cases), and an earned but unloaded prerequisite no longer satisfies a skill.
- `SkillAtlasExplorer.test.ts`: only the ready next skill is emphasised in the list and on the map, with its accessible name, visible label, inspector status and relation names; the memoised node gains and loses the emphasis as the model changes.
- `skill-atlas.spec.ts` (Chromium and WebKit): Sorting Algorithms is ready next with the stronger node; Greedy Algorithms (partly earned), Graph Algorithms and Python (no prerequisites) stay Ready. It failed against the previous code.
- `motionDurations.test.ts`: the three `src/components/skills` entries failed, then passed.

Checks: full Vitest (98 files, 605 tests), `npx playwright test` on the skill-atlas, personal-atlas and atlas-performance specs (Chromium and WebKit, 34 passed, 2 skipped), `npx vue-tsc -b --noEmit`, `npm run i18n:keys`, `npm run i18n:parity`, `npm run i18n:no-raw-text`, `npm run check:csp`, `scripts/check-docs.sh`.

Performance: `atlas-performance.spec.ts` saw no long tasks. With 20× CPU throttling on the scale fixture there were none in six runs on a quiet machine. With other builds running, the old and new code both had occasional single tasks of 50–65 ms, and outliers of about 200–300 ms, in an interleaved comparison of eight runs each.
