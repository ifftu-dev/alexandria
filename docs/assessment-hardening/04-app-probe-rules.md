# 04 Probes that ask something new against a fixed clock

Triage: ready-for-agent
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

- [ ] Tests: a satisfied reseed probe leaves the rung below `probed`; a failed reseed probe still flags; a satisfied authored variant earns `probed`.
- [ ] Test: two attempts at different paces get the same time limit for the same probe item.
- [ ] Existing probe tests updated to the new rules, none deleted without a replacement.
- [ ] fmt, clippy pass.

## Pointers

- `src-tauri/src/assessment/probes.rs` (`ProbePolicy::Reseed`, `per_item_ms`, `RESEED_TIME_BOX_*`), rung derivation in `src-tauri/src/credentials/issue.rs` and `src-tauri/src/sentinel/surface.rs`.
- Reading-time model: the response-process signal code under `src-tauri/src/sentinel/` (see [sentinel-trust-profiles/03](../sentinel-trust-profiles/03-response-process-signals.md)).
