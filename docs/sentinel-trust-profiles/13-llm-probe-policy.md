# LLM probe policy (enterprise)

Triage: ready-for-agent
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md
Blocked by: docs/sentinel-trust-profiles/07-probe-contract.md, docs/sentinel-trust-profiles/08-credential-surface.md

Spec: Probes → llm bullet; Q3 decisions in the spec's Decisions section.

- Separate one-shot run type, not chat; attempt gate still refuses chat.
- Grounded only in submission + skill description; item-bank guard on outbound payload; consent at attempt start.
- Sponsor-configured model connection on the role assessment; provider failure → skipped.
- Model id, prompt hash and latency recorded in the snapshot chain.
- Structured decision provider may score probe answers only through a newly approved task with held-out evaluation; it never generates probes. No provider name in UI.

## Notes

2026-11-12, what was built and deviations from the bullets above:

- Run type: `assessment::llm_probe`, one request per probe, no thread, history or tools. `assessment_probes_start` prepares a plan in one database job, calls the connection with no job held, and stores the answers in a second job. The attempt gate is unchanged: chat, the tutor, practice and the broker stay refused during the attempt (test-enforced); the probe call only runs for a submitted attempt the caller owns inside its live session, and still passes `ai_access::may_use` (age and guardian rules).
- Grounding: the outbound message holds the skill name and description and the text of the option(s) the learner chose. The item's prompt, other options and key never leave. The message passes `ai_chat::bank_guard` first (match: skipped `bank_guard`, nothing sent).
- Consent: `assessment_start_attempt` takes `llmProbeConsent`, stored on `assessment_attempts.llm_probe_consent` at the first start (a resume keeps it). `AttemptRoleTarget.generated_probes` tells the runner to ask; without a yes the step is skipped `not_consented`. Declining does not refuse the attempt, since a skipped probe never counts against the learner; a sponsor who needs the probe sets `min_verification_rung: probed`.
- Connection: `IssuancePolicy.probe_connection_id` names a saved model connection (Studio connection document) on the device that runs the attempt. The sponsor form gains one select; choosing a connection sets `probe_policy: llm`. Distributing a sponsor's connection to learner devices (Cloud) is out of scope. A bank-level `llm` policy has no connection and is skipped `provider_unavailable`, which keeps the policy enterprise-only.
- Chain record: each generated probe stores `generation_json` (`format`, `model`, `location`, `prompt_hash` = blake3 over the canonical request, `latency_ms`), copied into every verification result recorded for it, including skips after a provider failure. The issuance check needed nothing new.
- Format: the model must return one single-answer choice question with four options and its key (`mcq_single`); it is validated (bounds, distinct options, key grades itself) and graded by the item's own grader. Invalid answers are skipped `invalid_probe`, provider errors `provider_unavailable`. Only choice items get a generated probe (`unsupported_item` otherwise). Time box 60 s.
- Migration 38 `llm_probes` (renumber if 09 lands a migration first): the consent column, `generation_json`, and `assessment_probes.source` gains `llm` (table rebuilt, since the CHECK cannot be altered).
- Scoring seam, not wired: the structured-decision provider does not score generated probes. A free-text generated format would need a scorer, and that provider may take the role only through a newly approved task with a held-out evaluation, which does not exist. The seam is the generation `format` field plus the grading branch in `answer_probe_db`; today only `mcq_single` is produced and accepted. Recorded in the module docs of `assessment::llm_probe`.
- Concurrent starts: two starts racing during generation may both call the model; only the first store wins, the second returns the stored probes.
- New strings are English-only (`learn.assessment.generatedProbe*`, `learn.assessment.probePreparing`, `dashboard.sponsor.roleModal.generatedProbe*`, `dashboard.sponsor.roles.generatedProbe`), pending translation. The UI never names a provider: the sponsor sees the connection name they chose.
