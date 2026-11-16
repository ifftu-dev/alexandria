# 14 Collect organisation-issued credentials

Triage: ready-for-agent
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 13

Spec sections: *Organisation-issued credentials from the hosted service* (candidate collection bullet), story 27.

## What to build

- The app's existing holder pull from the Cloud origin also calls `GET /api/credentials/for/{did}` (ticket 13), verifies each credential with the library (including organisation backing via the TXT lookup), and stores accepted ones as received credentials. Rejected or pending ones are not stored as accepted; pending follows the existing pending-verification queue.
- Idempotent: fetching the same credential twice stores it once.

## Acceptance criteria

- [ ] Tests with a stubbed HTTP response: valid org-issued credential stored; tampered one rejected; repeat fetch is idempotent.
- [ ] The credential then appears in the credential list with organisation provenance.

## Pointers

- Holder pull: `src-tauri/src/commands/holder_pull.rs`.
- Received credential storage: `src-tauri/src/p2p/vc_did.rs` (`promote_pending_for`) and `src-tauri/src/credentials/store.rs`.
