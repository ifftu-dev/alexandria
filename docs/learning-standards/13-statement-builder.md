# The statement builder

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/12-identifier-compatibility-check.md

Spec: Learning records (builder, verbs, identifiers); Sending records → recipient identifier; ADR 0004.

- A desktop-free module in the shared crate that builds and validates xAPI statements, written as xAPI 2.0 within the subset also valid in 1.0.3.
- Verbs from the published ADL and cmi5 vocabularies. SCORM run-time data maps to statements following the published SCORM profile for xAPI.
- Activity identifiers under the DID that created the course, as confirmed by ticket 12. They do not change with owner, staff or visibility.
- Recipient identifiers: actor, statement and registration identifiers derived per recipient from the learner's key, so no two recipients receive the same ones and none equals the learner's DID unless the learner chose it.
- Records have a kind: completion-level or interaction-level.

Done when: unit tests build each statement kind and compare against published example statements, invalid input is refused before storage, and two recipients are shown never to share an actor, statement or registration identifier.

## Notes

2026-11-20: built as the `xapi` module of the studio crate, with SCORM mapping in `xapi::scorm`. Tests: `crates/alexandria-studio/tests/xapi_statements.rs` and `tests/xapi_scorm.rs`, against the identifier check's canonical example statements, copied to `crates/alexandria-studio/fixtures/xapi-canonical/` (throwaway keys only).

- **Inputs are events, not statements.** A learning event (local id, timestamp, local registration, action, optional profile) is plain serialisable data. The same event and recipient always give the same statement, so the records module can store events and rebuild them for any recipient; resending gives the same identifiers.
- **One statement for both versions.** The output types can only express the subset valid in 1.0.3 and 2.0 (no context agents or groups, sub-statements, attachments or signatures; UTC timestamps; durations at hundredths of a second), and every one of the nine canonical statements is reproduced exactly for both versions. There is no version switch.
- **Recipients.** A recipient is an account plus a key for identifiers. The default actor is a per-recipient `did:key` from HKDF-SHA256 over the learner's key material; the learner may choose their own DID or approve a proposed account (checked against the 50-character limit). Statement and registration identifiers are UUIDv8 from HMAC-SHA256 under a per-recipient key, even when the learner chose their own DID. The recipient label is the caller's (tickets 16 and 17 fix it).
- **Validation.** Every built statement is checked before it is returned: identifiers, account, IRIs and the 2083-character cap, language tags, score ranges, the progress extension, duration form, interaction types and components, voiding rules. A statement read back from storage can be checked again the same way. Refusals carry stable codes.
- **Activity types.** Course: `course`. Element and package activity (SCO): `lesson`. Interaction: `cmi.interaction`, the type the xAPI interaction model recommends (the SCORM profile's table lists `interaction`).
- **Deviation from the canonical examples, where the SCORM profile wins:** interactions use the verb `responded` (the examples used `answered`), and the profile's category activity carries the type `http://adlnet.gov/expapi/activities/profile` (the example left it out). The tests apply exactly these two changes to the examples.
- **Deviations from the SCORM profile:** no session statements (`initialized`, `terminated`, `suspended`, `resumed`), since the spec records outcomes and the app records element launch itself; no attempt activity in the grouping, since the registration identifies the attempt and an attempt identifier would need its own per-recipient derivation; a SCORM 1.2 score with a stated range is scaled within it, otherwise divided by 100 as the profile says. One terminated session gives one result record (passed or failed, else completed, else scored) carrying score and time.
- **Untrusted package values.** A SCORM value that does not parse is left out rather than refusing the record: an out-of-range score, a malformed time, an unknown interaction type, an interaction with no response.
- **One case beyond ticket 12's scheme:** an interaction under a package activity that already sits at the length cap, whose digest form would still be too long, hangs its digest off the activity's digest form.
- **Not in this ticket:** the export signature (ticket 15) and cmi5 statements (ticket 25). The verb list already holds the cmi5 verbs.

Proposed doc edits, for the owner: the glossary terms "learning record", "recipient identifier" and "record kind" could point at the module; the account home page constant could be named in the architecture document's identifier section when ticket 27 writes the conformance statement.
