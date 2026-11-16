# 15 Trust surfaces

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 03, 06, 09

Spec sections: *Surfaces* (app), stories 21–22.

## What to build

- The credential detail view and the bundle view state, in plain words, exactly one of: **Self-reported**, **Device-attested and verified**, or **Issued by {organisation name} ({domain})**, plus the suspension state and the resolved coverage statement.
- Wording is honest about limits: self-reported says the figures come from the holder's own device; device-attested says what attestation proves and does not (a genuine app on a genuine device, not who was present).
- The data comes from the credential trust command (attestation outcome from ticket 05, backing from ticket 09) and the status lists (ticket 03); the frontend does no verification of its own.
- English strings go through the existing i18n setup; add keys, leave other locales to translators.
- Domain structs and TypeScript types change together.

## Acceptance criteria

- [x] `vue-tsc -b --noEmit` and the composable vitest suite pass.
- [x] Rust tests for the trust command cover each of the three states and the suspended state.
- [x] `npm run check:tauri-commands` passes.

## Pointers

- Credential detail and bundles: grep `integrity_coverage_statement` and `CredentialDetail` under `src/`.
- Trust command: `get_credential_trust` in `src-tauri/src/commands/`.
- Types: `src/types/index.ts` mirrors `src-tauri/src/domain/`.

## Notes

2026-11-16: built.

- `get_credential_trust` returns `CredentialTrustView` (`domain::credential_trust`, mirrored in `src/types/index.ts`): `trust` (the classification under the default policy, unchanged), `provenance`, `issuer` (the signer when not the subject), `organisation` (backing when judged), `suspension` and `coverage_statement` (resolved from the verifier library registry). `credentials::trust::trust_view` builds it from a verification store and attestation trust inputs; the frontend does no verification.
- Provenance is exactly one of `organisation { name, domain }` (backing verified, with the descriptor's name and domain), `device_attested { platform }` (verified credential-bound evidence) or `self_reported { attestation }`, in that order. `self_reported` carries the attestation outcome so the view can say when evidence covered only the session or could not be checked here (story 22). It is `null` while the credential is invalid for another reason or pending.
- Decision: the credential is verified once with `reject_suspended: false`; `trust` is classified under the default policy and `provenance` with suspension set aside, so a suspended credential still states where it came from while `suspension` says it is suspended. Suspension only changes the acceptance decision, so one result serves both.
- Decision: an issuer-signed credential whose key no organisation lists reads self-reported, with a "Signed by {issuer}" line, and a "key no longer listed" line when the domain withdrew the key (signature still verifies).
- `SuspensionState`: `not_suspended`, `suspended`, `unknown { reason }` (suspension list missing or unavailable) or `no_list` (names no suspension list).
- `credential_trust_impl` takes the attestation trust inputs (the command passes `profile_trust_inputs()`), so tests use the synthetic Play keys of `scoring_inputs::test_support`.
- Detail view: the provenance section shows the headline, what it proves and does not, the suspension state and the coverage statement (moved from the integrity section; the page no longer resolves it itself). The proof panel gains a Suspended check that reads pending while the suspension list is missing. New keys under `credentials.detail.trust*` and `credentials.detail.suspended`, English-only and listed pending in `scripts/i18n/check-parity.mjs`. Unused `trustState.selfClaim|issuerSigned|courseEndorsement` and `trustDetail.issuerSigned` removed.
- Bundle view: the app has no bundle screen, so the bundle view is offline verification. `OfflineVerification` gains `trust` (one view per credential, offline with no endorsement evidence or descriptor), and the CLI's offline verify (command and TUI) prints one line per credential: provenance and suspension.
