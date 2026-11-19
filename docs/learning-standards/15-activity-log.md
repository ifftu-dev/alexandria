# The activity log

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/14-learning-records.md

Spec: Learning records → activity log view and export; user stories 24 to 27.

- A view listing every learning record the app has written about the learner, with the log's size limit shown.
- Export as a file of standard xAPI statements with a detached signature by the learner's key over the whole file. The standard's per-statement signature format is not used.
- Delete, for one record, a selection or everything.

Done when: host-seam tests export a log and verify the signature over the exact file, a tampered file fails, and deletion removes records; interface tests cover the list, the limit and the export and delete controls.
