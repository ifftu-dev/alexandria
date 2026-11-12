# Scoring and fusion in Rust

Triage: ready-for-agent
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md

Spec: Implementation Decisions → Signals and measurement (first four bullets); Testing seam 1.

- Move score derivation and flag decisions out of the frontend composable into a Tauri-free module in `crates/alexandria-studio` (ADR-0004: no new core crate). The app re-exports it.
- Composable becomes a collector: submits raw per-window features; backend derives scores and flags. Severity stays authoritative on the backend.
- Fusion: per-signal log-likelihood ratios accumulated per session against a per-profile threshold. Ratios come from a certification manifest (ticket 02 writes real ones); until then every signal has no ratio, contributes zero, and is reported advisory.
- Decision rule: two independent channels over threshold, or failed verification, before a credential-affecting outcome. A single flag of any severity puts the session in a new `review` status, never `suspended`. Auto-suspend stays only for the backend-recorded tamper flags.
- Historical snapshots are not rescored; old-client snapshots carrying precomputed scores follow the existing retirement pattern.

Done when: seam-1 tests (session over in-memory DB) cover single-flag → review, two channels → flagged, tamper flag → suspended, advisory signals contributing zero; composable vitest asserts raw features submitted.
