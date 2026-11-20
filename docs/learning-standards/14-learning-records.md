# Writing learning records

Triage: done
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

## Notes

2026-11-20: built.

- Records are stored as learning events with the learner's DID beside them, validated by building them once, and built into a statement per recipient when read. The local log's recipient is the learner's DID with the label `activity-log`.
- The two tables and the creator DID column are one new migration. The schema doc is regenerated.
- What is recorded: enrolment (registered, course); the first move of an element to in progress (launched); an element marked complete (completed, or for a quiz, interactive or assessment element with a score, passed or failed at the same pass score the completion witness uses), recorded again when a retake changes the score; course progress as a whole percentage each time an element is newly completed; each launch of a package activity; and, when a package session ends or the launch ends, what the package reported (one result per attempt, activity and verb, one response per interaction). No record is written for a built-in assessment's answers.
- "One summary per attempt" is read as one per attempt at each activity (per registration and activity identifier), so an enrolment keeps one record per element rather than one in all. The kept record is the strongest result, newest among equals. Records still waiting in an outbox are never removed; past that the log may stay over its limit rather than drop summaries.
- The size limit is a fixed 16 MiB of stored events, reported with the size by a new command for the activity log.
- Activity identifiers need the course creator's DID, which was not stored. Hydration now keeps the verified document's author DID on the course; a course this profile authored uses the profile's DID. The profile's DID, which the records name the learner by, was cached only when one screen asked for it; profile start now caches it. A record that cannot be named (no DID, unknown creator, an id that is not canonical) is logged and skipped; it never fails the learner's action.
- cmi5 package results are not read here: the cmi5 session writes its own statements (ticket 25).
- Proposed doc edits (not made): the glossary terms "learning record" and "activity log" from the spec's vocabulary; the enterprise boundary row "Writing learning records" in the worked examples.
