# Hosted ingest of shared records

Triage: ready-for-agent
Repos: alexandria-cloud
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/17-record-share-grant.md

Spec: Record store (hosted service) → ingest, named access, retention; Sending records → revoke.

- An ingest endpoint accepts signed batches, checks each statement against the share's scope and stores it. The filterable fields are stored as indexed columns beside the statement body from the start.
- Revoking deletes the records received under that share and keeps only their identifiers so they cannot be re-inserted.
- Records stay readable by name only while the share is active and the learner's enrolment with the organisation lasts. After either ends they are deleted or, where retention keeps aggregates, reduced to counts that obey the existing minimum cell size.
- Records follow the organisation's retention policy. Tenant isolation is enforced in the data layer.
- An administrator can see which learners have an active record share and when each last synced.

Done when: integration tests against Postgres cover scope enforcement at ingest, a batch with a bad signature refused, deletion on revoke with re-insertion blocked, the end-of-enrolment rule, tenant isolation and the share status listing.
