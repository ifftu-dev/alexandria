# Record shares on the existing grant mechanism

Triage: done
Repos: alexandria, alexandria-cloud
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/16-record-destinations.md

Spec: Sending records → Record shares; user stories 29 to 36, 42, 52, 60.

- A record share is a new scope on the existing access request and grant mechanism, with the same signed statements and the same create, sync, narrow and revoke operations. No second consent path.
- A request says which courses and record kinds it covers. The learner can approve a narrower scope. An organisation may propose the account identifier its systems use; the learner approves it or keeps the derived one.
- The device builds each batch and sends it to the organisation's hosted service with a signed request, through ticket 16's outbox.
- Revoking stops further sends.
- A supervised profile's record share goes through the same consent rules as any other grant for that profile.
- The wire structures are defined here for both repositories and duplicated on purpose, as elsewhere.

Done when: the grant format carries the new scope with test vectors both repositories check; host-seam tests show request, narrowed approval, batches built within scope, and revoke stopping sends.

## Notes

2026-11-20: decisions taken after reading the code, before work starts.

- The grant and request structures reject unknown fields and are signed over canonical JSON, and the hosted service uses a vendored copy of the verification crate pinned to an exact version. Adding the scope is therefore one coordinated change: a new version of the verification crate with the scope and its vectors, then the hosted service re-vendors it. The crate is not published from this ticket; the hosted service's vendored copy is refreshed from the local crate and the publish is left to the owner.
- The verification crate refuses a grant that names no skills. A record-only share needs that rule relaxed for grants that carry a record scope.
- No command narrows an existing grant today although the format allows it. This ticket adds the narrow operation for record scopes.
- No guardian rule exists on organisation grants today. "The same consent rules as any other grant for that profile" is therefore met by using the same path; this ticket adds no new guardian step, and says so in the consent screen test. A guardian approval step for grants is a separate decision for the owner.

2026-11-20: done. Decisions and deviations.

- Format. `alexandria-verify` 0.8.0 (version bumped locally, not published; the same version covers the record export module of ticket 15). `AccessRequest` and `GrantStatement` gain an optional `records` scope: `course_ids`, `kinds` (`completion`, `interaction`; completion is always included) and `account`. In a request the account is the organisation's proposal and optional. In a grant it is required and is the account the records will carry (derived, the learner's DID, or the approved proposal), so ingest can tie statements to the share. The field is omitted when absent, so older statements keep their bytes. A grant names skills, records or both. An update may narrow the scope and may not widen it or change the account. Shared vectors are in the crate's `tests/vectors/access/`; the hosted service runs them from its vendored copy.
- Recipient label. `share|<audience>|<request id>`, not `share|<org>`: it is known before approval, so the consent screen shows the exact identifier, and two shares with one organisation never share identifiers or collide on the unique label.
- App. The records half lives in `record_sending::shares` (offer, approval, approve, narrowed, narrow, stop, send_pending, the batch format and its transport); the grant commands call it. `org_grant` takes an optional records decision and sends no talent snapshot for a record-only share. `org_grant_narrow_records` is the new narrow operation, for record scopes only. Revoking deletes the share's recipient and queue before the revoke goes to the hosted service, so sends stop even offline. Shares are sent only for grants not revoked and only while the organisations switch is on. The background sender's pass serves destinations, then shares.
- Batch wire format. Duplicated in both repositories as the ticket says (`record_sending::shares` in the app, `record_batch` in the hosted service), not added to the verification crate. Shared vector: the app's `src-tauri/tests/fixtures/record-share/signed-batch.json`, copied verbatim to the hosted service's `tests/fixtures/record-share/`.
- Consent. The record consent screen gains the proposed account as a third identifier and the share's revoke text, and is shown on the access request card in the inbox; approval is the same `org_grant` call. No guardian step is added (the inbox test says so); a guardian approval step for grants remains the owner's decision.
- Left for ticket 18: the ingest endpoint `POST /api/grants/{id}/records` (until it exists, the app's sends fail visibly with 404 and are retried), per-statement scope checks, storage, replay refusal and deletion on revoke.
- Proposed doc edits, not made: the verification crate's README "New in 0.7.0" line, to mention 0.8.0; its vectors README, to list `access/`; the hosted service's `docs/organisations.md`, to describe record-only grants and `access_grants.records`; the spec's Sending records section, to name the grant's account as the identifier the records carry.

### Review (2026-11-21)

- **Owner decision needed: a share links the learner's DID to the per-recipient identifier.** Each record batch carries `subject_did` beside the statements' per-recipient account and is signed with the learner's key, and the grant itself is a statement the learner signs as their DID. The spec says no recipient can link the identifier to the DID (story 32); for shares that does not hold. Options:
  1. Sign batches with a per-share key derived from the learner's key and the share label, drop `subject_did` from the batch, and have the grant name that key. The grant statement still comes from the learner's DID, so the organisation can link the two through the grant unless the grant is reworked too; this narrows the link to the grant, it does not remove it.
  2. Accept it and amend the spec: a record share is a grant to an organisation that already knows the learner's DID, so the pairwise identifier only keeps two recipients from matching records; the "cannot link to the DID" promise holds for record destinations, not shares.
  Recommended: 2, unless the grant mechanism itself is to become pseudonymous. The share consent copy no longer promises otherwise (frontend review, ticket 16 Notes). No code changed for this item.
