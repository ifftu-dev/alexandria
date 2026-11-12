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
