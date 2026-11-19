# Check the identifier scheme against real record stores

Triage: ready-for-agent
Repos: alexandria
Part of: docs/learning-standards-spec.md

Spec: Learning records → "Identifiers do not depend on a web domain" and "The scheme is checked before anything is built on it"; Further Notes → DID-based activity identifiers.

- Hand-build xAPI statements that carry the proposed identifiers: DID URL activity identifiers, an account-based actor whose home page is the fixed location-independent identifier, and per-recipient statement and registration identifiers.
- Send them to at least two record stores from different vendors, and check them against the official open-source conformance suite's statement rules. Open-source stores run locally count as vendors. Record what each accepts, rejects and how each displays the activities.
- Confirm the scheme, adjust it or replace it on that evidence. The preferred fallback is a documented mapping for tools that need web addresses; a domain-rooted scheme is the last resort and needs its own decision record.
- Also confirm which signature algorithms the standard's per-statement signature format allows, since the spec's choice of a detached signature rests on it.

Done when: the results and the confirmed scheme are appended here under Notes, with the exact statements used saved beside the research notes, so ticket 13 can build on them.
