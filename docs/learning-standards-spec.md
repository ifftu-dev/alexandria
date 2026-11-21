# Learning standards: SCORM, xAPI and cmi5

Status: draft
Updated: 2026-11-20
Triage: ready-for-agent
Repos: alexandria, alexandria-cloud

Alexandria implements none of the e-learning interoperability standards today. This spec adds them: playing SCORM 1.2 and SCORM 2004 packages, producing xAPI learning records, sending those records to a store the learner names or sharing them with an organisation under a learner-signed grant, a conformant xAPI record store in the hosted service, and cmi5 launch. Who can see a course and how its files travel between devices are decided in [course-access-spec.md](course-access-spec.md), not here. Background: [plugins.md](plugins.md), [enterprise-boundary.md](enterprise-boundary.md), [ADR 0003](adr/0003-credential-authority.md), [ADR 0004](adr/0004-tauri-free-runtime-crate-boundary.md). Vocabulary: [GLOSSARY.md](../GLOSSARY.md).

## Problem Statement

An organisation evaluating Alexandria asks two questions on every procurement form: does it play our existing SCORM courses, and does it report to our learning record store. The answer to both is no.

- **Existing course libraries cannot be used.** Most corporate and institutional training content is packaged as SCORM 1.2 or SCORM 2004. Alexandria has its own signed course document and its own element types, so a learner or instructor holding a SCORM package has no way to open it.
- **Learning activity is invisible outside the app.** Progress lives in the learner's own database. An organisation that tracks training in a learning record store sees nothing of what happens in Alexandria, and a learner cannot take a standard record of their own activity anywhere else.
- **The hosted service cannot be the system of record.** Organisations that want Alexandria Cloud to hold learning records need it to speak the xAPI interface their reporting tools already use.
- **Newer packaged content cannot be launched.** cmi5 is the packaging and launch profile that replaces SCORM for xAPI content, and Alexandria cannot launch it.
- **A conformance claim needs evidence.** Saying "SCORM compliant" without a recorded test run is the kind of claim Alexandria refuses to make about credentials, and it should not make it about standards either.
- **The standards assume a central system that knows everyone.** They were written for a server that holds every learner's identity and every record. Adopted as written, they would give each organisation, and every store an organisation forwards to, one identifier that follows a learner everywhere, and would make a hosted service the only road out of the app.

## Solution

Alexandria plays standards packages on the device, keeps a standard record of learning activity that belongs to the learner, and lets the learner send it where they choose.

- **A standards package is an element.** An instructor adds a SCORM 1.2, SCORM 2004 or cmi5 package to a course as one element, or a learner opens a package file directly. The package runs offline in an isolated frame with its own table of contents and navigation, on desktop and mobile, and resumes where the learner left off.
- **Package content is untrusted and treated that way.** It cannot reach the network, the app, another package's data or the learner's real identifier, and it always runs under a label the app draws.
- **Package results are progress, never credentials.** A package reports its own completion and score from script running on the learner's device. That marks the element complete and nothing more.
- **Every learner has an activity log.** The app writes xAPI learning records for course and package activity into the learner's own database. The learner can read every record, export them as standard xAPI statements with their own signature over the export, and delete them.
- **The learner decides where records go, and no hosted service is required.** A learner can send records straight to any xAPI store they name. An organisation that uses the hosted service can instead ask for records through the existing grant mechanism. Either way the learner approves a scope, can narrow or revoke it, and the default scope is completion and score, not every answer.
- **Each recipient sees a different identifier.** Records sent to one organisation cannot be matched to records sent to another, or to the learner's credentials, unless the learner chooses to use their real identifier.
- **The hosted service is a conformant record store.** Alexandria Cloud accepts shared records, forwards them to an organisation's existing store if it has one, and exposes a standard xAPI interface that passes the official conformance suite.
- **Conformance is stated with its evidence.** Each claim names the standard, the edition, the test evidence and the known gaps.

## User Stories

### Learner

