# Device runs, external stores and the hosted conformance run

Triage: ready-for-human
Repos: alexandria, alexandria-cloud
Part of: docs/learning-standards-spec.md
Blocked by: docs/learning-standards/27-conformance-statement-and-docs.md

Spec: Testing Decisions → last paragraph.

Needs the owner:

- A run on a physical iOS and Android device per phase, covering a package with large video and focus mode, and whatever ticket 23 left for a phone.
- A package with media above the 32 MiB whole-file cap on Linux desktop. In CI, WebKitGTK opens such a file with no range, gets 416, and does not play it; a first chunk instead plays only that chunk. If the app's scheme behaves the same, large media needs a streamed response on Linux.
- The identifier compatibility check repeated with real records against external stores.
- The hosted xAPI conformance run that issues the certificate, against a deployed record store.

Record each result in the conformance statement.
