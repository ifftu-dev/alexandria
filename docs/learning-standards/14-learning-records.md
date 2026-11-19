# Writing learning records

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/13-statement-builder.md, docs/learning-standards/05-attempts-and-progress.md

Spec: Learning records (what is recorded, kinds, local naming by DID, size limit, tables).

- Learning records and per-recipient outbox tables in the baseline schema.
- The app writes records for enrolment, element launch, progress, completion, pass and fail, and for package activity. No keystroke-level or per-answer detail for Alexandria's own assessments.
- Interaction-level records are written to the local log and are marked so they leave the device only under a scope that names them.
- Local records name the learner by DID.
- A visible size limit. Past it, the oldest interaction-level records go first, then completion-level records are kept as one summary per attempt.
- Each profile's records are kept apart.

Done when: host-seam tests drive enrolment, element and package events and assert the records written, their kinds, the size limit behaviour and separation between two profiles.