1. As a learner, I want to open a SCORM package file I was given, so that I can take a course that only exists in that format.
2. As a learner, I want a package to run with no network connection, so that I can learn offline like everything else in Alexandria.
3. As a learner, I want a package to resume where I stopped, including its own bookmark and saved state, so that I do not restart a long module.
4. As a learner, I want a table of contents for a package with several lessons, so that I can see where I am and what remains.
5. As a learner, I want previous, continue, suspend and exit controls that follow the package's own sequencing rules, so that I cannot reach a lesson the author meant to be locked and am not blocked from one that is open.
6. As a learner, I want the package to hide the app's navigation controls when its author asked for that, so that I am not shown two competing sets of buttons.
7. As a learner, I want my progress on a package element to count towards the course like any other element, so that the course shows as complete when I finish it.
8. As a learner, I want to see the score and status a package reported, so that I know how I did.
9. As a learner, I want it stated plainly that a package result is self-reported and earns no credential, so that I am not misled about what finishing it proves.
10. As a learner, I want to start a fresh attempt at a package, so that I can retake it.
11. As a learner on a phone, I want packages to run in the same player as on desktop, so that I am not told to switch device.
12. As a learner on a phone, I want video inside a package to start quickly and seek without the app running out of memory, so that large packages are usable on a modest device.
13. As a learner, I want a package I am not currently taking to cost roughly its own size in storage, not twice that, so that a few courses do not fill my phone.
14. As a learner, I want a clear message when a package needs something the device cannot provide, such as a pop-up window, full screen or a remote resource, so that I know why it looks broken.
15. As a learner, I want to enlarge a package to fill the window while the app's label stays visible, so that video and simulations are comfortable to use without losing the warning.
16. As a learner, I want a package refused at import with a stated reason when it is malformed or unsafe, so that I am not left with a blank screen.
17. As a learner, I want to launch cmi5 content the same way as SCORM content, so that I do not need to know which standard a package uses.
18. As a learner, I want a package to be unable to learn my real identifier, so that content I did not write cannot recognise me across courses.
19. As a learner, I want a package to be unable to send anything off my device, so that opening someone else's content does not leak what I do in it.
20. As a learner, I want a label drawn by the app around every package, saying it is third-party content and that Alexandria never asks for my recovery phrase there, so that a package cannot pose as the app.
21. As a learner, I want one package unable to read what another package stored, so that content from one publisher cannot inspect my work in another's.
22. As a learner, I want a package file I opened straight from disk never to leave my device, so that something handed to me privately is not redistributed.
23. As a learner, I want a package in a course to be shared between devices by the same rules as the rest of that course, so that I have one set of sharing limits to understand.
24. As a learner, I want an activity log listing every learning record the app has written about me, so that I can see exactly what exists.
25. As a learner, I want to export my learning records as standard xAPI statements, so that I can take them to any tool that reads them.
26. As a learner, I want my export signed with my own key, so that someone I hand it to can check it came from me and was not altered.
27. As a learner, I want to delete my local learning records, and to have old ones summarised or removed automatically past a limit I can see, so that the log is mine and does not grow without bound.
28. As a learner, I want to send my records directly to a store I name, with credentials I enter, so that I need no organisation and no hosted service to use the standard.
29. As a learner, I want an organisation's request for my learning records to say which courses and which kinds of record it covers, so that I know what I am agreeing to.
30. As a learner, I want sharing to cover completion and score by default, with per-answer detail a separate choice I must make, so that I do not hand over every response by accident.
31. As a learner, I want to approve a narrower scope than was asked for, so that I share only what I choose.
32. As a learner, I want each recipient to see an identifier made only for them, so that two organisations cannot compare notes about me.
33. As a learner, I want to choose to use my real identifier with a recipient, so that I can link records to my credentials when that helps me.
34. As a learner, I want to see which identifier my records will carry at a recipient before I approve, so that I know how they will recognise me.
35. As a learner, I want nothing sent before I approve, so that a request alone reveals nothing.
36. As a learner, I want to revoke a record share and have the records received under it removed from the hosted service, so that revoking means what it means for every other grant.
37. As a learner, I want to be told before I approve that records sent to a store outside the hosted service cannot be recalled, so that I decide knowing that.
38. As a learner, I want every send and every read of my records to appear in my access log, so that I can see who looked and when.
39. As a learner, I want records queued while offline to send when I am back online, so that sharing does not depend on being connected at the moment I learn.
40. As a learner, I want a failed send to be visible and retried, so that I am not silently out of sync.
41. As a learner with several profiles on one device, I want each profile's packages, attempts and records kept apart, so that another person's activity never appears as mine.
42. As a guardian, I want any sending of a supervised profile's records to go through the same consent rules as any other grant for that profile, so that a minor's activity is not shared by a side door.

### Instructor

43. As an instructor, I want to add a standards package to a course as an element, so that I can reuse content I already have.
44. As an instructor, I want to see the package's title, standard, edition and lesson structure before I add it, so that I know what I am publishing.
45. As an instructor, I want import to tell me exactly what is wrong with a rejected package, so that I can fix or replace it.
46. As an instructor, I want a published course to carry the package by content hash, so that every learner runs the bytes I published.
47. As an instructor, I want to be stopped from making a package element the assessed basis of a credential, so that I do not publish a course whose credential rests on a self-reported score.
48. As an instructor, I want to preview a package as a learner would see it, so that I can check it works before publishing.
49. As an instructor, I want learners on an older app version to see an unsupported-element notice instead of a broken course, so that publishing a package element does not break them.
50. As an instructor, I want to be warned, when I add a package to a public course, that every device taking the course will redistribute it, so that I do not publish content my licence forbids me to pass on.

