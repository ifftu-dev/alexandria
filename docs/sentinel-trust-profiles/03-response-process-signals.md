# Response-process signals

Triage: done
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
- Coverage: migration 35 adds `integrity_sessions.response_items` (first visits with timing), so a later rung can tell signals that ran from signals that never did.
- `ServedQuestion` now carries the bank difficulty for the floor.
- Only the standalone assessment runner (choice items) is wired. Lesson quizzes, plugin events and interviews use the same `useSentinel().responses` API; no voice activity detector exists yet, so `voice_then_response` waits for ticket 10.

Review fix, 2026-11-12: the `process` rung counted summed first visits against items served, so a repeated or unserved id could fill the count and a resumed attempt could fall short. Migration 39 adds `integrity_observed_items` (session, item id): every visit the tracker reports, first or return, is recorded once per id, and `response_items` is now the count of those rows. The rule:

- Every served item with a submitted answer must appear in `integrity_observed_items`: the tracker reported a visit to it in this or an earlier window of the same session. The table persists across a reload, so an answer restored on resume and left alone counts when it was worked through before the reload. An answered item the tracker never observed keeps the rung at `none`: answers alone do not show that response-process signals ran.
- Ids the attempt never served are ignored.
- An item left unanswered and never visited is left out of the requirement: it carries no response and scores nothing, so it cannot be counted against the learner.
- At least one served item must be observed, so a session whose tracker never ran stays at `none`.
- Answered means a submitted answer with a selection (`submitted_answers_json`), or an adaptive item with a submission (`attempt_items.submission_json`).
- Review fix: the client cleared the response tracker's window (and every other window accumulator) even when the snapshot submit failed, so those visits never reached the backend. A window is now cleared only after the backend accepted it; otherwise it stays open and goes out with the next one. Native hotkey events drained from the ring are kept the same way.

- 2026-11-13: closed. Merged with the rest of the spec; review findings fixed alongside.
