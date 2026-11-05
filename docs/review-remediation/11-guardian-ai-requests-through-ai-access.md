# Guardian AI requests through the AI access module

Triage: done
Repos: alexandria
Part of: alexandria/docs/review-remediation-spec.md

## Task

Give the AI access module guardian-side operations:

- record an inbound request
- list requests for the guardian overview
- record a guardian decision
- mark a request delivered

The guardian commands and the P2P guardian handler call these operations. No SQL on guardian AI requests remains outside the module.

Covers spec stories 63–64. This completes deepening ticket 03.

## Acceptance

- The module's tests round-trip each operation against an in-memory DB.
- A decision made through one guardian link can't touch another link's request.
- A grep for the guardian AI request table outside the module (and its schema) finds nothing.
- The command/direction guard script passes.

## Completion evidence — 2026-11-04

What changed:

- `ai_access` gains the guardian side: `record_ward_request` (inbound request, idempotent), `ward_requests` (overview), `record_guardian_decision`, and `undelivered_decisions` with `mark_delivered` for the guardian sync. `GuardianAiRequest` moves into the module, unchanged on the wire.
- `commands/guardian.rs` (overview, decide, decision delivery) and `p2p/guardian.rs` (inbound request) call these. The P2P handler still checks the link key and payload bounds before storing.
- A decision changes only the request on the link it names, and only while this profile is that link's active guardian.

Red → green:

- `ai_access::tests::a_guardian_receives_decides_and_delivers_a_wards_request`: repeats store one request; a decision is listed and undelivered; a decision changed in flight is not marked delivered.
- `ai_access::tests::a_decision_through_one_guardian_link_never_touches_another_links_request`: two wards share a request id; deciding or delivering on one link leaves the other pending; ward-side and revoked links decide nothing.
- Both failed to build before the operations existed and pass now.

Grep: `guardian_ai_requests` appears outside `ai_access.rs` only in its schema (`db/ai_connection_approvals.sql`), the device-sync privacy invariant's table list in `p2p/guardian.rs`, and the schema doc test.

Checks: `cargo test -p alexandria-node --lib -- ai_access guardian` (31 passed), `--test guardian_e2e` (3 passed), `cargo fmt --check`, `cargo clippy -p alexandria-node --all-targets -- -D warnings`, `npm run check:tauri-commands` and its tests, `scripts/check-docs.sh`.

Deviations:

- One more operation than listed: `undelivered_decisions`, the read the sync needs before marking delivered.
- The P2P handler's own test counts requests through `ward_requests` instead of SQL.
