# Profile-bound notification events

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

Recording and announcing a notification take the profile session from the lease of the job that produced it. Nothing reads the session at send time. The new-event payload, the unread-count payload and the OS banner's profile tag all use the captured session.

If the lease is no longer current when the announcement would go out, the announcement and banner are dropped, and the row stays in its own profile's store. Mark-read, mark-all-read and clear emit their count update under the lease they ran with.

Covers spec stories 6–8.

## Acceptance

- A test switches profile between recording and announcing. No event or banner reaches the new profile, and the row exists only in the original profile.
- Count updates after mark-read or clear carry the acting profile's session, even if a switch follows straight after.
- Rust module tests and the notifications composable Vitest pass.
