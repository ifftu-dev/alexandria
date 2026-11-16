# 14 Collect organisation-issued credentials

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: 13

Spec sections: *Organisation-issued credentials from the hosted service* (candidate collection bullet), story 27.

## What to build

- The app's existing holder pull from the Cloud origin also calls `GET /api/credentials/for/{did}` (ticket 13), verifies each credential with the library (including organisation backing via the TXT lookup), and stores accepted ones as received credentials. Rejected or pending ones are not stored as accepted; pending follows the existing pending-verification queue.
- Idempotent: fetching the same credential twice stores it once.

## Acceptance criteria

- [x] Tests with a stubbed HTTP response: valid org-issued credential stored; tampered one rejected; repeat fetch is idempotent.
- [x] The credential then appears in the credential list with organisation provenance.

## Pointers

- Holder pull: `src-tauri/src/commands/holder_pull.rs`.
- Received credential storage: `src-tauri/src/p2p/vc_did.rs` (`promote_pending_for`) and `src-tauri/src/credentials/store.rs`.

## Notes

2026-11-16: built.

- `credentials::collect`: `fetch` makes the signed `GET /api/credentials/for/{did}`, decodes each `credential` under the single-credential limits (an unreadable item is reported, not fatal), and fetches the status lists the credentials name. `store` applies those lists, then verifies each credential through `credentials::receive`, the import path's verify-then-store moved out of `commands::import` (which now wraps it). Each credential ends `stored`, `already_present`, `pending` (queued in `credentials_pending_verification`, retried on the next pull, removed once decided) or `rejected`. Verified ones report organisation backing from the descriptor the TXT lookup cached; backing never decides storage.
- Where it runs: the profile's background pass to Cloud (`notifications::cloud_pull`, also run when a phone returns to the foreground) collects beside its notification lists. `run` refreshes the descriptor cache once per issuing domain first. No new Tauri command.
- Decision: a collected credential must have the learner as subject; any other is rejected.
- Decision: status lists are fetched only from the Cloud origin and only at a path naming the credential's own issuer, so a credential cannot send the app to another host or overwrite another issuer's list. A list is applied when `verify_fetched_list` accepts it; its `validFrom` orders versions (an older copy is ignored) and a row held under another issuer is never overwritten. A list that cannot be fetched leaves the credential pending.
- The Cloud-only `AssessedCredentialEvidence` entry is opaque to the verifier: a credential carrying it verifies, is stored, and reads as issued by the organisation in the trust view. No verifier library change was needed.
- `p2p::vc_did::queue_pending` takes a connection; `unqueue_pending` added. `org_trust::refresh_descriptor_for` serves callers that hold an executor and a lease.

2026-11-16 (review fixes): `apply_list` never writes a list the profile itself issues, and orders fetched copies by their signed `validFrom` against the stored copy's `valid_from` (migration 42), not against the `version` counter; a held row with no `valid_from` (from gossip or a bundle) takes the issuer-signed fetched copy. `receive` answers a credential already stored before verifying it, and its rejection reasons come from `bundle::rejection_reasons` (now naming a mismatched coverage statement and invalid attestation with its code). `refresh_descriptor_for` takes the issuer keys and refetches once when the cached descriptor does not list one (shared with `org_trust::check`). `AssessedCredentialEvidence` is now the verifier library's `assessed` type. A queued credential is promoted by a DID announcement only through `receive`.
