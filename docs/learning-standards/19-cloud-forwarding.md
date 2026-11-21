# Forwarding shared records to an organisation's own store

Triage: done
Repos: alexandria-cloud
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/18-cloud-ingest.md

Spec: Record store (hosted service) → ingest and forwarding; user stories 53 and 54.

- An organisation can configure an external xAPI store. Ingested statements are forwarded to it through the existing egress controls, with retry.
- Forwarding failures are surfaced to administrators and retried; a brief outage of the external store loses nothing.
- On revoke, voiding statements are sent to the external store on a best-effort basis for records already forwarded.

Done when: integration tests against a stub external store cover forwarding, retry after failure, a surfaced failure, voiding on revoke and the egress controls refusing a disallowed address.

## Notes

2026-11-20: done. Decisions, deviations and open questions for the owner.

- Configuration. An organisation has at most one external store: its xAPI base address, the username and password the store issued, and the xAPI version to speak (1.0.3 by default, or 2.0.0). Administrators manage it from a new card on the Integrations page, backed by `GET /api/record-forwarding`, `PUT` and `DELETE /api/record-forwarding/store`, and `POST /api/record-forwarding/retry`, all administrators only because the store receives named records. The password is sealed under the deployment's key the way webhook and email credentials are, bound to the organisation and the store, and its column is unreadable by the request role. The version is chosen, not negotiated: saving never contacts the store (the same save-time rule as webhooks), and negotiating on every send would double the requests. Owner may prefer negotiation on first send.
- Forwarding. A record is queued in the ingest transaction, only when it is newly stored, so a retried batch never forwards twice. A worker in the service sends what is due every two seconds, up to 50 statements a request, as the learner's app sent them (the store adds `stored` and `authority`). Each organisation is independent. Only records that arrive after the store is configured are forwarded; there is no backfill. A record deleted here before its turn (revoke, end of enrolment, retention) is never sent. Changing the store's address cancels what was queued for the old one rather than sending it to the new one (the webhook precedent); changing only the credentials keeps the queue.
- Egress. Sending goes through the existing pinned, https-only, no-redirect client. The test seam is a value the worker is handed (`egress::Egress`): production builds only ever construct the public policy, and the only other policy admits one named plain-http loopback address. Its constructor is compiled only in debug builds and is never read from configuration, so production egress is unchanged. Integration tests drive the worker directly with that policy against a stub store on loopback; the background worker in the service always runs under the public policy.
- Retry and failures. A failed request is retried on the webhook backoff (a minute, ten minutes, an hour, ten hours; five attempts). A 4xx other than 408, 425 or 429 (wrong credentials, a conflicting statement) stops at once. Either way the row stays visible to administrators with the store's status and a reason, and an administrator can send every failing row again once the store is fixed. A brief outage loses nothing; an outage longer than about eleven hours leaves the rows stopped and visible until an administrator sends them again.
- Voiding on revoke. In the revoke transaction, before the records are deleted, forwards not yet tried are cancelled and a voiding statement is queued for every record that may have reached the store: delivered, or tried at least once whatever the answer (a timeout can still have landed; the xAPI specification asks stores not to refuse a void of a statement they do not hold). The voiding statement uses the original statement's actor (the same pairwise account the store already holds, matching the app's own voiding) and an id derived from the voided statement's id, so a retried void is the same statement to the store. Voids are retried and surfaced like forwards. Until a void is sent, its row holds that actor; spent rows are deleted 90 days after the record is gone.
- Not voided (open question). Records deleted because the learner's enrolment ended or retention expired are not voided at the external store. The spec names only revoke, and the external store is the organisation's own system under its own retention. Owner to confirm.
- Narrowing a share does not void anything at the store, consistent with ticket 18 keeping already-received records after a narrowing (still an open owner question there).
- Audit. Saving, removing and resending reuse the existing integration audit verbs (configured, disconnected, replayed) rather than adding new ones, which would need a migration and a change to the shared audit enum.
- Migration 0063 (`record_forwarding_stores`, `record_forwards`), both tenant-isolated in the data layer. The worker reads through the owner connection, as the webhook worker does, because the credential is not selectable by the request role.
- Proposed doc edits, not made: the hosted service's `docs/organisations.md` access table should add the four `/api/record-forwarding` routes (administrator), and a short "Forwarding" paragraph under the record shares section proposed by ticket 18 (what is forwarded, retry and failures, voiding on revoke, no backfill). `docs/ats-integrations.md` could mention that the Integrations page also carries the learning record store card. The spec's Record store section could say that the xAPI version is chosen by the administrator and that voiding covers records that may have reached the store, not only confirmed ones.

### Review (2026-11-21)

- Narrowing a share now voids, at the store, what was forwarded from the courses or kinds the learner withdrew, and cancels what was not yet forwarded from them (ticket 18 reversed its narrowing reading). The bullet above saying narrowing voids nothing no longer holds.
- A revoke after an administrator's resend still voids: the void query asks whether a forward was ever attempted (`last_attempt_at`) rather than counting attempts, which a resend resets.
- One bad statement no longer holds back its batch: when the store refuses a batch's statements (a 4xx other than 401, 403, 408, 425 or 429), each is sent again on its own, so only the one the store will not take fails, visibly. The batch's lease is extended for the single sends.
- Audit verbs of its own (migration 0067): `record_forwarding.configured`, `.removed`, `.retried`, replacing the reused integration verbs.
- The action column is `record_forwarding::ForwardAction` in code; the version list is derived from `XapiVersion` (ticket 20); the admin gate is the console's single `api::require_admin`.
- Owner question (story 38): a forwarded copy reaches the organisation's own store, but the learner's access log records named reads through this service only, not the forward. Should forwarding (or the store's later reads, which this service cannot see) appear in the learner's access log? Not implemented.
