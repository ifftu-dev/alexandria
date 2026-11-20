# 03 Companion level and the verified-device requirement in the app

Triage: ready-for-agent
Type: task
Repos: alexandria
Part of: alexandria/docs/assessment-hardening-spec.md
Blocked by: 01

Spec sections: *Companion-attested*, *A role that requires a verified device*, stories 9, 10, 23.

## What to build

- A desktop integrity session backed by a companion phone states `companion_attested`, and issuance builds credentials with it.
- A self-issued credential with `companion_attested` takes the plain self-claim weight; the attested weight applies only to `device_attested`. Advance the derived-state calculation version.
- Remove every statement in the app that pairing a phone raises a credential's trust. Pairing stays available and the session describes what it is.
- When a role's or request's policy requires `device_attested`, starting the attempt on a desktop is refused before any item is shown, with a stable error the UI turns into "take this on your phone" (an iPhone until Android attestation is enabled).
- The invite or request screen shows the device requirement before the candidate accepts.

## Acceptance criteria

- [ ] Rust test: a companion-backed session's credential states `companion_attested`.
- [ ] Rust test: scoring weights a `companion_attested` self-issued credential as a plain self-claim.
- [ ] Rust test: starting an attempt for a role that requires `device_attested` on a desktop is refused before any item is drawn; on mobile it proceeds.
- [ ] No UI string claims pairing raises trust (grep the locale files).
- [ ] fmt, clippy, `vue-tsc`, `npm run check:tauri-commands` pass.

## Pointers

- Companion: `src-tauri/src/sentinel/companion/`, `src-tauri/src/sentinel/attestation/`; issuance `src-tauri/src/credentials/issue.rs`.
- Weights: `src-tauri/src/aggregation/config.rs` (`attested_weight`), `aggregation/store.rs`.
- Attempt start: `src-tauri/src/commands/assessment.rs`, `commands/role_assessment.rs`; pairing copy: `src/composables/useSentinel.ts` and the locale files.