### Organisation

51. As an organisation administrator, I want to receive a learner's records in our own store without running or buying the hosted service, so that adopting Alexandria does not add a vendor.
52. As an organisation administrator, I want to request learning records from a learner for named courses through the hosted service, so that training completed in Alexandria appears in our reporting.
53. As an organisation administrator, I want shared records forwarded to our existing learning record store, so that we do not run a second system.
54. As an organisation administrator, I want forwarding failures surfaced and retried, so that a brief outage of our store does not lose records.
55. As an organisation administrator, I want the hosted service to be our learning record store if we have none, so that we can start without buying one.
56. As an organisation administrator, I want to create and revoke interface credentials with read or write scope, so that each reporting tool gets only the access it needs.
57. As an organisation administrator, I want our records isolated from every other organisation's, so that a credential of ours can never read theirs.
58. As an organisation administrator, I want record retention to follow our existing retention policy, so that learning records are not an exception to it.
59. As an organisation administrator, I want to see which learners have an active record share and when each last synced, so that I know whose data is current.
60. As an organisation administrator, I want to propose the account identifier our systems use for a learner, so that their records match our HR data once they approve it.
61. As an organisation administrator, I want to hand a procurement reviewer a conformance statement with its evidence, so that the claim can be checked.

### Integrator

62. As an integrator, I want the record store to implement the standard xAPI resources, so that my existing reporting tool connects without custom code.
63. As an integrator, I want the record store to accept both xAPI 1.0.3 and 2.0 clients, so that older tools keep working.
64. As an integrator, I want to write statements from my own tools into the organisation's record store, so that it holds more than Alexandria's activity.
65. As an integrator, I want statement queries, paging, voiding, attachments and the document resources to behave as the specification says, so that I can rely on documented behaviour.
66. As an integrator, I want statement queries to stay fast as the store grows, so that a report over a year of records does not time out.
67. As an integrator, I want stable error responses for invalid statements, so that I can correct them.
68. As an integrator, I want Alexandria's statements to use published verbs and a documented activity identifier scheme, so that I can write reports against them.
69. As an integrator, I want SCORM activity mapped to xAPI the way the published SCORM profile describes, so that records from SCORM content look like records from any other conformant system.

### Maintainer

70. As a maintainer, I want package content unable to reach the app's command bridge, the network or another package's data, so that untrusted legacy script cannot harm a learner.
71. As a maintainer, I want package content served under its own scheme and policy, so that loosening rules for legacy content does not loosen them for plugins.
72. As a maintainer, I want isolation to hold on platforms where every package shares one web origin, so that the guarantee does not depend on which device a learner has.
73. As a maintainer, I want archive extraction bounded and path-safe, so that a hostile archive cannot fill a disk or write outside its directory.
74. As a maintainer, I want package files served by byte range without loading a whole file into memory, so that large media does not crash a phone.
75. As a maintainer, I want frequent commits from content coalesced before they reach the database, so that a chatty package does not stall the rest of the app.
76. As a maintainer, I want the runtime to be a pinned, vendored open-source library, so that the sequencing engine is not rewritten and its version is reviewable.
77. As a maintainer, I want package parsing and record building free of desktop dependencies, so that a headless node and the CLI can use them.
78. As a maintainer, I want activity and actor identifiers that do not depend on any web domain staying registered, so that records outlive whoever hosts a site.
79. As a maintainer, I want those identifiers tried against real external stores before the record work is built on them, so that an incompatibility is found while it is cheap to change.
80. As a maintainer, I want a conformance matrix mapping each mandatory requirement to an automated test, so that a regression is caught and a claim can be traced.
81. As a maintainer, I want the official xAPI conformance suite run in continuous integration against the record store, so that conformance cannot quietly lapse.
82. As a maintainer, I want package results kept out of grading, credential issuance and reputation by construction, so that the credential authority rule holds without relying on review.
83. As a maintainer, I want the phases shippable one at a time, with everything a learner needs working before any hosted-service work, so that the app never depends on the service for a standard.

## Implementation Decisions

### Vocabulary

New terms, to be added to the glossary when the work starts.

