# Check the identifier scheme against real record stores

Triage: done
Repos: alexandria
Part of: docs/learning-standards-spec.md

Spec: Learning records → "Identifiers do not depend on a web domain" and "The scheme is checked before anything is built on it"; Further Notes → DID-based activity identifiers.

- Hand-build xAPI statements that carry the proposed identifiers: DID URL activity identifiers, an account-based actor whose home page is the fixed location-independent identifier, and per-recipient statement and registration identifiers.
- Send them to at least two record stores from different vendors, and check them against the official open-source conformance suite's statement rules. Open-source stores run locally count as vendors. Record what each accepts, rejects and how each displays the activities.
- Confirm the scheme, adjust it or replace it on that evidence. The preferred fallback is a documented mapping for tools that need web addresses; a domain-rooted scheme is the last resort and needs its own decision record.
- Also confirm which signature algorithms the standard's per-statement signature format allows, since the spec's choice of a detached signature rests on it.

Done when: the results and the confirmed scheme are appended here under Notes, with the exact statements used saved beside the research notes, so ticket 13 can build on them.

## Notes

2026-11-20: checked. The scheme holds with one change: the account home page is a fixed `urn:uuid`, not a DID.

**What was tested.** 60 hand-built statements per xAPI version (1.0.3 and 2.0.0), plus four signed statements. They cover the activity identifier in path, query, fragment and bare-DID forms; package activities with reserved, non-ASCII and IRI-shaped identifiers; lengths from 1024 to 4096 characters; case twins; dot segments; activities with no definition; seven account home page candidates; account names of 48, 50, 51 and 56 characters; four ways to derive a UUID; DID-based extension keys, activity types and verbs; and one realistic statement for each record kind (registered, launched, progressed, completed, passed, failed, answered, voiding, a batch of three). All keys are throwaway test keys. The statements, the generator, the runner, the raw results and the store screenshots are saved beside the research notes.

**Stores.** Four open-source stores from four vendors ran locally: SQL LRS 0.9.14 (Yet Analytics), the ADL reference LRS, the Learning Locker xAPI service and Ralph 5.1.0. Learning Locker and Ralph took 1.0.3 only. Ralph is a statements-only store; it has no activities, agents or document resources.

