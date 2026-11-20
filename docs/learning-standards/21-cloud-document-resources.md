# The record store's document and descriptive resources

Triage: done
Repos: alexandria-cloud
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/20-cloud-statements-resource.md

Spec: Record store (hosted service) → second bullet; user story 65.

- Activity state, activity profile and agent profile documents with the standard's concurrency control, for both versions.
- The agents, activities and about resources.
- The same credentials, scopes and tenant isolation as the statements resource.

Done when: integration tests cover create, read, merge, delete and concurrency conflicts for each document resource, and the matching portions of the official conformance suite pass locally.

## Notes

2026-11-21: done. Decisions, deviations and open questions for the owner.

- Resources. Under each organisation's endpoint base: `activities/state`, `activities/profile`, `agents/profile` (GET, HEAD, PUT, POST, DELETE), `activities` and `agents` (GET, HEAD) and `about`. Same interface credentials as statements: read scope for GET, write scope for the rest; another organisation's credential gets 403. `about` answers without a credential or version header and names `1.0.3` and `2.0.0`.
- Storage. One table for the three document kinds (migration 0061), keyed by organisation, kind, activity, agent, registration and document id, with row-level security like every other record store table; all access goes through one tenant-bound data layer. Any content type is stored as sent and returned with it. An agent is matched by object type and identifier, so two spellings of one agent name the same documents. Bodies up to 4 MiB.
- Concurrency. The ETag is the quoted SHA-1 of the content (the 1.0.3 suite requires SHA-1). `If-Match` (an ETag or `*`) and `If-None-Match` (`*` or ETags) are checked on PUT, POST and DELETE; a failed precondition is 412 and changes nothing. A PUT with neither header over an existing document is 409 for the profiles in both versions and for state in 2.0 only (1.0.3 state PUT overwrites). In 1.0.3 a profile PUT with neither header is 400 even when nothing is stored yet (the 1.0.3 suite checks this); 2.0 asks for a precondition only over an existing document. Writes to one document are serialised so a checked precondition still holds when the write lands.
- Merge. POST over an existing document merges top-level properties when both are JSON objects stored and sent as `application/json`; anything else is 400 and changes nothing. POST with nothing stored stores the document.
- Listing and deleting. GET without `stateId`/`profileId` lists ids, optionally changed after `since`. DELETE without `stateId` removes every state of the activity, agent and registration; profiles are deleted only by id. Unknown or repeated parameters are 400, as on the statements resource.
- Activities. There is no activity registry; the canonical activity is built from the readable statements that name the id (object, context activities, sub-statement), the most recent 1000, merged in stored order: name, description and extensions merge key by key, other properties are replaced. An id never stored returns `{objectType, id}`. Not access-logged: a definition is course content, not a learner's data.
- Agents. A Person object with the agent's own identifier and every name stored for it (the parameter's name included). The store links no identifiers, so a Person holds one. A read that drew on a learner's shared records is written to that learner's access log, one entry per read, as statement queries are. A group is refused.
- Retention, revoke and enrolment end (decision). Documents arrive only through the interface; record shares carry statements, never documents. So documents are the organisation's own data, like statements its own tools write: they follow the store's 180-day retention from their last change (swept with the rest) and are not removed when a share is revoked or an enrolment ends. Owner question: an organisation's tool could key state or a profile to the pseudonymous actor a learner's shared records use; if such documents should go with the share, the revoke and enrolment-end paths would also delete documents whose agent is that actor.
- Conformance (ADL lrs-conformance-test-suite at upstream commit 5bc232d, whole suite, local server and Postgres, same recipe as ticket 20): 1.0.3 1365 passed, 0 failed; 2.0 1435 passed, 0 failed. This includes the 2.0 IRI test ticket 20 left open.
- Deviations: the 1.0.3 alternate request syntax is served for statements only, not for the document resources (the suite does not exercise it there); a DELETE of all states with `If-Match`/`If-None-Match` is refused with 400, since a precondition names one document.
- Method: each slice was test-first at the record store seam (HTTP against Postgres), red before green. The isolation and scope tests passed on their first run, because they exercise the credential check and row-level security the new routes reuse from ticket 20.
- Not done (still open from ticket 20): CORS and rate limiting for `/xapi/`; the console shows no documents.
- Proposed doc edits, not made: the hosted service's `docs/organisations.md` "Record store" section (proposed by ticket 20) should also list the document, agents, activities and about resources, the 4 MiB document limit, that `about` is open, and that documents follow the retention from their last change and are not covered by a share; the spec's Record store section could state that documents are the organisation's own data.

### Review (2026-11-21)

- The about handler is `xapi_about`, so the route scanner's open-route exemption names it unambiguously; it answers the version list from `XapiVersion::served()`.
- `/xapi/` is now rate limited (ticket 20), which closes that part of the "still open" bullet above; CORS is still open.
