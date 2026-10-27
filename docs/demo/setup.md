# Assessment → credential → Cloud demo: setup

Status: live
Updated: 2026-10-27

The demo runs Alexandria and Alexandria Cloud side by side on one machine. It
shows:

- a learner earning and publishing a skill credential;
- an organisation requesting an assessment or an existing credential;
- an organisation asking for access to some of the learner's skills, and the
  learner granting, syncing and revoking it;
- an interview held inside Alexandria, with a panel that sends scorecards;
- an offer answered with the learner's signature;
- the validity pilot workspace.

It does not need Jev or an AI provider key.

| Document | Use it for |
|---|---|
| This page | Preparing the machine, what is seeded, limits, test data |
| [Runbook](runbook.md) | The click path, what to answer, and what to do when something does not arrive |
| [Presenter notes](presenter-notes.md) | A one-page cue card for the presenter |

## Prepare the machine

1. **Alexandria.** Open the installed app (built from this repository) and
   unlock the demo profile. Profile activation runs migrations and the bundled
   seeds automatically: the public taxonomy, synonyms, goal templates, question
   banks and items, bootstrap trust data, all nine bundled plugins, eleven
   profile-owned course drafts (eight examples, Plugins Showcase and two video
   labs) and three classroom templates. This happens on profile creation,
   mnemonic restore and unlock; locked profiles are seeded on their next
   unlock. There is no `db seed` CLI command. Unlock must succeed before
   assessment content is ready. Existing attempts and credentials are kept.
2. **Cloud.** Start Docker Desktop. From the Cloud checkout run
   `scripts/demo-assessment.sh` and wait for "listening on 127.0.0.1:8787".
   The launcher creates the empty demo organisation, activates the exported
   taxonomy, builds the console if `web/dist` is missing, and starts Ollama for
   the role-requirements step. Nothing else is seeded: no candidates,
   credentials, interviews or pilots.
3. **Organisation identity.** The launcher gives the organisation the signing
   key in `.demo/org-signing-seed` (when that file exists), claims
   `alexandria.ifftu.dev` for it as **IFFTU**, and verifies the claim against
   the DNS TXT record `_alexandria.alexandria.ifftu.dev`, which must read
   `v=alexandria-org1; name=IFFTU; key=<did:key of that seed>`. If the record
   is missing or lists another key, the launcher warns and carries on, but
   Cloud refuses to send assessment requests, access requests, interview
   invitations and offers until the claim verifies. **Settings → Organisation
   identity** in Cloud shows the status and the exact record to publish. Both
   Cloud and the app read the record from public DNS, so the machine needs a
   network connection even though everything else is on loopback.
4. **Sign in to Cloud.** Open <http://127.0.0.1:8787> and choose **Continue**.
   This is a loopback-only development sign-in, not a production identity
   provider. **Settings → Organisation identity** should say **Verified**.
5. **Point the app at Cloud.** In Alexandria, **Settings → Developer →
   Alexandria Cloud address** = `http://127.0.0.1:8787` (setting
   `organisations.cloud_origin`; it saves on Enter or when you leave the
   field). **Settings → Organisations → Receive requests from organisations**
   is on by default, and the section should show *Requests* rather than "No
   Alexandria Cloud is set up". Both apps use the exported bundled taxonomy.
6. **Pick the skills.** Use **JavaScript** (`skill_javascript`) or **Big-O
   Analysis** (`skill_big_o`), which have bundled MCQ banks. Each attempt draws
   three randomised questions and needs at least 70%, so all three must be
   right.
7. **Use a fresh profile**, or a skill with no recent attempt. Normal attempt
   limits and cooldowns apply. An interrupted attempt counts; do not reload to
   change the draw.

The launcher keeps Postgres in the `alexandria-assessment-demo-db` Docker
container on loopback port 5544. The database `alexandria_demo` is separate
from `alexandria_demo_test`. Local session configuration and the taxonomy
export live under the ignored `.demo/` directory. Stopping the server or
container keeps the demo data; no other development database is reset.

