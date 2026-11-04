# Notification domain types and vocabulary

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md
Blocked by: alexandria/docs/review-remediation/01-push-respects-organisations-opt-out.md, alexandria/docs/review-remediation/02-profile-bound-notification-events.md

## Task

- **Domain types.** Move the notification row, push status and notification-open types into the domain module as snake_case types in both Rust and TypeScript. Event payload keys follow. This is IPC only: the Cloud wire contract is unchanged.
- **Vocabulary.**
  - Generate notification kinds and sources from the existing vocabulary macro, including the TypeScript union.
  - Type the row's kind and source.
  - Handle unknown kinds arriving from outside at the boundary, by rejecting them or mapping them to the generic kind.
- **Interview-room route.** Build it in one function, shared by the inbox and Cloud discovery.
- **Permission request.** Return a typed outcome (granted, denied or prompt-unavailable), and make its documentation match.
- **README.** Fix the mobile push plugin README: iOS returns an FCM token derived from the APNs token.

Covers spec stories 17–22.

## Acceptance

- The vocabulary snapshot test covers notification kinds.
- No camelCase notification fields remain on either side.
- Strict `vue-tsc` passes with `kind` typed to the generated union.
- Rust and Vitest suites pass.
