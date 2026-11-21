# 03 Companion level and the verified-device requirement in the app

Triage: done
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

- [x] Rust test: a companion-backed session's credential states `companion_attested`.
- [x] Rust test: scoring weights a `companion_attested` self-issued credential as a plain self-claim.
- [x] Rust test: starting an attempt for a role that requires `device_attested` on a desktop is refused before any item is drawn; on mobile it proceeds.
- [x] No UI string claims pairing raises trust (grep the locale files).
- [x] fmt, clippy, `vue-tsc`, `npm run check:tauri-commands` pass.

## Pointers

- Companion: `src-tauri/src/sentinel/companion/`, `src-tauri/src/sentinel/attestation/`; issuance `src-tauri/src/credentials/issue.rs`.
- Weights: `src-tauri/src/aggregation/config.rs` (`attested_weight`), `aggregation/store.rs`.
- Attempt start: `src-tauri/src/commands/assessment.rs`, `commands/role_assessment.rs`; pairing copy: `src/composables/useSentinel.ts` and the locale files.

## Notes

2026-11-20: built.

- A paired phone's session is `companion_attested`: `accept_hello` stores it on the session row and `effective_assurance` returns it at issuance, so the integrity assertion states it. The phone still credits the trust profile's device attestation signal (`credits_attestation_signal`), so profile selection, floors and the rung behave as before.
- Weights: only `device_attested` evidence takes `attested_weight` and makes session status count; `companion_attested` weighs as a plain self-claim. Calculation version `1.5`.
- Provenance: a paired phone's credential reads self-reported with the companion outcome; the credential view, its integrity line and the CLI say a phone was present and nothing is claimed about the computer. Pairing copy no longer says it makes an attempt device-attested; the offer and the paired state say what pairing is.
- Refusal: the stable error code is `verified_device_required` (`sentinel::attestation::VERIFIED_DEVICE_REQUIRED`), sent as `verified_device_required:<iphone|phone>: <message>` (`phone` once Android attestation is enabled). `require_verified_device(required, AttemptDevice)` judges it: only a required `device_attested` refuses, on a desktop or on Android without Play Integrity keys. Attempt start runs it for the role (its column, else its policy) or the request policy, before anything is drawn and on resume. The UI reads it with `src/utils/verifiedDevice.ts` (`verifiedDeviceRefusal`); `AssessmentRunner` shows "take this on your iPhone" in the chooser and blocks the start, and `RequestPolicy` shows it on the request card. `AttemptRoleTarget.verified_device_required` and `DeviceSupport.verified_device` / `android_attestation` let the screens say so before the candidate starts.
- Deviation: an Android phone is refused too while Android attestation is off, since it cannot reach `device_attested` either.
- Deviation: interview checks by video call, phone or in person still accept a paired phone, and the signed report now states `companion_attested`. The Cloud's interview report intake accepts only `local` and `device_attested`, so it needs a follow-up to accept the new value.
- Changed pairing strings render in English until the next translation pass; their stale translations were removed (`check-parity.mjs` lists them as pending).
- Not changed: `docs/skills-and-reputation.md` still names calculation version `1.4`, and the glossary's companion phone entry is unchanged; both are left for ticket 12.
