# 05 Answer custody and required rung in the app

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: 02

Spec sections: *Answer custody*, *Required rung*, stories 22, 33, 38.

## What to build

- Every integrity assertion the app builds states answer custody `device`. The app never writes `issuer`.
- Role and request issuance policies in the app carry the required rung and required answer custody, judged through the shared rule. A role created on a sponsor's own device defaults to custody `device` (nothing else is possible there) and its form states that candidates' devices hold the answers.
- A role that requires `live` waits for the existing live segment: role credential issuance on a sponsor's device succeeds once the panel has marked a live segment done for that candidate and role, and refuses with the rung code before that.
- Credential views show answer custody and, below `live`, that a second device or person was not ruled out (from the coverage statement).

## Acceptance criteria

- [x] Test: a built assertion carries custody `device` and verifies under the library.
- [x] Test: sponsor-device issuance refuses below the required rung and succeeds once the rung is met, including `live` through a marked live segment.
- [x] fmt, clippy, `vue-tsc`, `npm run check:tauri-commands` pass.

## Pointers

- `src-tauri/src/credentials/issue.rs` (`build_integrity_assertion`, `IssuancePolicy`), `src-tauri/src/commands/role_assessment.rs`, `src-tauri/src/commands/hiring.rs` (live rung), credential trust surfaces from [credential-trust/15](../credential-trust/15-trust-surfaces.md).

## Notes

2026-11-20: built.

- Custody: `build_integrity_assertion` writes `answer_custody: Some(Device)` on every assertion; the app never writes `issuer`. A self-issued credential states `answerCustody: "device"` and the library accepts it with no custody defects.
- Roles: `create_role_assessment_impl` stores `required_answer_custody: device` on every role made here (a role with no policy now gets one carrying only that) and refuses `issuer`, since candidates' devices hold the answers on this path. The role form says so (`policy-custody`). The required rung is the existing `min_verification_rung`, already in the form and judged by the shared rule.
- Requests: the request card lists `issuer` custody first, as the shared rule judges it ("take it as an attempt on their service"); `device` asks for nothing. Request policies were already the library type, so `custody_below_required` and the rung code reach the picker unchanged.
- `live`: sponsor issuance refuses with `rung_below_minimum` at `process`, `probed` and `consistent`, and issues once a satisfied live segment (step `interview`, policy `panel`) is in the attempt's session. Gap, not changed: `record_live_segment` records only into an interview-purpose session, so the device flow has no way to put a live segment into a role attempt's session; the test records the same verification directly. A role requiring `live` therefore cannot issue from a real interview yet. Closing it means linking an interview to a role attempt, which the spec ruled out ("nothing is added to interviews"); left for a decision.
- Companion reconciled with ticket 02: `credits_attestation_signal` is now `device_attested` only, so a paired phone no longer credits the desktop's device attestation signal. A companion session cannot select `attested` or `full`, an `attested` floor is unmet in the app as in the library (test in `sentinel::companion::session`), and the assertion states `companion_attested` on its own profile. Interview checks outside Alexandria still accept a paired phone (`attested_phone_present`). The request card no longer offers pairing a phone to meet an attestation floor (the `pairPhone` string is removed).
- Credential view: an "Answers held by" row (a missing custody reads `device`) and, above the coverage statement, the statement's own line that a second device or person was not ruled out (`utils/sentinel/secondDevice.ts`; absent at `live` and for statements before version 3).
- Tests: the session fixtures `session_on_profile`, `assessed`, `verified` and `scored_with_history` moved from the issuance tests to `credentials::fixtures` for the role test; `scored_with_history` now uses the attempt's own subject.
- New strings English-only, listed pending in `check-parity.mjs`.
- For ticket 12: `docs/sentinel.md` still says a desktop reaches `device_attested` or the `attested` profile through a companion phone (platform table, the `attested` profile row, *Integrity Assurance*, *Companion phone*), and nothing documents answer custody in the credential view or role form.
- Checks: fmt, workspace clippy `-D warnings`, `vue-tsc`, `check:tauri-commands`, i18n parity and `check-docs.sh` pass. The full workspace and Vitest runs fail only on load-sensitive tests that pass alone: iroh `offline_peer_fetch`, Vitest `AssessmentRunner` follow-up probes, and on the integration tip, one run each of `notifications::push::boundary_tests::a_put_response_after_preference_changes_cannot_publish_a_recipient` and `cardano::blockfrost::tests::total_request_limit_applies_to_submit_and_query`. The 02+03 merge needed no fix.
