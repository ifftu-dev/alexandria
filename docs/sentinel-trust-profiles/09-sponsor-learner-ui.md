# Sponsor and learner surfaces

Triage: ready-for-agent
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md
Blocked by: docs/sentinel-trust-profiles/08-credential-surface.md

Spec: Credential and policy surface → sponsor and credential bullets; User stories 2, 4, 14, 16-18, 24, 26-28.

- Sponsor role-assessment form: minimum profile, rung, probe policy, with plain-language coverage text.
- Sponsor session summary ranked by fusion posterior, per-signal certified rates, advisory marking.
- Learner pre-attempt notice: chosen profile, why, policy floor result, how to raise it (better device, paired phone).
- Credential detail and exported bundle show the resolved coverage statement.
- i18n keys in all locales; strict vue-tsc.

## Notes

2026-11-12, what was built and deviations:

- Sponsor form: `RoleIntegrityPolicy` (components/integrity) holds minimum trust profile, minimum verification rung and follow-up policy (`reseed`, `authored`, `plugin`; ticket 13 adds `llm` to `PROBE_POLICIES` in `utils/sentinel/rolePolicy.ts`). Each select has localised plain language beside it, and the coverage statement a credential under those minimums names is shown below, from the new unscoped IPC `integrity_coverage_statements`. Role cards list the three minimums.
- Session summary: IPC `list_role_session_reviews(roleAssessmentId)` rescores each finished session of the role's attempts from its committed windows (`sentinel::surface::session_review`) under `active_manifest()` and ranks them by fusion posterior: the summed channel log-likelihood ratio, which orders sessions the same as a posterior under any shared prior, so no prior is assumed. Ties fall to session status, then signal windows. Each raised signal carries its certified precision and recall from `active_certification()` or is marked advisory. Until ticket 14 every signal is advisory and the queue is ordered by status. Shown on the sponsor's issue tab with a "Use" that fills the issue form.
- Learner notice: `TrustProfileNotice` before the first item names the chosen profile, the signals not run and why (from the selection's exclusions), the floor result and what would raise the profile (pair a phone, attested device, connect or allow the camera, faster device). When the floor is unmet the runner stops monitoring before `assessment_start_attempt` and shows the notice instead of the backend's refusal; nothing is served.
- Credential detail shows profile, rung, signals run/degraded/not run, the probe summary and the statement resolved from `coverageStatementDigest` (or says this build does not carry it, with the digest). Exported bundles carry `coverage_statements`: every statement their credentials name, once, in digest order (omitted when empty, so older bundles still parse).
- Statement lines are quoted in English in every locale: they are the content-addressed contract, and translating them would change what the digest covers. Headings, profile and rung names and hints are localised.
- Ticket 10's voice-check consent text is translated into every locale and its pending-translation entry is gone.
- Live rung: new interview signal `live_segment { segment_id }` (panel to candidate). A panel seat's signal feed has "Live segment done"; the candidate's running check (`useInterviewCheck`, now listening for panel signals whether or not voice checks run) calls `hiring_interview_live_segment`, which records a satisfied `interview` verification (policy `panel`, subject the segment id, detail: interview, segment, who marked it) through `record_verification`. Refused unless the learner accepted the interview with `mode = alexandria` and an integrity check, the session's purpose is `interview` and it has not ended; a segment is recorded once. Like every result in the check it is the candidate's app's own record, folded into the commitment chain; the panel's mark is not signed by the panelist.
- Limitation: the rung is a strict ladder and an interview session has no attempt, so `live` evidence in an interview check does not lift that session above `none`. Reaching `live` on a credential needs an attempt and an interview in one session, which no flow creates yet. The interview report sent to Cloud does not carry the verification (its wire struct refuses unknown fields; Cloud is out of scope).
