# Record shares on the existing grant mechanism

Triage: ready-for-agent
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
