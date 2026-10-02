# Assessment → credential → Cloud demo

This demo uses the `codex/assessment-demo` worktrees of Alexandria and Alexandria Cloud. It supports both a learner publishing an earned skill and an organization requesting an assessment or an existing credential. It does not require Jev or an AI provider key.

## Prepare this Mac

1. Open the installed **Alexandria** application and unlock the demo profile. Profile activation runs migrations and the current bundled seeds automatically: the public taxonomy, synonyms, goal templates, question banks/items, bootstrap trust data, all nine bundled plugins, and ten profile-owned course drafts (seven examples, Plugins Showcase, and two video labs), seven local Opinion examples, and three classroom/channel templates. This applies to every profile on creation, mnemonic restore, and unlock; locked profiles are seeded on their next unlock. There is no separate `db seed` CLI command in this branch. Unlock must succeed before assessment content is ready. Existing learner attempts and credentials are preserved.
2. Start Docker Desktop. From the Cloud checkout run `scripts/demo-assessment.sh`. Open <http://127.0.0.1:8787> and choose **Continue**. This is a loopback-only development sign-in, not a production identity-provider deployment.
3. In Alexandria, open **Settings → Directories** and add `http://127.0.0.1:8787`, named **Local demo**. Both applications use the exported bundled taxonomy. The launcher creates only an organization; it does not seed candidate assessment results.
4. Use **JavaScript** (`skill_javascript`) or **Big-O Analysis** (`skill_big_o`). These have bundled MCQ banks. Each attempt draws three questions and requires at least 70%; with three equally weighted questions, all three must be correct. Questions/options are randomized.
5. For the instructor demo, switch to **Instructor → My courses**. The seven AI-generated course drafts with inline lessons and quizzes and Plugins Showcase are already installed for the current profile. **Load example courses** and **Load plugin showcase** remain available to restore missing examples. Repeating an import or unlocking again preserves edits. Open a draft in the composer; publishing uses the normal signed publication flow. Retired placeholder media, fabricated personas, and learner results are not imported.
6. Use a fresh demo profile or a skill with no recent attempt when rehearsing. Normal attempt limits and cooldowns apply. An interrupted attempt counts; do not repeatedly reload to change the draw.

The launcher keeps Postgres in the `alexandria-assessment-demo-db` Docker container on loopback port 5544. The database `alexandria_demo` is separate from `alexandria_demo_test`. Local session configuration and the taxonomy export live under the ignored `.demo/` directory. Stopping the server/container preserves the demo data. No other development database is reset.

## Show Sentinel Dev

Press **Command–Shift–S** in the unlocked app, or click **Sentinel Dev** during an assessment. Click **Start camera preview** and allow the normal camera permission prompt. The feed, face box, landmarks, and gaze overlay work without diagnostics. During a standalone skill assessment, this camera also supplies face/gaze signals to monitoring. Closing the panel or stopping the camera ends its capture.

Outside an assessment, the panel explicitly reports idle; typing/mouse/integrity measurements begin with an assessment. Capture and inference errors appear in the panel. Do not enter diagnostics during an assessment: that consumes the open attempt under normal policy. Diagnostics is not needed to show the live feed.

Low-data signals can show unavailable rather than a score. Three MCQ clicks do not supply sufficient typing data to train or demonstrate every behavioral model. Sentinel reports local integrity observations; it does not establish independent identity or an independently proctored result.

## Demo A — learner first

1. In Alexandria, open **Goals → Engineering Manager → View path**, then **Assess** beside **Big-O Analysis**. To add a separate goal, use **Skills & Credentials → Browse → Big-O Analysis → 🎯 Goal**, then view its path. Big-O has no prerequisite; JavaScript’s goal path requires HTML & CSS first. Use JavaScript for the invitation-first path.
2. Open Sentinel Dev and start the camera preview if wanted. Answer the questions and submit. Answers freeze before the last telemetry snapshot and session finalization; grading then issues one credential on a pass.
3. Open the credential from the result. Show its issuer/subject DID, skill, score, Bloom level, terminal integrity assertion, and evidence references.
4. In **My profile → Talent index**, select the earned skill and optional display name, review the exact record, and save consent.
5. In **Settings → Directories**, publish the listing to **Local demo**.
6. In Cloud **Talent**, select the same skill, **Remember**, and **0 minimum issuer clusters**. The candidate appears as a holder-published listing. Choose **Add to candidates**.
7. On the candidate page, choose **Invite to assess or share a credential**. Select the skill and purpose; uncheck **Require a new assessment** to request the credential just earned.
8. In Alexandria **Settings → Directories → Assessment and credential requests**, refresh, choose the request, choose the credential, and **Preview disclosure**. Show the exact payload, then explicitly share it.
9. Refresh the Cloud run. Show the same credential ID, signature verification decision, learner-issued label, and integrity result.

## Demo B — organization first

