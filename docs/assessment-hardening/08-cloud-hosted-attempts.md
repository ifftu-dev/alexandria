# 08 Hosted attempts

Triage: done
Type: task
Repos: alexandria-cloud
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: 07

Spec sections: *Hosted attempts*, *Probes* (hosted), *Timings*, stories 11–16, 19, 31, 32, 39–41, 43.

## What to build

The service runs the attempt and the device only shows it.

- An invited candidate opens a hosted attempt for a role with a hosted bank. Every request is signed by the candidate's key, like the existing signed fetches. Consent (answers and per-item timings go to the organisation, device requirement, retake rule, retention) must be accepted in the open request; it is recorded.
- The draw happens at open: random within skill and difficulty band, options shuffled per attempt, preferring items this candidate has not seen.
- One item at a time: question, options, time limit. The next item only after an answer. No response contains an answer key or correctness before the attempt closes. Exposure counters increment on send.
- The service stamps its own send and receive times per item; device-reported timings are stored beside them and decide nothing. The service grades; any score from the device is ignored.
- Probes: for each satisfied full-marks item the role's policy probes (at most two per attempt), the service sends another unseen item on the same skill with a fixed time limit from the item. A probe answered after its limit fails. A failed probe marks the run flagged.
- Disconnection: open for ten minutes after the last request; resume at the same item with its clock still running; after that the attempt closes as abandoned.
- Retakes: limit and wait set on the role, default one retake after seven days; abandoned attempts count.
- Only the invited candidate with an open attempt receives items; any other request is refused without revealing whether the role or bank exists.
- Attested requests: when the role requires `device_attested`, each answer request carries a platform assertion over the request; one without a valid assertion is refused. Use the vendored verifier's App Attest assertion check.
- At close the device posts its integrity assertion and, on an attested phone, platform evidence over a digest of the session's commitment root, the candidate and the attempt; the service stores both. Issuance is ticket 09.
- Answers and timings are kept with the run for its retention period and deleted with it.
- Document the endpoints (request and response shapes) in the Cloud API docs so ticket 10 can build the client from them.

## Acceptance criteria

- [x] HTTP tests: full lifecycle; no key or correctness in any candidate response; forged score and forged timings ignored; late probe fails and flags; resume inside the window, abandoned after it; retake limit and wait; non-invited and closed-attempt requests refused identically; unsigned or wrongly signed requests refused; attested-request refusal when required.
- [x] Retention deletion covers answers and timings.
- [x] Migrations, fmt, clippy pass.

## Pointers

- Signed fetch prior art: `GET /api/assessment-requests/for/{did}` in `src/http/assessment.rs`; App Attest checks in `vendor/alexandria-verify/src/attestation/`; retention in `src/http/api.rs`.

## Notes

2026-11-20: built in the Cloud repo (`0958d34`). Migration 0058 adds `hosted_attempts` and `hosted_attempt_items` (cascading with the run, so retention deletes answers and timings), retake columns on `role_definitions` (default one retake after seven days) and four audit actions. The candidate's signed routes live under `/api/hosted-attempts/for/{did}`: terms, open with consent by terms digest, resume, answers, close. Any refusal of service is one identical 404. Attested roles register an App Attest key at open and need an assertion over every answer and the close. Probes are unseen items on the same skill with the app's time-limit rule plus 2 s grace; a failed probe flags the attempt and run. Close stores the device's assertion and evidence with the close digest `SHA-256("alexandria-hosted-attempt-close-v1\0" || commitmentRoot || 0x00 || did || 0x00 || attempt id)`. Ticket 09 reads results through `hosted_attempts::closed`. The API is documented in the Cloud repo's `docs/hosted-attempts.md`; administrators set retakes through `PUT /api/roles/{id}/retakes` and on the bank page.

- Probes run when the policy names a probe policy other than `none` or requires `probed` or higher; at most two, placed at random. No unseen probe item left counts as `unavailable`.
- Retakes count per role and DID across invites. Close is refused until every item is answered; an answered attempt never closed lapses as abandoned.
- The candidate's signature does not cover request bodies, as with the existing signed fetches; attested roles have bodies covered by the App Attest assertion. Attested means iOS only.
- Run status is left alone, so the invite-expiry sweep would mark hosted runs `expired`; ticket 09 decides.
