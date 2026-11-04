# Frontend hygiene and tracker bookkeeping

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

**Frontend:**

- One shared in-app path check, used by the deep-link composable and the deep-link parser.
- One shared helper that turns an unknown thrown value into a message.
- One localised-field helper in place of the inbox's near-identical title and body functions.
- Remove the held target from the deep-link composable's production return value. Tests observe it through replay behaviour instead.

**Bookkeeping.** Propose these doc edits before making them:

- The credentials deepening ticket's header moves to done, with a dated closing note.
- The accepted behaviour changes listed in the parent spec are confirmed there.

Covers spec stories 71–72 and 74–75.

## Acceptance

- The deep-link and notifications Vitests pass, with held-target coverage driven by behaviour.
- `scripts/check-docs.sh` passes.
- No duplicate path checks remain.

## Completion evidence — 2026-11-04 (code part)

Bookkeeping doc edits proposed, awaiting go-ahead.

What changed:

- `isInAppPath` in `src/deeplink/parse.ts` is the one in-app path check. The `open?route=` link, `openPath`, a notification tap's route and a held open's resolved route all use it. It also refuses backslashes (`/\host`), control characters and whitespace, and anything that resolves to another origin. `useNotifications`' `openRoute` still picks path or link by the leading slash; it checks nothing itself and hands paths to `openPath`.
- `errorMessage` in `src/utils/errorMessage.ts` replaces 42 inline copies and the genesis import page's local helper. The two camera catches with a localised fallback keep their own expression.
- `kindText(row, 'title' | 'body')` replaces the inbox's `titleOf` and `bodyOf`.
- `heldTarget` is gone from `useDeepLinks`.

Red → green:

- `isInAppPath` accepts and rejects cases, and `open?route=` with an encoded backslash: red (no such function; the link parsed). Now green.
- `openPath` refusing `/\host`, `/<tab>/host` and `javascript:`, and a tap or held open whose route is `/\host`: red (both navigated). Now green.
- `errorMessage` tests: red (no module). Now green.
- The held-target tests now unlock and check navigation: a held path navigates once, an immediate navigation leaves nothing to replay, the most recent held path wins and a refused path never navigates. They were checked against two mutations (keeping the first held target; never clearing it), and each mutation fails them.
- The inbox helper is a pure refactor, covered by the existing localisation tests.

Checks: full Vitest (92 files, 575 tests); `npx vue-tsc -b --noEmit`; `npm run check:tauri-commands`; `scripts/check-docs.sh`; `npx playwright test tests/desktop-ui/notification-links.spec.ts` (4 passed, Chromium and WebKit).
