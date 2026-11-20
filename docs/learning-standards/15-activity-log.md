# The activity log

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/14-learning-records.md

Spec: Learning records → activity log view and export; user stories 24 to 27.

- A view listing every learning record the app has written about the learner, with the log's size limit shown.
- Export as a file of standard xAPI statements with a detached signature by the learner's key over the whole file. The standard's per-statement signature format is not used.
- Delete, for one record, a selection or everything.

Done when: host-seam tests export a log and verify the signature over the exact file, a tampered file fails, and deletion removes records; interface tests cover the list, the limit and the export and delete controls.

## Notes

2026-11-20: built.

- **Export format** (`alexandria_verify::record_export`, format tag `alexandria-record-export/1`). Two files: the statement file, a pretty-printed JSON array of xAPI statements with a trailing newline, and a signature file (the same name plus `.sig`), a JSON object `{format, signer, sha256, signature}`. `signature` is base64url (no padding) Ed25519 by the learner's `did:key` over `format || 0x00 || statement file bytes`; `sha256` is the file's lower-case hex digest, checked first so a changed file reads as "not the file that was signed". `verify` returns the signer; the reader decides whether it is the learner they expect. Byte vectors with a manifest are in the verify crate's `tests/vectors/record-export/`.
- **What is exported.** Every record, oldest first, built for the learner's own log: the learner's DID as the actor, identifiers keyed by the `activity-log` label and the learner's key seed, names disclosed. No per-statement signature, no attachments. Always the whole log; there is no selective export.
- **Saving.** The command returns both files as strings; the view saves them through two webview downloads, unchanged. Export needs the vault unlocked.
- **Deleting is local.** A deleted record leaves this device's log. Its outbox rows go with it, so a deleted record that was waiting to be sent (queued or failed) is never sent. A record a recipient already received stays with that recipient; deleting does not void it. The delete confirmation says both. Voiding stays with removing a destination or revoking a share.
- A writer that is idempotent by local id (package session results, folded again on terminate) can write a deleted record again if the same event is reported again. Not handled; deletion is not a tombstone.
- The view is a page of its own (`/activity-log`, user menu), read a hundred rows at a time, so it does not touch the settings page that record destinations will use.
- Fixed a clippy `type_complexity` error in the size-limit code that failed `-D warnings` on the integration branch.

Proposed doc edits (not made):

- `crates/alexandria-verify/CHANGELOG.md`: an additive entry for the `record_export` module, and a version decision if 0.7.0 is already published.
- `crates/alexandria-verify/README.md` and `tests/vectors/README.md`: list the record export format and its vectors.
- `docs/desktop-ui-validation.md`: the `activity-log` harness screen.
- `GLOSSARY.md`: "signed export" as the activity log's export with a detached learner signature.
