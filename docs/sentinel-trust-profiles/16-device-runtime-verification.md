# Device runtime verification

Triage: ready-for-human
Repos: alexandria
Part of: docs/sentinel-trust-profiles-spec.md
Blocked by: docs/sentinel-trust-profiles/12-desktop-device-axis.md

Needs the owner. Steps: [sentinel-runbook.md](../sentinel-runbook.md) Procedure 5.

- **App Attest:** enabled on the app identifier; nothing left to configure.
- **iOS checks, on the first TestFlight build that carries ticket 12:** an iPhone reads `device_attested`; a desktop paired with that iPhone reads `device_attested (phone)` with the `attested` profile, and its credential carries `assuranceLevel: device_attested`. Negative checks: a phone signed in as another learner, an expired code, and an unpaired session (stays `core` / `local`).
- **Android, on hold:** link a Cloud project for Play Integrity, add the response keys to the preprod network profile, then verify `device_attested` on an Android device.
