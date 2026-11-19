# The record store's statements resource

Triage: ready-for-agent
Repos: alexandria-cloud
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/18-cloud-ingest.md

Spec: Record store (hosted service); user stories 55 to 57, 62 to 67.

- Each organisation has its own endpoint base and its own interface credentials with read or write scope, created and revoked in the console.
- The statements resource for xAPI 1.0.3 and 2.0: storing single and multiple statements, retrieval by identifier, query filters, paging, voiding and attachments, with the standard's version negotiation and error responses.
- Queries use the indexed filter columns. Tenant isolation is enforced in the data layer, not only at the route.
- Statements written by an organisation's own tools are that organisation's data and are not subject to a record share.
- One access-log entry per query and per learner whose records it returned, written in a single batch.

Done when: integration tests cover each operation in both versions, a credential of one organisation never reading another's records, read and write scope, and the access-log batching; the statements portion of the official conformance suite passes locally.
