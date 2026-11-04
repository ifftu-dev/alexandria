# Frontend hygiene and tracker bookkeeping

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

**Frontend:**

- One shared in-app path check, used by the deep-link composable and the deep-link parser.
- One shared helper that turns an unknown thrown value into a message.
- One localised-field helper in place of the inbox's near-identical title and body functions.
- Remove the held target from the deep-link composable's production return value. Tests observe it through replay behaviour instead.

**Bookkeeping.** Propose these doc edits before making them:

- The credentials deepening ticket's header moves to done, with a dated closing note.
- The accepted behaviour changes listed in the parent spec are confirmed there.

Covers spec stories 71–72 and 74–75.

## Acceptance

- The deep-link and notifications Vitests pass, with held-target coverage driven by behaviour.
- `scripts/check-docs.sh` passes.
- No duplicate path checks remain.
