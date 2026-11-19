# The statement builder

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/12-identifier-compatibility-check.md

Spec: Learning records (builder, verbs, identifiers); Sending records → recipient identifier; ADR 0004.

- A desktop-free module in the shared crate that builds and validates xAPI statements, written as xAPI 2.0 within the subset also valid in 1.0.3.
- Verbs from the published ADL and cmi5 vocabularies. SCORM run-time data maps to statements following the published SCORM profile for xAPI.
- Activity identifiers under the DID that created the course, as confirmed by ticket 12. They do not change with owner, staff or visibility.
- Recipient identifiers: actor, statement and registration identifiers derived per recipient from the learner's key, so no two recipients receive the same ones and none equals the learner's DID unless the learner chose it.
- Records have a kind: completion-level or interaction-level.

Done when: unit tests build each statement kind and compare against published example statements, invalid input is refused before storage, and two recipients are shown never to share an actor, statement or registration identifier.
