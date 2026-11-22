# Sending records directly to a store the learner names

Triage: done
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

2026-11-20: done. Decisions and deviations.

- Layout: `src-tauri/src/record_sending/` (recipients, scope, outbox, access log, sending, record destinations); the xAPI client (address rules, version negotiation, posting) is `alexandria_studio::xapi::lrs`. Commands in `commands/record_destinations.rs` only fetch the learner's key and hand over. Database work goes through a small `RecordDb` trait so the background sender, the commands and the tests run the same code without holding the database across a request.
- Migration 47 adds four tables: `record_recipients` (any approved recipient: kind `destination` or `share`, label, identifier choice, account name shown at consent, scope), `record_destinations`, `record_destination_secrets` and `record_access_log`. The schema doc is regenerated.
- Recipient label for a destination: `destination|<normalised xAPI base address>|<store-issued key>`. The same store and key cannot be added twice.
- Credentials: the secret is a row in the profile's encrypted database, apart from the destination, like model API keys; nothing returned to the interface carries it. Only HTTP Basic credentials are supported, which is what stores issue.
- Addresses use the model connections' rules: a public store must use https; an address on this device or a private network needs an explicit confirmation on the consent screen; names are resolved and pinned, redirects not followed. The address is resolved again before each pass, and sending stops with a visible failure if it now leads to a different kind of network.
- Version: negotiated from the store's `about` resource at approval (2.0.0 preferred, else 1.0.3), stored, and sent as the version header.
- Nothing is sent before approval: the preview only resolves the address and derives identifiers. Approval queues every record already in scope, so past records are sent too.
- Outbox: each pass queues what is newly in scope (the record writers are untouched), then sends due records oldest first in batches of 50. A store that cannot be reached keeps records queued; any other error marks them failed. Both retry after 30 s, doubling to a 6 h cap. A batch refused with 400 is retried one statement at a time. A background profile job runs a pass at least every minute, when a retry falls due, and when a destination is added or the learner asks.
- Access log: one row per delivery that reached the recipient (accepted or refused) and per voiding batch, with time, recipient, scope, record count, record ids, identifier and outcome. A try that reached nobody is not logged; it shows as waiting. The log is listed beside the hosted views in the organisations access panel, and by the destinations panel when that panel is not on screen (organisations switched off or no Cloud).
- Removal deletes the destination, its secret and its outbox rows in one transaction before anything else, then asks the store once to void every record it accepted. The ids come from the access log, so a record the learner later deleted locally is still voided. The outcome is shown and logged.
- Records from private courses: no course is private yet, so names are always disclosed. `send.rs` has the single place to choose `Names::Omit` once the course access spec adds the switch; the consent screen has no private-course notice yet.
- Story 42 (supervised profiles) is not in this ticket's list; no guardian rule was added.
- Send failures in the activity log: ticket 15 owns that screen. The hook is `record_sending::outbox::failures(conn)` (record id, recipient, attempts, last error, next try); no command was added for it. Until it is wired, failures show on each destination in the destinations panel.
- Placement: the destinations panel lives in Settings → Organisations, outside the switch, as the only existing screen with an access log. Moving it beside the activity log once ticket 15 lands may read better.
- Strings are English-only under `organisations.records.*`, listed as pending translation; the consent copy should be prioritised.
- Not run: the desktop interface harness. No harness screen mounts the new panels; the notification-links screen answers unknown commands with null, which the panels tolerate.

Proposed doc edits (not made):

- `GLOSSARY.md`: "record destination", "recipient label" and "access log (local)".
- `src-tauri/src/AGENTS.md`: sending code lives in `record_sending/`; reuse `send_due` with a `Transport` rather than writing a second outbox driver.
- The spec's Enterprise boundary rows are unchanged by this ticket.

### Review (2026-11-21)

- The send log is shared state (`useRecordSendLog`), so the destinations panel and an organisation's access page show and refresh the same rows.
- Supervised profiles: the panel maps a host refusal starting with `guardian_approval_required` to `organisations.records.guardianRequired` (a guardian decides; nothing was added). Gap: no in-app request-approval flow on the frontend; it follows whatever the host side adds.
- Share consent copy: `RecordConsent` takes `share`; for a share the pairwise note no longer promises recipients cannot link the identifier (shares carry the learner's DID and are signed with their key). The destination promise is unchanged. The owner decision on shares is recorded with ticket 17.
- Host side, supervised profiles: `destinations::add` refuses with `guardian_approval_required:` when the holder is a minor or a guardian link to the profile is active. Gap: there is no guardian approval for destinations yet (the AI chat approval is the prior art); until there is, they are refused rather than left as a side door (story 42).
- A destination's statement and registration identifiers are keyed by its label and the approval's id (`recipients::instance_label`), so a store removed and added again does not see the voided ids repeat. The actor still comes from the label, as the preview shows it.
- A removal whose voiding stops partway reports the count voided before it stopped (`send::Voided`).
- `preview`, `add`, `send_pending`, `remove`, `send_due`, `void_sent` and the share sender return `SendError`; commands convert to strings. `RecordDb` is now the packages' `PackageDb` trait (one database handle for both); `LeasedDb` is gone. The stored location is read only in its own vocabulary.
- Not done (judgement): `RecipientKind::as_str/parse` stay hand-written; they also name the SQL column values.
- Proposed doc edit, not made: the `record_recipients.label` comment in `record_sending.sql` (and so the schema document) says every identifier is derived from the label; for a destination the statement and registration identifiers also take the approval's id.
