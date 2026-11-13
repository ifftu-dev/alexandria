# Scoring and fusion in Rust

Triage: done
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md

Spec: Implementation Decisions → Signals and measurement (first four bullets); Testing seam 1.

- Move score derivation and flag decisions out of the frontend composable into a Tauri-free module in `crates/alexandria-studio` (ADR-0004: no new core crate). The app re-exports it.
- Composable becomes a collector: submits raw per-window features; backend derives scores and flags. Severity stays authoritative on the backend.
- Fusion: per-signal log-likelihood ratios accumulated per session against a per-profile threshold. Ratios come from a certification manifest (ticket 02 writes real ones); until then every signal has no ratio, contributes zero, and is reported advisory.
- Decision rule: two independent channels over threshold, or failed verification, before a credential-affecting outcome. A single flag of any severity puts the session in a new `review` status, never `suspended`. Auto-suspend stays only for the backend-recorded tamper flags.
- Historical snapshots are not rescored; old-client snapshots carrying precomputed scores follow the existing retirement pattern.

Done when: seam-1 tests (session over in-memory DB) cover single-flag → review, two channels → flagged, tamper flag → suspended, advisory signals contributing zero; composable vitest asserts raw features submitted.

## Notes

2026-11-12, deviations and choices made while implementing:

- Module: `alexandria_studio::integrity` (features, flags, fusion), re-exported as `sentinel::scoring`. The snapshot request is `{ session_id, element_id, features }`; any other field (old-client scores or flags) is refused before writing.
- "A single flag of any severity" is read as any warning or critical flag. Info flags (`external_display`, `tab_switching`, `ai_assistant_running`, …) stay context and do not send a session to review.
- Interview checks never suspend: a tamper flag sets them to `flagged`.
- The weighted composite stays as a descriptive per-window score (`composite_score`, session `integrity_score`); it no longer decides status, so a low running score no longer flags a session by itself. `integrity_end_session` takes no client scores.
- `review` is accepted wherever terminal session statuses are listed (grading gate, evidence consent, hiring report, `alexandria-verify` exchange, dashboards). No migration: `integrity_sessions.status` has no CHECK constraint.
- Sessions are scored under profile `core` until ticket 05 records a profile per session.
- The calibration wizard's training metrics keep their small TS analyzers; they never reach a snapshot.

- 2026-11-13: closed. Merged with the rest of the spec; review findings fixed alongside.
