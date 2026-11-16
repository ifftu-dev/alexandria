# 15 Trust surfaces

Triage: ready-for-agent
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

- [ ] `vue-tsc -b --noEmit` and the composable vitest suite pass.
- [ ] Rust tests for the trust command cover each of the three states and the suspended state.
- [ ] `npm run check:tauri-commands` passes.

## Pointers

- Credential detail and bundles: grep `integrity_coverage_statement` and `CredentialDetail` under `src/`.
- Trust command: `get_credential_trust` in `src-tauri/src/commands/`.
- Types: `src/types/index.ts` mirrors `src-tauri/src/domain/`.
