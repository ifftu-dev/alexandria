# Sending records directly to a store the learner names

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/14-learning-records.md

Spec: Sending records → common rules and Record destinations; user stories 28 to 35, 37 to 40.

- The shared sending mechanics: a scope naming courses and record kinds, completion-level by default; recipient identifiers applied at send, with the learner able to choose their real DID; a consent screen showing scope, identifier and what removal can and cannot undo; an outbox that batches, queues offline, retries with backoff and shows failures; every send written to the learner-readable access log.
- A record destination: the learner enters a store's address and the credentials issued to them. The app speaks xAPI to it directly and negotiates the version. A private or local address needs explicit confirmation.
- Credentials are kept with the protection the app gives other learner secrets.
- Removing a destination stops sends and issues voiding statements on a best-effort basis.
- Nothing here needs the hosted service.

Done when: host-seam tests against a small in-process xAPI stub show nothing is sent before approval, scope filtering by kind, per-recipient identifiers, offline queueing and retry, a visible failure, voiding on removal and the access log entries; interface tests cover the consent screen.

## Notes

2026-11-20: decisions taken after reading the code, before work starts.

- There is no access log on the device today; the only one is fetched from the hosted service. This ticket creates the local one: a per-profile table with one row per send (time, recipient, scope, record count, identifier used, outcome), shown beside the hosted entries in the existing access log screen. Reads by a recipient are known only to the hosted service and keep coming from there.
- Record destinations must work with the organisations feature switched off; do not route them through the code that returns nothing when that switch is off.
- The sending mechanics live outside the commands layer so ticket 17 can use them; commands stay thin.