- **Standards package**: a SCORM 1.2, SCORM 2004 or cmi5 content package, stored as one content-addressed archive.
- **Package element**: a course element whose content is a standards package. One element is one whole package, whatever its internal structure.
- **Package attempt**: one learner's run through a package under an enrolment, with its own run-time state.
- **Learning record**: an xAPI statement written on the learner's device about the learner's own activity.
- **Activity log**: the learner's local, readable collection of learning records.
- **Record destination**: an xAPI store the learner has named and authorised to receive records directly from the device.
- **Record share**: a learner-signed grant that lets an organisation receive learning records through the hosted service within a stated scope.
- **Recipient identifier**: the identifier a learner's records carry at one record destination or record share, derived for that recipient alone.
- **Record store**: the hosted service's xAPI learning record store, one per organisation.

### What conformance means here

| Standard | Target | Evidence |
|---|---|---|
| SCORM 1.2 | Run-time environment with every mandatory and optional data model element, content packaging import | Conformance matrix with automated tests; self-tested |
| SCORM 2004 4th Edition | Content aggregation, run-time environment, sequencing and navigation. Packages built for the 2nd and 3rd Editions run on the same runtime | Conformance matrix with automated tests; self-tested |
| xAPI 1.0.3 and 2.0 (IEEE 9274.1.1-2023) | Record store conformance in the hosted service; well-formed statements from the app | Official conformance suite in continuous integration; hosted run for the certificate |
| cmi5 | Launch, session and completion rules for packaged assignable units | Conformance matrix using the published cmi5 test packages |

SCORM conformance is self-declared by design of the standard. The claim wording is "conformant, self-tested", never "certified".

The recorded claims, their evidence and known gaps: [learning-standards-conformance.md](learning-standards-conformance.md).

### Package storage and import

- A package is stored as a single archive in the content store and addressed by its hash, like any other element content. A package element in a course document carries that hash. The course document format version does not change: the element type is a new value, and nodes that do not know it show the existing unknown-element notice.
- Import is a pure function from archive bytes to either a parsed package description or a stable refusal code. It detects the standard and edition, parses the manifest or course structure into an activity tree, and validates that every launchable resource exists in the archive.
- Refusal codes cover at least: not an archive, no manifest, manifest unparseable, unsupported standard or edition, launch resource missing, launch resource remote, unsafe entry path, size or count limit exceeded.
- Extraction is bounded: entries with absolute paths, parent traversal or links are refused; total uncompressed size, entry count and compression ratio are capped. Starting limits are 1 GiB uncompressed, 20,000 entries and a 200:1 ratio.
- Extracted files live in a per-profile cache keyed by package hash, following the existing practice of materialising content into a cache file before the player reads it.
- **The archive is the kept copy; the extracted files are a cache.** A device keeps the archive, because the archive is what it serves to other devices under the course access rules. The extracted files can be rebuilt from it at any time, so they are dropped when the learner completes the package or has not opened it for a stated period, and rebuilt at the next launch. Dropping them loses nothing the learner downloaded. A package being taken costs about twice its size; one that is not costs its own size.
- In a private course the archive is stored encrypted like every other file of that course. The extracted files are readable and live only in the profile's local cache, never in the store the node serves from.
- Parsing and validation live in the desktop-free shared crate, per ADR 0004. The app adds only storage and commands.
- A learner opening a package file directly gets a draft course on their own device with one package element. It is never announced or served. It leaves the device only if the learner publishes that course, which is a deliberate act under the course access rules.

### Distribution

- **This spec sets no distribution rules of its own.** A package travels, is announced and is served exactly like every other file of the course it belongs to. Those rules, including the sharing limits each learner sets and the rule that supervised profiles never serve, are in [course-access-spec.md](course-access-spec.md).
- **A package in a public course is redistributed by every device that takes the course.** The course editor says so when a package is added to a course that is or will be public, and asks the instructor to confirm they may pass the content on.
- **Licensed and internal packages belong in a private course**, where files are encrypted and exchanged only among invited members. Private courses need an organisation that proves its domain.
- Until the sharing phase of the course access spec ships, packages follow whatever other course content does at that time.
- The existing size cap on fetching content from a web address applies; larger packages travel peer to peer only.

### Serving and isolation

Package script is untrusted. The content policy alone is not treated as sufficient; each rule below is enforced and tested on its own.

