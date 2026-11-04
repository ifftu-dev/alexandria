# Guardian AI requests through the AI access module

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

Give the AI access module guardian-side operations:

- record an inbound request
- list requests for the guardian overview
- record a guardian decision
- mark a request delivered

The guardian commands and the P2P guardian handler call these operations. No SQL on guardian AI requests remains outside the module.

Covers spec stories 63–64. This completes deepening ticket 03.

## Acceptance

- The module's tests round-trip each operation against an in-memory DB.
- A decision made through one guardian link can't touch another link's request.
- A grep for the guardian AI request table outside the module (and its schema) finds nothing.
- The command/direction guard script passes.
