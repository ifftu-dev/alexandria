# 12 Documentation and glossary

Triage: done
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

- [x] `scripts/check-docs.sh` passes in both repos.
- [x] Every term in the spec's *Vocabulary* is in the glossary.

## Notes

2026-11-21: done.

- App: `GLOSSARY.md` gains answer custody, hosted item bank, hosted attempt, companion-attested and required rung; the assurance level is a three-step ladder, the companion phone is present and attested during the session, and a probe counts towards `probed` only when it asks something new. The Cloud has no glossary; the shared vocabulary stays in the app's.
- App docs: `sentinel.md` (companion level, verified-device refusal, probe rules, watchlist signer, answer custody and the hosted attempt on the device, coverage statement version 3, the `live` gap), `sentinel-runbook.md`, `protocol-specification.md` (§12.6 probe rules; §14.7, §14.9.5 assertion and policy rule; new §14.9.7 hosted attempts; §14.13.1 custody defects; §14.14.5 weights), `interview-assistant.md` (required-rung wait and the sponsor-device limitation), `enterprise-boundary.md`, `skills-and-reputation.md` (calculation version 1.5), `ai-chat-spec.md` (bank guard), `project-structure.md`, `database-schema.md` (regenerated), a superseded-in-part note on `sentinel-trust-profiles-spec.md`, and the docs index.
- Cloud docs: `hosted-attempts.md` (plugin items, close evidence, issuance and the result endpoint, run status), `organisations.md` (hosted item banks, defaults, run view, migrations 0057–0060), `CONTRIBUTING.md` (vendored plugins), the `src/lib.rs` header.
- Found in code: the trust profile notice still suggests pairing a phone on a desktop (`profileNotice.ts`, `not_attested` → `pair_phone`), although a paired phone no longer raises the profile. Left for a code fix; `sentinel.md` said so. 2026-11-21 review fixes: fixed (the step is `attested_device` on every device; `pair_phone` is gone) and the note removed from `sentinel.md`.