- **Own scheme.** Package content is served under a dedicated scheme, separate from the plugin scheme and from the app's own origin. The app's frame policy is extended to allow it.
- **Launch-scoped addresses.** Every launch gets an unguessable path prefix that maps to one package and is valid only while that launch is open. The handler serves nothing else. This is what isolates packages from each other on platforms where every package shares a single web origin, and it is required on all platforms.
- **No reliance on origin storage.** State persists only through the run-time API. Browser storage, caches and service workers at the package origin are cleared before and after every launch, and service worker registration is blocked.
- **Content policy.** Inline script and script evaluation are allowed, because legacy authoring tools emit both. Requests, frames, media, fonts, images and form submissions are limited to the package's own origin. Plugins and objects are blocked.
- **Network-capable interfaces removed.** The handler injects a bootstrap into every HTML response that removes the app's command bridge and the browser interfaces the content policy does not govern, peer connections first among them. Navigation of the package frame is limited to the package origin by the frame policy at both the app level and the wrapper level.
- **No device capabilities.** Camera, microphone, location, clipboard read and full screen are not granted. A package that opens its content in a new window has that call redirected into the player frame.
- **Host-drawn label.** The player draws a permanent bar outside the frame naming the content as third-party, with the publisher's name, and stating that Alexandria never asks for a recovery phrase inside it. Because full screen is not granted, content cannot cover it.
- **Focus mode.** The player can enlarge the package frame to fill the app window, hiding the course's own chrome and keeping the label as a thin strip. The learner switches it on and off from the label. A full-screen request from content is turned into a focus mode request, which is safe to honour because the label stays in view. The frame never gets the browser's real full screen.
- **Pseudonymous learner identifier.** The learner identifier given to content is derived from the learner's key and the package hash. It is stable across attempts at one package and unlinkable across packages. Content receives the profile's display name, which it needs for things like a completion page.
- Content that depends on a blocked resource or capability degrades, and the player says which one.

### Serving performance

- The scheme handler is asynchronous, honours byte-range requests and reads only the requested range from disk. It never loads a whole file to answer a ranged request. The existing practice of loading a whole media file into memory before playing it is not used for package content.
- Whole-file responses are capped; a file above the cap is served only by range.
- Extraction runs off the interface thread with progress shown, and the first lesson can start once its own files are extracted.

### Runtime

- The run-time API and the SCORM 2004 sequencing engine come from a pinned, vendored open-source runtime library under a permissive licence. Alexandria does not write its own sequencing engine.
- The app serves a wrapper page at the package's origin. The wrapper holds the run-time API objects so that content in a nested frame finds them by the discovery algorithm each standard defines, and calls them synchronously.
- Run-time state is held in memory in the wrapper. Commits from content are coalesced: the wrapper hands state to the host at most once per short interval, and always on termination, on frame unload and when the app is backgrounded. The host persists it in one write. A persistence failure shows a player-level error; the attempt stays resumable from the last stored state.
- The host supplies the pseudonymous learner identifier, display name, launch data, mastery score and prior state at initialisation.
- The player shows the activity tree as a table of contents and provides the navigation controls SCORM 2004 requires. Controls are enabled, disabled or hidden according to the sequencing engine and the package's presentation settings.
- Auto-advance is suppressed for package elements, as it is for plugins.

### Attempts and progress

New per-profile tables, added to the baseline schema with no data migration:

- packages: hash, standard, edition, title, parsed activity tree, import time.
- package attempts: enrolment, element, package hash, attempt number, status, start and end times.
- activity state: per attempt and activity, the run-time data as stored by the content.
- sequencing state: per attempt, the global objective and tracking state for SCORM 2004.

Rollup into existing element progress:

- A package element is complete when the package reports completion: SCORM 1.2 lesson status of completed or passed; SCORM 2004 root activity completion status of completed; cmi5 when the course's completion criteria are satisfied.
- The element's score is the package's reported score normalised to the scale element progress already uses. Time spent is the sum of reported session times.
- Package attempt state is device-local in this spec. Element progress keeps syncing as it does today.

### Credentials

Consistent with ADR 0003:

- A package result is a self-report from untrusted script. It never becomes a graded submission, never contributes credential evidence and never feeds reputation.
- A course's completion policy cannot name a package element as an assessed element; the course editor and document validation both refuse it.
- The player and the course page label package results as self-reported.

### Learning records

