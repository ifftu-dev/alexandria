# 04 Probes that ask something new against a fixed clock

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: none

Spec sections: *Probes*, stories 8, 15, 24.

## What to build

- Under the `reseed` policy a probe still runs and a failed one still flags the session, but a satisfied reseed probe no longer earns the `probed` rung; the session's rung stays at what it would be without it.
- `probed` needs a satisfied probe on a question the candidate had not answered: `authored` or `plugin` variants, or a generated probe.
- A probe's time limit is fixed by the item: the item's own limit when set, otherwise a default from the item's length and difficulty using the reading-time model the response-process signals use. Never derived from the attempt's pace.
- The probe screen states the time limit before it starts (check whether it already does).

## Acceptance criteria

- [x] Tests: a satisfied reseed probe leaves the rung below `probed`; a failed reseed probe still flags; a satisfied authored variant earns `probed`.
- [x] Test: two attempts at different paces get the same time limit for the same probe item.
- [x] Existing probe tests updated to the new rules, none deleted without a replacement.
- [x] fmt, clippy pass.

## Pointers

- `src-tauri/src/assessment/probes.rs` (`ProbePolicy::Reseed`, `per_item_ms`, `RESEED_TIME_BOX_*`), rung derivation in `src-tauri/src/credentials/issue.rs` and `src-tauri/src/sentinel/surface.rs`.
- Reading-time model: the response-process signal code under `src-tauri/src/sentinel/` (see [sentinel-trust-profiles/03](../sentinel-trust-profiles/03-response-process-signals.md)).

## Notes

2026-11-20: built.

- `probed` rule: `assessment::probes::asks_a_new_question(source)` is true for probe sources `variant`, `authored`, `plugin` and `llm`; `reseed` (the same item again), a missing or an unknown source are not. `sentinel::surface::session_surface` judges the `probed` evidence with satisfied probe results whose `detail.source` asks nothing new left out, so a satisfied same-item probe neither earns nor blocks the rung. Failed and skipped results still count, so a failed reseed probe flags the session as before. The assertion's probe summary is unchanged and still reports every result.
- An authored variant served under the `reseed` policy (source `variant`) earns `probed`: it asks a question the learner had not answered. The rule keys on the probe's source, not the policy.
- Time box: `item_time_box_ms(content, difficulty)` replaces `per_item_ms` and the `RESEED_TIME_BOX_*` / `DEFAULT_AUTHORED_TIME_BOX_MS` constants. It is the response-process reading floor (`reading_floor_ms`) over the prompt and option text at the item's difficulty, times 4, plus 20 s, clamped to 15 s–5 min. Authored follow-ups and variants keep the time box their author set and otherwise get this default from their own text. Plugin probes keep the grader's time box; generated probes keep their fixed 60 s. Nothing reads the attempt's start and submit times any more.
- Deviation: assessment items have no time limit column, so a same-item reseed probe always gets the default. No column was added.
- Probe screen: it did not state the limit before starting (it rendered at once, which starts the clock). It now states "This follow-up has a time limit of N seconds" with a Start button, and renders (starting the clock) only on Start; a probe whose clock already runs resumes without the step. English strings only; the other locales keep the probe strings pending.
- Tests: probe module tests cover the reseed rung, the failed-reseed flag, the authored variant, a mixed reseed + variant attempt, and equal time boxes across paces. Issuance tests now record a probe's source; the minimum-rung test shows a reseed probe does not lift the rung and an authored one does. The command-level consistent-rung test uses a variant source.
- For later tickets: a verifier check of rung evidence (02) must accept `probed` with policies `["reseed"]`, because an authored variant under `reseed` earns it. Docs still describing the old rules, for ticket 12: `docs/sentinel.md` (reseed time box, rung), `docs/sentinel-trust-profiles-spec.md` (reseed time), `docs/protocol-specification.md` §12.6 and the example assertion with `policies: ["reseed"]` on the `probed` rung.
