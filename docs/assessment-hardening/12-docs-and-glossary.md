# 12 Documentation and glossary

Triage: ready-for-agent
Type: task
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11

Spec sections: *Vocabulary*, *Further Notes* (documentation list).

## What to build

- Glossary: answer custody, hosted item bank, hosted attempt, companion-attested, required rung; the companion phone entry changed to "present and attested during the session".
- Update: Sentinel document and runbook, interview assistant (required-rung wait), enterprise boundary rows (hosted banks and attempts Enterprise; checking stays Core), protocol specification (assertion format), database schema reference, the Cloud organisation document and API docs.
- Close the spec and tickets as the issue tracker describes, with dated notes.

## Acceptance criteria

- [ ] `scripts/check-docs.sh` passes in both repos.
- [ ] Every term in the spec's *Vocabulary* is in the glossary.
