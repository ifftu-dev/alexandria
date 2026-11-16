# 03 Suspension status list

Triage: done
Type: task
Repos: alexandria
Part of: alexandria/docs/credential-trust-spec.md
Blocked by: none

Spec sections: *Suspension in a status list*, stories 9–10, 40–43.

## What to build

Suspension is published in a Bitstring Status List of its own, so every verifier sees it.

- Every newly issued credential carries two `credentialStatus` entries: purpose `revocation` and purpose `suspension`, each a `BitstringStatusListEntry` into a list of that purpose for the issuer. The envelope's `credentialStatus` becomes a list (VC 2.0 allows one or many); accept the single-object form on input.
- Suspend sets the credential's bit in the suspension list, reinstate clears it, and the issuer's node clears it when a timed suspension's `until` passes (on the existing status-list publication path). Revocation stays permanent and separate.
- The suspension list is signed, published to the status host and carried in bundles exactly as the revocation list is. The Cloud host already accepts lists of purpose `suspension`.
- The verifier reads suspension from the suspension list bit only. Remove the store lookup for a locally recorded suspension from `alexandria-verify` and its callers. A missing or unreachable suspension list is a pending reason, as for revocation.
- The issuer's local suspension columns (reason, until) remain as bookkeeping; verification never reads them.
- Only the issuer may suspend or reinstate (unchanged; ADR 0003).

## Acceptance criteria

- [x] A suspended credential verifies as suspended from its published list alone, with no issuer database.
- [x] Reinstate and timed expiry clear the bit in the published list.
- [x] A credential whose suspension list is absent is `pending` with a suspension-list reason.
- [x] A credential with a single-object `credentialStatus` still verifies.
- [x] Vectors added for suspended, reinstated and suspension-list-missing.
- [x] Receivers of a credential by gossip or fetch see suspension from the list.

## Pointers

- Lifecycle: `src-tauri/src/credentials/lifecycle.rs`; lists: `src-tauri/src/credentials/status_list.rs`.
- Verifier: `crates/alexandria-verify/src/vc/verify.rs` (local suspension lookup), `vc/status.rs`, store trait in `crates/alexandria-verify/src/lib.rs`.
- Bundles: `src-tauri/src/credentials/bundle.rs`. Gossip: `src-tauri/src/p2p/vc_status.rs`.
- Cloud list host, for reference only: `alexandria-cloud/src/status_lists.rs`.

## Notes

2026-11-16: built.

- Envelope: `credentialStatus` is `CredentialStatuses` (one entry or a list), written back in the form read so a single-object credential keeps verifying. Issuance writes a list: revocation then suspension, the same index in both.
- Lists: per issuer per host, list number 1 is revocation and 2 is suspension (`urn:alexandria:status-list:<did>:<n>` without a host). Allocation stays keyed on the revocation list.
- Lifecycle: suspend sets the suspension bit and reinstate clears it, each bumping the list version only when the bit changes. `lift_expired_suspensions` reinstates due timed suspensions and runs before every publication pass (command and background loop). Suspend refuses a credential that names no suspension list; reinstating one only clears the bookkeeping. Suspend and reinstate now publish lists, as revoke does.
- Verifier: suspension is read only from the suspension entry's list bit; the store's local suspension lookup is gone. New pending reasons `suspension_list_missing` and `suspension_list_unavailable` replace `suspension_state_unavailable`.
- Exchange: a holder-issuer's own `IssuerState` reaches the verifier as the suspension entry's bit; an active suspension still rejects when the credential names no list, as revocation already did.
- Gossip: the status document carries a signed `status_purpose` (canonical bytes now `alexandria-status-list-v2`), so a gossiped suspension list is stored as one.
- Scoring fingerprint covers every list a credential's entries name, so a suspension list change recomputes scores.
- Vectors 13 (suspended), 14 (reinstated), 15 (suspension list missing); the runner now compares `suspended`. Vectors 07 and 11 keep the single-object form.

2026-11-16 (review fixes): `lift_expired_suspensions` continues past a credential it cannot lift and returns a `LiftReport` (lifted count and one line per failure), which both callers log; a list the credential names but the store does not hold is `StatusListNotHeld`, not `NotIssuer`. A gossiped status list is applied only when its `list_id` names the signing issuer (hosted URL or URN form) and never over a row held under another issuer.

2026-11-16 (merge): vectors renumbered to 16 (suspended), 17 (reinstated) and 18 (suspension list missing), because ticket 02 took 13–15. Criteria checked against vectors 07, 11 and 16–18, the gossip test `a_suspension_list_arrives_as_a_suspension_list` and ticket 14's collection tests.