- The app writes learning records for enrolment, element launch, progress, completion, pass and fail, and for package activity. It does not record keystroke-level or per-answer detail for Alexandria's own assessments.
- Records have a kind: completion-level (launch, progress, completion, pass, fail, score) or interaction-level (individual responses a package reported). Interaction-level records are written to the local log and leave the device only under a scope that names them.
- Statements are built by a desktop-free module in the shared crate and validated before storage. They are written as xAPI 2.0 and kept within the subset that is also valid 1.0.3.
- Verbs come from the published ADL and cmi5 vocabularies. SCORM run-time data maps to statements following the published SCORM profile for xAPI.
- **Identifiers do not depend on a web domain.** Activity identifiers are DID URLs under the DID that created the course, as fixed in the course's identity, naming the course and the element. They do not change when a course gains co-instructors, changes owner or moves from private to public. Activities inside a package extend the element's identifier with the package hash and the activity's own identifier. cmi5 activities follow the cmi5 rules. The account home page that names the identifier scheme is a fixed, location-independent identifier defined once in the shared crate.
- **The scheme is checked before anything is built on it.** The first ticket of the learning records phase sends hand-built statements carrying these identifiers to the official conformance suite and to at least two external stores from different vendors, and records what each accepts and how each displays them. The scheme is confirmed, adjusted or replaced on that evidence before the statement builder is written.
- **Local records name the learner by DID.** They never leave the device in that form unless the learner chooses it.
- The log has a visible size limit. Past it, the oldest interaction-level records are removed first, then completion-level records are kept as one summary per attempt.
- New tables: learning records, a per-recipient outbox with send state and last error, and the xAPI document stores cmi5 needs.
- The activity log view lists records, exports them and deletes them. An export is a statement file with a detached signature by the learner's key over the whole file. The standard's own per-statement signature format is not used: it requires signature algorithms the learner's key cannot produce.
- Wire structures are duplicated between app and hosted service on purpose, as elsewhere.

### Sending records

Common to both ways of sending:

- **Recipient identifier.** Each recipient gets an actor identifier derived from the learner's key and the recipient, so no two recipients see the same one and none can be linked to the learner's DID. Statement identifiers and registration identifiers are derived per recipient as well, so they cannot be used to join records across recipients either. The learner may instead choose their real DID, or approve an account the recipient proposed.
- **Scope.** A scope names courses and record kinds. The default is completion-level only. Interaction-level is a separate choice shown with a plain description of what it contains.
- **Consent screen.** Shows the scope, the identifier the records will carry, and what revoking can and cannot undo for this recipient.
- **Records from a private course do not name it by default.** They carry activity identifiers and omit the course's and its activities' names, unless the course's owner has allowed its title to be disclosed. The consent screen says the course is private. The switch is defined in the course access spec.
- **Outbox.** Records are batched, queued while offline, retried with backoff, and failures are visible in the activity log.
- Sends are written to the learner-readable access log.

**Record destinations (direct, no hosted service).**

- The learner enters a store's address and the credentials issued to them. The app speaks xAPI to it directly, negotiating the version with the store.
- Credentials are kept with the same protection as other secrets the app holds for a learner. The device holds only credentials given to this learner, never an organisation's shared ones.
- The learner's entry is the authorisation to contact that address. A private or local address needs an explicit confirmation.
- Removing a destination stops sends and issues voiding statements on a best-effort basis. The consent text says records already sent stay with the store's operator.
- This path is core and complete on its own: nothing about it needs the hosted service.

**Record shares (through the hosted service).**

- A record share is a new scope on the existing access request and grant mechanism, with the same signed statements and the same create, sync, narrow and revoke operations. No second consent path is added.
- The device builds each batch and sends it to the organisation's hosted service with a signed request.
- Revoking stops further sends and makes the hosted service delete the records received under that share, keeping only their identifiers so they cannot be re-inserted. For records the service already forwarded, it sends voiding statements to the external store on a best-effort basis.

### Record store (hosted service)

- An ingest endpoint accepts signed batches, checks each statement against the share's scope, stores it and, where configured, forwards it to the organisation's external store through the existing egress controls, with retry.
- The record store implements the xAPI resources: statements with query filters, paging, voiding and attachments; activity state, activity profile and agent profile documents with concurrency control; agents, activities and about. It serves both 1.0.3 and 2.0.
- Each organisation has its own endpoint base and its own interface credentials with read or write scope, managed in the console. Tenant isolation is enforced in the data layer, not only at the route.
- **Queries are designed for size.** The filterable fields are stored as indexed columns beside the statement body. Passing the conformance suite is necessary and not sufficient: a load test at a stated record count is part of the phase.
- **Reads are logged without multiplying writes.** One access-log entry is written per query and per learner whose records it returned, batched in a single write, not one per statement.
- **Named access follows the existing cohort rules.** Records received under a share stay readable by name only while the share is active and the learner's enrolment with the organisation lasts. After either ends, they are deleted or, where the retention policy keeps aggregates, reduced to counts that obey the existing minimum cell size.
- Records follow the organisation's retention policy.
- Statements written by an organisation's own tools are that organisation's data and are not subject to a record share.

### cmi5

