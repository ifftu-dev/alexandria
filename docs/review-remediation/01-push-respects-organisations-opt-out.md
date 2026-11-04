# Push respects the organisations opt-out

Triage: ready-for-agent
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md
Release: next patch release, ahead of every other ticket in this spec

## Task

Make push registration eligible only while the push preference is on, organisations are on, and a Cloud origin is present. When organisations are off, push reconciliation behaves as it does with push disabled:

1. Retire routing first.
2. Mark registrations for deletion.
3. Send the existing signed DELETE, and keep it pending across restarts until Cloud acknowledges it.

The stored push preference never changes.

Changing the organisations setting triggers a push reconcile, just as changing the push setting does. Push status gains `paused_by_organisations`. Settings shows a translated explanation (all supported locales) that links to the organisations switch.

Covers spec stories 1–5.

## Acceptance

- With organisations off, no PUT reaches the fake Cloud server. An existing registration is deleted, and the deletion survives restart while offline until it is acknowledged.
- Turning organisations back on re-registers, provided push is still on.
- The push preference is untouched throughout.
- Rust module tests at the notifications interface (in-memory DB, fake Cloud server), a Vitest for the Settings status, strict frontend checks, and Rust fmt and clippy all pass.
- No Cloud change is needed. The existing DELETE endpoint is used.