To rehearse the whole path in one command, and the optional Cloud console
walkthrough, see [runbook §0](runbook.md#0-before-the-audience-arrives-10-minutes).

## Sentinel Dev

Press **⌘⇧S** in the unlocked app, or click **Sentinel Dev** during an
assessment. **Start camera preview** and allow the camera prompt. The feed,
face box, landmarks and gaze overlay work without diagnostics. During a
standalone skill assessment the same camera feeds face and gaze signals to
monitoring. Closing the panel or stopping the camera ends capture.

Outside an assessment the panel reports idle; typing, mouse and integrity
measurements start with an assessment. Capture and inference errors appear in
the panel. Do not open diagnostics during an assessment: that consumes the open
attempt under normal policy.

Low-data signals can show unavailable rather than a score. Three MCQ clicks do
not supply enough typing data to train or demonstrate every behavioural model.
Sentinel reports local integrity observations; it does not establish identity
or an independently proctored result.

## What the audience should understand

- The bundled factual MCQs attest **Remember**, even with a perfect score. A
  percentage score is not a Bloom rank.
- A device-issued credential is **learner-issued** and contributes **zero
  independent issuer clusters**. A valid signature is not an endorsement of
  job suitability.
- A public listing, sending a full credential to one organisation, and an
  access grant are three separate consents. Sending a credential does not
  publish a listing.
- The organisation's name in the app comes from the DNS TXT record on its own
  domain, which the app reads and checks the signature against itself. A
  request that does not verify is shown with a warning and cannot be answered.
- Cloud stores the exact signed disclosure and its verification result. The
  payload carries the signed integrity summary, not camera frames, keystrokes
  or mouse traces. The disclosure preview names the organisation and purpose.
- Verification reports **accept**, **pending** or **reject** at receipt time.
  Self-issued lifecycle state is an issuer assertion inside the holder-signed
  disclosure. Third-party status Cloud cannot read stays pending; the holder
  cannot assert another issuer's status.
- Invitations expire after 14 days. Shares are valid for at most five minutes
  and bind the holder, request, audience, nonce, skill, network and taxonomy.
  Exact retries are idempotent; a different second submission conflicts.
- Recorded results are kept for 180 days after receipt. Unanswered invitations
  have a 180-day retention deadline from creation and are marked expired by the
  retention worker. Deleting a retained run also deletes its disclosure.

## Recovery and limits

- If final telemetry persistence fails, the attempt stays ungraded and the
  same frozen answers can be retried.
- If monitoring finalised but grading failed, reopening the same assessment or
  request recovers the frozen submission. Changed item contents cannot silently
  alter grading.
- If monitoring was interrupted before a terminal summary existed, start a new
  attempt under the normal policy.
- If sharing fails, keep the preview open and retry the same payload within
  its five-minute lifetime. After expiry, create a new preview if the request
  is still pending. Refresh Cloud before creating another request after a lost
  response.
- A durable offline send queue and continuously refreshed issuer-status
  fetching are follow-up work, not part of this demo.

The demo source setup does not replace profile storage. The installer keeps a
copy of the previous application bundle; the installation receipt says where.

## Seeded content

### Instructor drafts and the plugin showcase

Switch to **Instructor → My courses**. The example drafts (inline lessons and
quizzes) and Plugins Showcase are already installed for the current profile.
**Load example courses** and **Load plugin showcase** restore anything missing
or retry plugin installation; repeating an import or unlocking again keeps
edits. Publishing uses the normal signed publication flow. Retired placeholder
media, fabricated personas and learner results are not imported.

Plugins Showcase has six plugin lessons; activation installs the bundled
manifests, UI and graders. Open **Plugins Showcase → Review → Preview**, then
**Prepare and enroll** and **Continue learning**. Microphone access is
requested only when a music lesson needs it. No scores, credentials or reviews
are seeded.

**Prepare and enroll** on an owned draft uses the same signed course document
and blob preparation as publication, keeps the course in draft, and skips the
catalogue entry and P2P announcement. Enrolment stays bound to the verified
document CID and version; unsigned seed data is not accepted. The player sends
unenrolled learners through the same plugin pre-flight, and errors appear
inline.

### Discussions

The **Discussions** feed holds credential-qualified peer publications:
text/link/video threads, nested comments (eight levels), one up/down vote per
DID, new/top/most-discussed sorting, author edits, tombstone deletion and
signed reports. Reports do not remove posts automatically. Posting and
commenting use the topic qualification checker. The seven read-only example
viewpoints are gone; activation deletes only those known bundled rows. Old
`/opinions` links still work; navigation uses `/discussions`.

The pinned demo policy trusts the `demo` profile's public DID
`did:key:z6MkrgAA1UeVKMFvnerXN4S4Z1cTnzY31Adih31whBp9KY5U` for `sf_cs` and
`sf_web` at Apply (2) or above, through the accepted-issuer or accepted
course-endorsement routes. Owning that profile grants nothing by itself and
creates no qualifying credentials.

Before publishing, the UI discloses that the post and the qualifying
credential or endorsement evidence are shared with peers. Uploaded media is
public content-addressed data (25 MiB limit), not a private attachment.
Connected nodes relay and validate signed events on the Opinions gossip topic.
Offline delivery needs a connected peer that kept the history; there is no
central archive.

The rehearsal learner profile (`demo_learner`, endorsed by `demo`) has eight
published demo threads and six comments, including a nested reply and author
starter replies, all by that one learner through the normal composer. They are
labelled demo prompts and do not represent independent participants. Other
profiles receive them through the normal peer exchange; seeding never
publishes under a new user's identity. Live receipt on a second device has not
been verified.

### Completion endorsements

The completion endorsement request includes the exact public signed course
document, so it can be reviewed on another profile or an offline device.
Signing verifies its BLAKE3 CID and author signature, matches course ID and
version, then enforces the document's attestor policy. Bare binding requests
still work when the instructor already has the exact document. In rehearsal,
`demo_learner` completed the Full-Stack course checkpoint (3/3), the `demo`
profile signed the completion request, and the endorsement was verified and
imported back into the learner profile with the policy satisfied.

### Classrooms, live rooms and video labs

Three owned classrooms (Algorithms Study Group, Web Development Cohort, Design
Critique Circle) provide seven starter channels. Only the current user is a
member; there are no invented conversations, participants or past calls.
Classrooms and live tutoring fill the content area; on phone and tablet,
channels and members open as drawers.

Tutoring attendance uses signed room heartbeats, independent of camera and mic.
A local observer does not count as an attendee. Empty rooms expire after 300
seconds (checked every ten seconds while unlocked). A known ended invite is
rejected; an unfamiliar invite needs a reachable participant before media
opens. A partition cannot prove global absence, so unreachable sessions show
an explanation rather than claiming the host ended.

**Algorithm Essentials — Video Lab** and **Web Foundations — Video Lab** each
have four narrated videos and four transcript lessons; use **Prepare and
enroll**. The 15 MP4s (eight lessons, seven retained discussion clips) ship
with the binary, are copied into each profile's content store, and are pinned
for offline playback. They are labelled AI-generated with synthetic narration.
Sources and transcripts are in `demo-world/content/resources.json`; regenerate
on macOS with Python + Pillow, FFmpeg and `scripts/generate-demo-videos.py`.
Course thumbnails are backfilled on unlock only when both thumbnail fields are
empty.

The temporary `AUTO_SEED_DEMO_CONTENT` switch in
`src-tauri/src/commands/demo_courses.rs` controls installation on profile
activation. Fabricated credentials, reputation, chain observations, devices,
sync history, tutoring history and retired governance records are excluded.

### Governance

Governance supports signed founding-genesis review and pinning. The
[temporary single-operator preprod genesis](../default-network-genesis.md) is
bundled and verified against its DAO ID, exact BLAKE3 envelope digest, network
scope and all 21 founder signatures. Creating or restoring a profile pins it
automatically; unlocking an existing profile does not change its anchors.
**Community** shows the status and offers explicit acceptance for existing
profiles. A live proposal and voting demo would also need committee services,
qualified-voter admission, signed vote receipts and outcomes, verified
ingestion, and UI. The former local DAO/proposal implementation must not be
used as a substitute.

## Development test-data picker

Debug builds, and optimised builds with the Cargo feature `dev-seeding`, have
**Settings → Developer → Test data** (on phones, tap **Developer** in the
settings list). It lists the bundled resources: nine example course drafts, two
video courses, nine plugins, fifteen question banks, individual goal templates,
three classrooms, fifteen videos, seven thumbnails, six local discussion
prompts, and eight proposal-style discussion drafts (one per bundled subject
field, with that field preselected in the composer). There are no governance
proposal, ballot or tally tables to seed. Development profiles get the
required taxonomy, synonyms and global plugins on startup; optional content is
opt-in rather than added on each unlock. Standard release builds keep the
temporary automatic demo rollout. Onboarding and the Goals picker read the
bundled role/exam/curriculum catalogue offline even with no goal-template rows.

- **Seed:** choose resources (or **Seed everything**) → **Review selection**,
  which lists dependencies in installation order → **Seed these resources**.
  One run reports added, kept and failed resources. Retry is idempotent and
  keeps edited rows. The review shows the current resource and whether it is
  compiling a grader or installing data. Grader compilation runs on a blocking
  worker outside the database executor, so other queries continue.
- **Reset:** **Reset selected** and **Reset all seeds** preview the local
  records to remove (edits, enrolments, progress, notes, attempts, classroom
  messages) and need a confirmation checkbox. The backend binds the
  confirmation to a digest of the affected records and rejects a stale review.
  Reset runs in one transaction; required global plugins, active assessments
  and calls, and shared dependencies are kept with an explanation. Issued
  credentials, completion evidence, integrity history, learner goals and
  public P2P copies remain. Media pins and plugin registrations are removed;
  cached immutable files stay for reuse.
- Commands need the active profile session lease and refuse writes in release
  builds. No attempts, credentials, enrolments, votes or signed public
  discussions are synthesised. Discussion prompts open in the normal composer
  with its credential gate and publication consent.

**Android performance retest:** use a profile with missing code-editor plugins,
and record the commit, added/kept/failed counts and the per-resource `seed ...
completed in ...` log timings. An all-kept retry only shows idempotence. Do not
reset learner data just to get a fresh sample. The 2026-10-22 Android 16 run
(ARM64 debug APK) logged 53 added, 26 kept, none failed, 56.5 s of summed
per-resource work; each JavaScript/TypeScript/C++ editor took about 8.5 s and
Python 18.3 s (previously 74–156 s per editor plugin). These are summed
resource timings, not end-to-end UI timings.

**CI artifacts with test data:** dispatch **Validate (Desktop)** and
**Validate (Mobile)** with a non-`v` tag, `dev_seeding=true` and every platform
enabled (iOS with `ios_export_method=debugging`). See
[Releasing](../releasing.md#cutting-a-release). Test installers use the normal
application identity, so they can replace an installed app; profile data is
kept. Desktop validation keeps the updater configuration needed for startup
but disables its endpoints and artifacts. A successful build is not evidence
of camera or P2P behaviour on a device; record those checks separately.
