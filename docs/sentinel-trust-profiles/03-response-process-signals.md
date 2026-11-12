# Response-process signals

Triage: ready-for-agent
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md
Blocked by: docs/sentinel-trust-profiles/01-scoring-core.md

Spec: Signals and measurement → response-process bullet and cross-modal bullet.

- Input-agnostic features per item: time to first input against a reading floor (item length + difficulty), time to commit, revision count, idle-then-burst, answer change after idle. Works for choice clicks, short answers and plugin events.
- Per-learner baseline from the first items of the attempt or recent practice; relay-latency feature = uniform added lag vs baseline.
- Cross-modal feature: gaze-down or second-voice event followed by a response event within a short window.
- Supersedes the planned `impossible_speed` flag from ai-chat-spec §13A as a measured feature.
- Features flow through the ticket-01 module as advisory until certified.
