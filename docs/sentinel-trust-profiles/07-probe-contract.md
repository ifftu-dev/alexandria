# Probe contract and reseed/authored/plugin probes

Triage: ready-for-agent
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md

Spec: Implementation Decisions → Probes (all but the llm and interview bullets); Testing seam 3.

- Probe policy per assessment/role: `none`, `reseed`, `authored`, `plugin` (`llm` is ticket 13).
- Grader contract gains an optional probe envelope beside the grade envelope (item content + submission in; question, machine-gradable expectation, time box out). Native choice graders and wasm graders both implement it.
- Instructors can author follow-ups and seeded variants per item.
- Probes run inside the integrity session after submission; time box starts on render; probe latency is a response-process input.
- Verification result recorded in the snapshot chain: satisfied, failed, skipped (+ policy). Skipped never counts against the learner.
- Plugin credential rule (ADR-0003) unchanged: bundled graders only for credentials.

## Notes

2026-11-12, deviations from the bullets above:

- One policy per assessment (`question_banks.probe_policy`) and per role (`IssuancePolicy.probe_policy`, not a column); a role policy other than `none` wins. Combining policies per item type is not built.
- The role's `probe_policy` only chooses probes. Evaluating it at issuance belongs to ticket 08.
- Only items answered with full marks are probed, at most two per attempt. Which items get a probe hints at which answers were right; the outcome itself stays on the host.
- Reseed time box: 1.5 × (attempt duration ÷ items), clamped to 15 s–5 min. Per-item timing does not exist yet (ticket 03).
- Policy `none` still records one skipped probe step (reason `policy_none`), so the chain shows the policy.
- The built-in MCQ grader wasm was rebuilt with `alex_probe` (a complement probe: pick every option that is not correct), so its grader CID changed. The editor graders have no `alex_probe` yet, so the `plugin` policy is skipped (`provider_unavailable`) for their items.
- Beyond the bullets: a failed verification raises the session to `flagged` and feeds ticket 01's fusion input `verification_failed`, following the spec's decision rule.
- Authoring is IPC only (`assessment_item_probe_*`, allowlisted); there is no instructor UI yet. Authored probes do not travel with items over sync.
- Review fix: every grading error used to skip the probe `provider_unavailable`, so a response no grader could read escaped as a skip. `items::grade_item` now returns `GradeError`: `Unavailable` (grader not installed, not runnable on the platform, bytes that do not match, a failure inside it) still skips; `InvalidResponse` (not a JSON object, or choice positions that are not a list of the item's options) fails the probe, with `error: invalid_response` in the detail. A choice response with no selection still grades as an unanswered, wrong answer. Attempt grading sees the same check; submitted answers are validated before they are stored, so it changes nothing there.