- cmi5 packages import through the same path and run in the same player. Only assignable units packaged in the archive are supported; a course structure that launches a remote unit is refused at import.
- The app performs the launching system's duties: launch parameters, the one-time authorisation token, launch data and learner preference documents, the launched statement, validation of the unit's statements against the session rules, satisfied statements for blocks and the course, the abandoned statement on abnormal end, and completion criteria evaluation.
- The actor given to a unit is the same pseudonymous identifier other package content receives. Records the unit writes are rewritten to the recipient identifier when they are sent anywhere.
- Content talks xAPI to an on-device endpoint that exists only while a cmi5 session is open, accepts only that session's token and implements only the resources cmi5 requires, backed by the local record tables. It is not claimed as a conformant record store.
- The endpoint's transport is settled by a prototype ticket before the rest of the phase: unmodified content must be able to send requests with bodies on every supported platform, and a session must survive the app being backgrounded and resumed. A loopback listener is the expected answer; a custom scheme is acceptable only where request bodies reach the handler.
- Units that require their own window open in a separate window on desktop. The mobile behaviour is settled by the same prototype.

### Enterprise boundary

Rows to add to the worked examples when the work lands:

| Feature | Verdict |
|---|---|
| Playing a standards package | Core |
| Writing learning records, the activity log, signed export | Core |
| Sending records directly to a store the learner names | Core |
| The record share consent, the device-built batches | Core |
| The record store, forwarding, interface credentials | Enterprise |

### Sequence

Each phase ships on its own. Phases 1 to 3 need no hosted service. None is blocked by the course access spec, but an organisation's own package library is usually licensed or internal, so "plays our existing SCORM courses" is an honest answer for organisations only once private courses exist. Before that, packages work for files opened from disk and for content an instructor may publish to everyone.

1. SCORM 1.2: range-capable serving, isolation rules, import, runtime, attempts, progress rollup, player, mobile.
2. SCORM 2004 4th Edition: sequencing and navigation, navigation controls, conformance matrix.
3. Learning records in the app: identifier compatibility check first, then statement builder, activity log, signed export, record destinations.
4. Record shares: the grant scope, hosted ingest, forwarding, deletion on revoke.
5. Record store: the full xAPI interface in the hosted service, conformance suite and load test in continuous integration.
6. cmi5: transport prototype, import, launch, session rules, completion criteria.

## Testing Decisions

A good test here drives behaviour a learner, an organisation or an external tool can observe: a package goes in and progress, stored state or a refusal comes out; a statement goes in and a response comes out. Tests do not assert on the runtime library's internals, the wrapper's message shapes or table layouts.

Three seams, one per layer:

1. **Host seam (Rust, in-memory database).** Import a fixture archive, open an attempt, feed commits and termination, and assert on stored attempt state, element progress, written learning records and outbox contents. Also covers every import refusal code, extraction limits, the credential exclusions, scope filtering by record kind, the log size limit, and the identifier rules: two recipients never receive the same actor, statement or registration identifier, and none equals the learner's DID unless chosen. It also covers the two distribution rules that are this spec's own: a package opened from disk is never announced or served, and dropping the extracted cache leaves the archive intact and the package launchable. Records from a private course omit names unless disclosure is allowed. Direct sending is tested against a small in-process xAPI stub. Prior art: the command `_impl` functions tested against an in-memory migrated database, and the end-to-end credential tests.
2. **Player seam (browser harness).** Load real packages into the wrapper and runtime with a fake host bridge, in Chromium and WebKit, and assert on what the content sees and what the host receives. This is where the SCORM 1.2 and 2004 conformance matrices run, using the standards bodies' published test and sample packages plus small purpose-built packages for single requirements. It is also where isolation is proved with hostile fixture packages: each tries one escape (a request, a peer connection, a frame navigation, a pop-up, reading another launch's address, reading storage left by an earlier launch, the command bridge, covering the label in and out of focus mode) and the test asserts it failed. Ranged media requests and commit coalescing are asserted here too. Prior art: the desktop UI checks with fixtures and a fake backend.
3. **Record store seam (HTTP, hosted service).** The official open-source xAPI conformance suite runs against a local instance for both versions in continuous integration. Integration tests in the existing style cover what the suite does not: tenant isolation, share-scope enforcement at ingest, deletion on revoke, the end-of-enrolment rule, access logging, forwarding and retry. Prior art: the hosted service's integration tests against Postgres.

cmi5 is tested through the first two seams with the published cmi5 test packages. The statement builder has unit tests against published example statements, and its output is fed to the record store seam so the two halves are shown to agree, including that DID-based activity identifiers are accepted.

Checks that stay manual and are recorded in the conformance statement: a run on a physical iOS and Android device per phase, covering a package with large video and focus mode; the identifier compatibility check against external stores at the start of the learning records phase, repeated with real records at its end; and the hosted conformance run that issues the certificate.

