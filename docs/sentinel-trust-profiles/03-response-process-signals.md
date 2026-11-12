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

## Notes

2026-11-12, deviations and choices made while implementing:

- Feature group `WindowFeatures.response` (`alexandria_studio::integrity::ResponseFeatures`): per-visit aggregates; the input timeline stays on the client. Six signals: `response_below_reading_floor` (replaces the planned `impossible_speed`), `response_relay_lag`, `response_idle_burst`, `answer_change_after_idle`, `gaze_down_then_response`, `voice_then_response`.
- All six are `Info`, not `Warning` as `impossible_speed` was planned: unmeasured, they reach a decision only through a certified ratio. Channels: `response_process` for the first four; the gaze cross-modal signal joins `gaze` (one piece of evidence with `device_glance`); `voice_then_response` is `voice`.
- The reading floor is not conditioned on correctness: answer keys never reach the client and windows commit before grading.
- "Recent practice" baseline: the last 12 first-visit timings from earlier monitored sessions on the device, kept in the behavioural profile (`responsePattern`). Without three of them, the first three items of the attempt. Practice commands record no timing.
- Coverage: migration 34 adds `integrity_sessions.response_items` (first visits with timing), so a later rung can tell signals that ran from signals that never did.
- `ServedQuestion` now carries the bank difficulty for the floor.
- Only the standalone assessment runner (choice items) is wired. Lesson quizzes, plugin events and interviews use the same `useSentinel().responses` API; no voice activity detector exists yet, so `voice_then_response` waits for ticket 10.
