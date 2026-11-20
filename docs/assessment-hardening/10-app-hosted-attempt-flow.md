# 10 Hosted attempts in the app

Triage: ready-for-agent
Type: task
Repos: alexandria
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: 03, 05, 08, 09

Spec sections: *Hosted attempts* (device side), *A role that requires a verified device*, stories 9–20.

## What to build

- When a request names a role with a hosted bank, the app runs a hosted attempt against the endpoints ticket 08 documents: consent screen (answers and per-item timings go to the organisation, device requirement, retake rule, retention) before the first item; then one item at a time from the service.
- Item text lives in memory only while shown: never written to the profile database or logs, and covered by the guard that keeps bank items away from the in-app assistant.
- The integrity session runs as for any attempt. On close the app posts its assertion and, on an attested phone, platform evidence over the digest ticket 08 defines; when the role requires `device_attested`, each answer request carries a platform assertion.
- Disconnection: retry and resume at the same item inside the window, showing that the clock kept running; an abandoned attempt is explained with the retake rule.
- After close the app collects the organisation's credential through the existing signed fetch, or shows the refusal reason from ticket 09 in plain words.

## Acceptance criteria

- [ ] Rust test with a stub service: a hosted attempt's item text is absent from the profile database and logs afterwards.
- [ ] Rust test: no item request is sent before consent is accepted.
- [ ] Rust test: a refusal code maps to its message.
- [ ] fmt, clippy, `vue-tsc`, `npm run check:tauri-commands` pass.

## Pointers

- Request flow: `src/components/organisations/OrgRequests.vue`, `src-tauri/src/commands/assessment.rs`, exchange `src-tauri/src/commands/exchange.rs`; signed fetch prior art in [credential-trust/14](../credential-trust/14-collect-org-credentials.md); assistant guard: the item-bank guard used by the `llm` probe policy.
