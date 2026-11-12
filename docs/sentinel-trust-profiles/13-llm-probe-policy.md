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
