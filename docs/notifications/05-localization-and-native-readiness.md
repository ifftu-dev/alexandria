# Finish localization and native configuration behavior

Triage: ready-for-agent
Repos: alexandria, alexandria-cloud
Part of: alexandria/docs/notifications-spec.md
Blocked by: alexandria/docs/notifications/03-profile-isolation-and-opt-out.md, alexandria/docs/notifications/04-discovery-and-inbox.md

## Task

Translate all notification locale resources and any new recipient/cleanup states into the eight non-English supported languages. Localize app-generated and Cloud provider generic copy using selected/registered locale with English fallback; retain a consistent app/Cloud vocabulary without personal data. Finish native configuration handling so builds without Firebase project files remain usable and report push unavailability. Inspect existing native patches for build compatibility, duplicate foreground banners and cold-start taps. Never invent or commit credentials, and do not provision/deploy services.

Acceptance: actual translations and key parity, app/Cloud payload contract checks, focused tests and available native builds pass; unavailable deployment/device prerequisites are explicitly recorded for final validation.

## Implementation workflow

Read the parent spec and applicable AGENTS instructions. Use the TDD skill at the seams already confirmed by the user. Start an isolated implementer branch from the appropriate integration branch; confirm ancestry before editing. Merge the integration tip before reporting completion. Return commits, red/green evidence, tests and unresolved blockers; the parent arranges merger review. Tracker updates are authorized by the invoked implement-spec workflow.

## App implementation evidence

Status: App portion implemented and validated; parent merger owns combined closure after integrating this app portion. Cloud localization has merged.

- Eight non-English notification catalogs are translated, including pending cleanup and registration. All ten app kinds use generic copy; completed guardian links are described as completed. Native recording and webview rendering share the catalogs and use English fallback. The five Cloud kinds match the versioned localization contract for all nine languages, including normalized locale variants. Translations are machine-drafted, not native-speaker-certified.
- Android token acquisition without a default Firebase app now rejects with actionable configuration guidance instead of throwing. The real native command regression test runs in mobile CI. Existing recipient isolation, cold taps, iOS fallback and foreground presentation are retained.
- iOS Push Notifications capability and APNs entitlement are wired into the native project. Distribution CI requires the matching App ID capability and production APNs entitlement in its provisioning profile. No credentials, provisioning or deployment were performed.
- Validation passed: 33 Rust notification tests, nine mobile-push plugin tests, 104 targeted frontend tests, Android native command test, strict TypeScript, locale parity, Rust formatting and Rust 1.91 app/plugin Clippy with warnings denied. The plugin documentation example remains ignored. Both integrated native builds passed without Firebase project files: Android arm64 debug APK/AAB with NDK 26.3 and JDK 21; iOS arm64 simulator with Xcode 27, Rust 1.91 and tutoring video enabled.
- Real-device delivery remains unverified. Final release acceptance needs matching Firebase app configuration, Firebase/APNs and Cloud sender configuration, push-enabled signing/provisioning, and connected iOS/Android devices for permission, delivery, tap, token rotation and offline opt-out checks. Simulator builds do not establish delivery readiness.