## Out of Scope

- Exporting Alexandria courses as SCORM or cmi5 packages.
- LTI, AICC and Common Cartridge.
- Remote cmi5 assignable units and any package content loaded from the network.
- Resuming a package attempt on a different device.
- Who can see a course, private courses, course staff, and how and how much devices share: all in the course access spec.
- The standard's per-statement signature format.
- A learner-facing analytics view built on learning records.
- Issuing or weighting credentials from package results in any form.
- Independent certification, where a standard offers none that can be run against a desktop and mobile app.
- Marketing claims on the website; those follow the first recorded conformance statement.

## Further Notes

- **Assumptions made without asking.** The goal is both the procurement answer and real use of existing libraries, so SCORM 2004 sequencing is in. A looser content policy for packages is accepted, confined to its own scheme and backed by the isolation rules above. The hosted service both forwards and stores. Any of these can be reversed by cutting the matching phase.
- **What isolation cannot promise.** The rules above are defence in layers against script that is free to try anything. They are tested with hostile fixtures, not proven. A package still sees what the learner does inside it and its display name; the guarantee is that this goes nowhere.
- **What revoking cannot promise.** Records sent to a store outside the hosted service are in someone else's hands. Voiding marks them invalid; it does not delete them. The consent screen says this before the first send.
- **Recipient identifiers and timing.** Per-recipient identifiers stop joining on identifiers. Two recipients who pool data can still match records by timestamps and course. Coarsening timestamps would break the standard's ordering rules and is not done.
- **DID-based activity identifiers.** They are valid under the standard, which asks only for an IRI, but some reporting tools assume web addresses. The compatibility check that opens phase 3 is where that is found out, before any code depends on the scheme. The preferred fallback is a documented mapping for the tools that need one; a domain-rooted scheme is the last resort and would need its own decision record.
- **Storage cost of serving.** Because every holder serves the archive, a device cannot drop it after extraction as an earlier draft of this spec planned. Treating the extracted files as a rebuildable cache is the answer chosen. Publishing a package as separately addressed files, so that no archive and no second copy exist, would remove the cost entirely; it changes how a package element is addressed and is left for a later decision.
- **Large packages and mobile limits.** The default mobile sharing limit is far smaller than a large package, so phones on mobile data contribute little to serving one. Packages are served by range, so a transfer that stops at a limit continues from another holder.
- **Official SCORM test suites.** They were built for browsers and plug-ins that no longer exist. Whether any part can be driven through the player seam is a research ticket at the start of phase 2; the conformance matrix does not depend on the answer.
- **Runtime library.** The candidate is `scorm-again` (MIT, actively maintained, covers both SCORM versions including sequencing). Phase 1 begins by confirming it against the wrapper design; if it falls short on sequencing, the gap is fixed upstream or in a vendored patch, not by a rewrite.
- **Legacy content limits.** Packages that depend on browser plug-ins will not run anywhere. Packages built for a fixed desktop size run on a phone but may be hard to use; the player does not reflow them. The browser's real full screen is unavailable to package content; focus mode stands in for it.
- **Licensing of package content** is the publishing instructor's responsibility, as for any other course content.
- **Documentation to update with each phase**, proposed alongside the code: the glossary, the database schema reference, the architecture and protocol documents for the new element type, the enterprise boundary examples, the plugin document's note on element dispatch, and the hosted service's organisation and deployment documents.

## Notes

- 2026-11-17: Spec written from the compliance assessment. No code yet.
- 2026-11-18: Revised after a privacy, performance and decentralisation review. Added: pseudonymous identifier to content, per-recipient identifiers, completion-level default scope, direct record destinations with no hosted service, domain-independent identifiers, signed export, isolation rules stated instead of deferred, range-capable serving, single storage copy, commit coalescing, log limit, no announcement of package holdings, record store query and access-log design, end-of-enrolment rule for named records. Phases resequenced so the first three need no hosted service.
- 2026-11-19: Added a per-course sharing setting (off by default), focus mode in place of full screen, and an identifier compatibility check as the first ticket of the learning records phase.
- 2026-11-20: Reassessed against the course access spec. Removed this spec's own distribution rules: the no-announcement rule, the per-course sharing setting and the single-copy rule. A package now follows its course. Added: the archive as the kept copy with extracted files as a cache, the warning when a package joins a public course, private courses as the place for licensed packages, names omitted from a private course's records by default, and activity identifiers anchored to the course's creator so they survive a change of owner, staff or visibility. Stories 13, 22, 23 and 50 were rewritten to match.