1. In Cloud **People**, add a candidate using the demo profile's DID (or use the candidate from Demo A).
2. On that candidate, create a request with **Require a new assessment** checked. Choose an available bank, role label, and purpose. If reusing the profile from Demo A, choose the other bundled skill to avoid its normal cooldown.
3. In Alexandria, refresh **Assessment and credential requests** and choose **Take requested assessment** on that invitation. This binds the attempt and resulting credential to this exact request and nonce.
4. Show Sentinel Dev during the assessment. Pass, return to directories, choose the newly earned credential, preview, and share.
5. Refresh the Cloud run. Open the candidate to show the run there as well. An older credential cannot satisfy a request requiring a new assessment.

## What the audience should understand

- The bundled factual MCQs attest **Remember**, even with a perfect score. Percentage score is not a Bloom rank.
- A device-issued credential remains **learner-issued** and contributes **zero independent issuer clusters**. A valid signature is not an endorsement of job suitability.
- Public listing consent and sending a full credential to one organization are separate actions. Sending a credential does not publish a listing.
- Cloud stores the exact signed credential disclosure and its verification result. The payload contains the signed integrity summary, not raw camera frames, keystrokes, or mouse traces. The disclosure preview identifies the organization and purpose.
- Verification reports **accept**, **pending**, or **reject** at receipt time. Self-issued lifecycle state is an issuer assertion inside the holder-signed disclosure. Third-party status unavailable to Cloud remains pending; the holder cannot assert another issuer's status.
- Invitations expire after 14 days. Shares are valid for at most five minutes and bind the holder, request, audience, nonce, skill, network, and taxonomy. Exact receipt retries are idempotent. A different second submission conflicts.
- Recorded results are retained for 180 days after receipt. Unanswered invitations have a 180-day retention deadline from creation and are marked expired by the retention worker. Deleting a retained run also deletes its disclosure.

## Recovery and limits

If final telemetry persistence fails, the attempt stays ungraded and the same frozen answers can be retried. If monitoring finalized but grading failed, reopening the same assessment/request can recover the frozen submission. Changed item contents cannot silently alter grading. If monitoring was interrupted before a terminal summary existed, start a new attempt under the normal policy.

If sharing fails, keep the preview open and retry the same payload within its five-minute lifetime. After expiry, create a new preview if the request is still pending. Refresh Cloud before creating another request after a lost response. A durable offline send queue and continuously refreshed issuer-status fetching are follow-up work, not part of this demo.

The installed app is built from this worktree. Main checkouts and profile storage are not replaced by the demo source setup. The installer preserves a copy of the previous application bundle; see the installation receipt for its location.

## Live role requirements

In Cloud, open **Hiring → Role requirements**. Paste the job description during
the demo, or use **Load from link** with a public HTTPS URL and review the loaded
text. Click **Read it**, review the quoted skill proposals and Bloom levels,
then supply a code/title and create the role. Preferred proposals start unchecked.
The local Ollama model is configured by the Cloud demo launcher; allow up to two
minutes. **Create role manually** is available as a fallback. Coverage reflects
real published listings, and Remember evidence will not clear Apply/Analyze bars.
See the Cloud runbook for model startup and configuration.

### Plugin showcase

In **Instructor → My courses**, open **Plugins Showcase**. Profile activation
imports this owned draft with six plugin lessons and installs the bundled
manifests, UI, and graders automatically. **Load plugin showcase** can restore
a missing course or retry plugin installation; existing course edits are preserved. Open **Plugins Showcase → Review → Preview** to view the
course; choose **Prepare and enroll** to create its signed local version, then
**Continue learning** to run its lessons. Microphone access is requested only when a
music lesson needs it. No scores, credentials, or reviews are seeded.

### Local draft enrollment

Owned drafts offer **Prepare and enroll**. This uses the same signed course
document and blob preparation as publication, keeps the course in draft status,
and skips the catalog entry and P2P announcement. Enrollment remains bound to
the verified document CID/version; it does not accept unsigned seed data.
The player redirects unenrolled learners through the same plugin pre-flight.
Enrollment/preparation errors appear inline rather than only in the console.

### Opinion examples, classrooms, and video labs

Every profile receives seven read-only examples under **Opinions → Example
viewpoints**, each with a locally bundled narrated video. They use a separate
`demo_opinion_examples` table and have no author, signature, credential claims,
or network publication. Normal Opinion posting keeps its qualification rules.
The text adapts the retired seed topics; fabricated identities and proofs are
not restored.

Three owned classrooms (Algorithms Study Group, Web Development Cohort, and
Design Critique Circle) provide seven starter channels. Only the current user
is a member; there are no invented conversations, participants, or past calls.
Existing edits are preserved on subsequent unlocks.

**Algorithm Essentials — Video Lab** and **Web Foundations — Video Lab** each
contain four narrated teaching videos and four transcript lessons. Use
**Prepare and enroll** as for the other drafts. The 15 original MP4s (eight
lessons and seven Opinion examples) ship with the binary, copy into each
profile’s content store, and are pinned for offline playback. They are clearly
labelled AI-generated with synthetic narration. Sources/transcripts live in
`demo-world/content/resources.json`; regenerate on macOS with Python + Pillow,
FFmpeg, and `scripts/generate-demo-videos.py`.

