# Forwarding shared records to an organisation's own store

Triage: ready-for-agent
Repos: alexandria-cloud
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/18-cloud-ingest.md

Spec: Record store (hosted service) → ingest and forwarding; user stories 53 and 54.

- An organisation can configure an external xAPI store. Ingested statements are forwarded to it through the existing egress controls, with retry.
- Forwarding failures are surfaced to administrators and retried; a brief outage of the external store loses nothing.
- On revoke, voiding statements are sent to the external store on a best-effort basis for records already forwarded.

Done when: integration tests against a stub external store cover forwarding, retry after failure, a surfaced failure, voiding on revoke and the egress controls refusing a disallowed address.