**What every store accepted.** Every DID URL activity identifier form, in course, element, package activity and interaction positions, in the statement object and in context activities. Every account home page candidate: a bare `did:key`, a DID URL with a path, `urn:uuid`, an unregistered `urn:` namespace, a `tag:` URI and even the bare prefix `did:key:`. Statement and registration identifiers shaped as UUID versions 4, 5, 7 and 8. DID and URN extension keys, a DID activity type and a DID verb. Voiding and batches. Each identifier came back byte for byte: no store normalised case, percent-encoding or dot segments. The two case twins (DIDs that differ in one letter's case) stayed two separate activities. Filtering statements by activity, by agent and by registration found the right records, related-activity queries found completions under the course, and the document resources worked with DID activity identifiers and the account agent.

**What stores rejected.** Every store refused an identifier with no scheme and an invalid UUID, except that Ralph does not validate identifiers at all. The limits that matter:

- ADL refuses an activity identifier over 2083 characters and an account name over 50 characters. Both fail as server errors (500) from database column sizes, not as validation errors. A full `did:key` is 56 characters, so it cannot be an account name.
- SQL LRS fails on an activity identifier over about 2800 characters with a stack overflow (2803 for the DID form, 2807 for a web address). Learning Locker and Ralph accepted 65,536.
- SQL LRS and Learning Locker refuse a UUID whose version and variant bits are not set (a raw truncated hash). ADL and Ralph accept it. So derived identifiers must carry valid UUID bits.
- SQL LRS refuses `openid` set to a DID. The scheme does not use `openid`.
- The web-address controls hit exactly the same length limits. Nothing failed because an identifier was a DID rather than a web address.

**How stores displayed the activities.** The SQL LRS statement browser shows the actor as the account name, the verb's display text, and the activity's name when the statement defines one, or else the full DID URL wrapped in the column. The DID URL works as a clickable filter there. The ADL "My Statements" page shows each record as account name, verb and full activity identifier, then the raw statement. Learning Locker's xAPI service and Ralph have no interface of their own; their query results matched the others. Activities with no name (private courses) show as their identifier everywhere, which is the intended effect.

**Conformance suite.** The official suite (adlnet `lrs-conformance-test-suite`, September 2025) tests stores, not statements. So it ran twice against each store: once unchanged, and once with its statement templates rewritten so that every activity identifier, account home page and extension key the suite sends uses the scheme under test. That second run covers 1365 tests for 1.0.3 and 1435 for 2.0.0.

- On a fresh SQL LRS, the rewritten suite passed every test: 1365 of 1365 for 1.0.3 and 1435 of 1435 for 2.0.0.
- On the long-running stores, the rewritten suite failed exactly the tests the unchanged suite failed, plus at most one: the test that fetches the suite's fixed activity `http://www.example.com/verify/complete/34534` and expects its definition to match exactly. That activity's stored definition already held extension keys from the earlier run, so it is cross-run state, not a rejection. SQL LRS and Learning Locker failed only that test in both runs. ADL failed 83 tests on 1.0.3 and 78 on 2.0.0 unchanged, which is its own non-conformance, plus that one in the rewritten run.

**Confirmed scheme** (exact strings and derivations are in the note handed to ticket 13):

- Activity identifiers: `<creator DID>/courses/<course id>`, then `/elements/<element id>`, then `/packages/<package hash>/activities/<segment>`, then `/interactions/<segment>`. A package's own activity identifier becomes one path segment: every UTF-8 byte outside the RFC 3986 unreserved set is percent-encoded with upper-case hex, and a segment of exactly `.` or `..` is encoded as `%2E` or `%2E%2E`. Stores compare byte for byte, so this canonical form is fixed. The path form is kept over the query and fragment forms because it nests, and related-activity queries rely on that.
- Length: a complete identifier stays within 2083 characters, the lowest limit seen. A package or interaction identifier that would exceed it is replaced by its SHA-256 digest under a separate path word, so the result cannot collide with an encoded identifier.
- Actor: an account with home page `urn:uuid:044eac38-2871-47c7-a8d8-5c25a8a9479c`, defined once in the shared crate, and a name that is the method-specific part of the per-recipient `did:key` (48 characters for Ed25519, within the 50-character limit). The real-DID choice uses the same form with the learner's own key.
- Statement and registration identifiers: UUID version 8, made from the first 16 bytes of a keyed hash over the local identifier. The key is a per-recipient key derived from the learner's key, so identifiers are stable across retries (a resend is idempotent) and no two recipients receive the same ones.

The realistic statements, rewritten to this exact scheme, were sent again to all four stores in both versions, and all were accepted. One quirk: Ralph answers a single-statement GET with a statements list.

**Why the home page changed.** Every candidate was accepted, so the choice rests on meaning. A `did:key` home page made from a published label is a key whose private half anyone can compute; it suggests a controller that does not exist. A `did:web` or `tag:` home page brings a domain back in. An unregistered `urn:` namespace breaks the URN registration rules. `urn:uuid` is a registered, permanent, location-independent name that claims nothing more. The standard calls the home page an IRL, but neither the stores nor the suite check that it can be dereferenced. Not being a web address is what domain independence costs here.

**Fallback.** No store needed it, so no mapping to web addresses is built. The mapping stays the documented fallback for a reporting tool that turns out to require web addresses, and no domain-rooted scheme or decision record is needed.

**Signatures.** Both 1.0.3 and 2.0 allow only RS256, RS384 and RS512 for the per-statement signature (Data 2.6, XAPI-00117; the suite tests it for both versions). An Ed25519 (EdDSA) signed statement was refused with 400 by SQL LRS ("JWS Algorithm MUST be RS256, RS384, or RS512"), ADL and Learning Locker. An RS256 control was accepted. Ralph does not accept signed statements at all. The learner's Ed25519 key cannot produce an allowed signature, so the spec's detached signature over the export file holds.

**Proposed doc edits** (not made here): in the spec's Learning records section, say that the fixed account home page is a `urn:uuid`, and state the 2083-character limit on activity identifiers and the 50-character limit on account names. In Further Notes → DID-based activity identifiers, record that four stores and the conformance suite accepted the scheme and that the mapping is not built.