The seed inventory also contained fabricated credentials, reputation, chain
observations, devices, sync history, and tutoring history. These are not
content resources and remain excluded, as do retired governance records.

## Community demo update

Course thumbnails are backfilled on unlock only when both thumbnail fields are
empty; user artwork is preserved. The temporary `AUTO_SEED_DEMO_CONTENT` switch
in `commands/demo_courses.rs` controls profile-activation demo installation.
Seven bundled Opinion previews now have local JPEG thumbnails. These remain
read-only examples, not fabricated public posts or credentials.

The new Opinions feed supports text/link/video threads, nested comments (eight
levels), one up/down vote per DID, new/top/most-discussed sorting, author edits
and tombstone deletion, and signed reports. Reports do not automatically remove
posts. Both posting and commenting use the existing topic qualification checker.
The pinned demo policy trusts `demo`'s public DID
`did:key:z6MkrgAA1UeVKMFvnerXN4S4Z1cTnzY31Adih31whBp9KY5U`
for `sf_cs` and `sf_web` at Apply (2) or above, through accepted issuer or
accepted course endorsement routes. This does not grant eligibility merely for
owning that profile and does not create any qualifying credentials.

Before publishing, the UI discloses that post/comment content and the qualifying
credential/endorsement evidence are shared with peers. Uploaded media is public
content-addressed data (25 MiB limit), not a profile-encrypted private attachment.
Connected nodes relay and independently validate signed events on the existing
Opinions gossip topic. Offline delivery needs a connected peer retaining the
history; there is no central archive. Older video opinions remain readable.

Classrooms and live tutoring occupy the available content area without outer
page padding. Phone/tablet classrooms use channel/member drawers; composer and
bottom navigation remain separate. Sidebar name animations measure actual
text overflow, pause independently, and show the full name on hover/focus.
Reduced-motion preferences disable animation.

Tutoring attendance uses signed room heartbeats, independent of camera/mic
state. A local observer does not count as an attendee. Empty rooms expire after
300 seconds, checked every ten seconds while unlocked; reopening reconciles
stale records after a short reconnect window. A known ended invite is rejected;
an unfamiliar invite needs a reachable participant before opening media.
Network partitions cannot prove global absence: unreachable sessions show an
availability explanation rather than claiming a remote host definitely ended.

### Opinions presentation and authored quizzes

The feed, published thread, and bundled-video reading page share the app’s card and typography styles. Bundled examples remain read-only; published comments and votes must come from real profile actions after topic qualification. The classroom channel/member surfaces use the same card tint as the course sidebar.

Quiz-editor questions now persist point weights. Older editor questions without weights count as one point each; invalid nonpositive weights are rejected. Failed quizzes can be retried until course completion. Completion updates remain bound to the lesson submitted even if the learner navigates while saving.

Governance supports signed founding-genesis review and pinning. The [temporary single-operator preprod genesis](default-network-genesis.md) is bundled and verified against its DAO ID, exact BLAKE3 envelope digest, network scope and all 21 founder signatures. Creation and mnemonic restoration pin it automatically; unlocking an existing profile does not change its anchors. Community shows the default status and offers explicit acceptance for existing profiles. A live proposal/voting demo additionally needs committee services, qualified-voter admission, signed vote receipts/outcomes, verified ingestion and application, and the corresponding UI. The former local DAO/proposal implementation must not be used as a substitute.

The completion endorsement request includes the exact public signed course document so it can be reviewed on a different profile or offline device. Signing verifies its BLAKE3 CID and author signature, matches course ID/version, then enforces the document’s original attestor policy. Bare binding requests remain supported when the instructor already has the exact document locally.

The installed-app rehearsal completed the Full-Stack course checkpoint as `demo_for_ashish` (3/3), signed its exact completion request under the separate `demo` profile, and verified/imported the endorsement back into the learner profile. The app confirmed one valid endorsement and a satisfied policy. Two clearly labelled demo threads, three comments including a nested reply, and three upvotes were then published through normal app controls. They all belong to the same demo learner; they do not represent independent participants or a real person's assessed competence. Live two-device receipt remains unverified.

On macOS, the media permission delegate forwards other optional UI callbacks to the retained original Wry delegate. This preserves native file selection for thumbnails/video while retaining the existing camera/microphone permission decisions. A native regression test exercises the forwarded open-panel callback. Course details link to the current composer route; course-settings reads and writes clone the raw tutor configuration rather than a Vue proxy.

Build `41e9c6c` was installed and verified on the Mac: native thumbnail and MP4 uploads, published video playback, persisted discussion interactions, and Sentinel live camera/face/gaze preview all worked. Course settings saved successfully through **Instructor → My Courses**. Switch to Instructor mode before editing; the mode guard still redirects the learner-mode Edit Course entry to Home. The learner profile was left unlocked on Opinions with camera/video stopped.

The subsequent installed build `d998ec63` includes the signed default-network demo genesis. In the native app, `demo_for_ashish` shows **Pinned for this profile**; its displayed DAO ID and envelope hash match the bundled public document. The status persisted after navigating away and returning. The app was left unlocked on Community, ready to show the temporary single-operator label and expand the verified founding facts.
