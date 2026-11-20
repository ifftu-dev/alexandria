# 10 Hosted attempts in the app

Triage: done
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

- [x] Rust test with a stub service: a hosted attempt's item text is absent from the profile database and logs afterwards.
- [x] Rust test: no item request is sent before consent is accepted.
- [x] Rust test: a refusal code maps to its message.
- [x] fmt, clippy, `vue-tsc`, `npm run check:tauri-commands` pass.

## Pointers

- Request flow: `src/components/organisations/OrgRequests.vue`, `src-tauri/src/commands/assessment.rs`, exchange `src-tauri/src/commands/exchange.rs`; signed fetch prior art in [credential-trust/14](../credential-trust/14-collect-org-credentials.md); assistant guard: the item-bank guard used by the `llm` probe policy.

## Notes

2026-11-20: built.

- Client: `assessment::hosted` holds the candidate API (`HostedService`: terms, open, resume, answer, close, result), every request signed like the other holder fetches, with the wire structs copied from the Cloud service in `hosted::wire`. The terms digest is recomputed (JCS) and must match, so what is shown is what is accepted; the open request is refused locally, before anything is sent, without acceptance. Commands `hosted_attempt_terms`, `_open`, `_resume`, `_answer`, `_close`, `_result` (`commands/hosted_attempt.rs`) reject with `hosted_attempt:<code>: <message>`; the verified-device refusal keeps its own code.
- When: the request card's assess button runs a hosted attempt when the request requires `issuer` custody (shown even without `require_new_assessment`), on the new page `pages/learn/HostedAttempt.vue`: terms and consent (the service's statements, attempts used, device requirement, camera gate), then one item at a time with the service's remaining time, then the result.
- Item text: held in the page's memory while shown and dropped on answer; `wire::Item`'s `Debug` leaves the text out; each item received is registered with the assistant's bank guard (`bank_guard::hold_hosted_item`), which keeps only shingle hashes, for the rest of the run, of the question alone and with its options.
- Integrity: monitoring starts under the request's floor (`exchange_prepare_assessment` first, as for a request attempt); close ends it and posts the session's assertion with, for an attested session, `DeviceAttestationEvidence` with a credential binding whose proof covers the close digest, from this phone or the paired phone (`PairedAttester`). Attested roles register an App Attest key over the open request's nonce and assert every answer and the close (`native::assert_bytes`).
- Disconnection: an answer that fails to reach the service retries every 3 s, resumes, resends the answer if the service is still on that item, and then says the clock kept running; `not_found` on resume shows the abandoned explanation with the retake rule. A close that fails reads the result first and closes again only if the service has not closed.
- After close: an issued credential is collected through the existing signed fetch (`credentials::collect::run`) before the reply returns; a refusal is explained by `hosted::refusal::explain` (plain words and whether a retake could help); `held` offers to check again.
- Tests (seams): the client against a stub hosted service with an in-memory profile database (whole attempt: no item text in any table or captured log line, and the bank guard knows it; no request reaches the service before consent), and `refusal::explain` as a pure function; a Vitest for the error-code helper.
- Deviation: refusal messages are composed in the backend in English (the ticket asks for the mapping in Rust), so they are not in the locale files; the new page and button strings are English-only and listed pending in `check-parity.mjs`.
- Deviation: an attested attempt's App Attest key id is held in memory, so after the app restarts an attested attempt cannot be resumed (its answers would be refused for a missing assertion) and lapses as abandoned.
- Not done: the page shows multiple-choice items only; plugin items are ticket 11. The device's per-item timings sent are only `shown_ms`.
