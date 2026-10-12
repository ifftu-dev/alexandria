# Demo runbook — learner to offer, and the validity pilot

Everything needed to run the full demo without getting lost: what is where, in
which order to click, what to say, and what to do when something does not
arrive. The product narrative is in [`assessment-demo.md`](assessment-demo.md);
this is the operator's sheet.

## Where everything is

| Thing | Location |
|---|---|
| Learner app source | this repository (`alexandria`), `main` |
| Cloud source | the `alexandria-cloud` repository, `main`, checked out next to this one |
| Cloud console | <http://127.0.0.1:8787> (sign-in page → **Continue**; loopback dev sign-in only) |
| Cloud Postgres | Docker container `alexandria-assessment-demo-db`, loopback port 5544, database `alexandria_demo` |
| Cloud launcher | `scripts/demo-assessment.sh` in the Cloud repository (needs Docker Desktop running) |
| App Cloud setting | Alexandria → **Settings → Developer → Alexandria Cloud address** = `http://127.0.0.1:8787` (setting `organisations.cloud_origin`; in a build without the Developer section it is under **Settings → Advanced**). **Settings → Organisations → Receive requests from organisations** must be on, which is the default |
| Learner inbox (requests, interviews, offers) | Alexandria → **Settings → Organisations**. *Requests* holds access requests, assessment requests, interview invitations and offers in one list, newest first, then *Upcoming interviews* and your own answers under *Your answers*. Below it, *Who has access* |
| Organisation identity | Cloud → **Settings → Organisation identity**. The demo organisation claims `alexandria.ifftu.dev` as **IFFTU**. The proof is the DNS TXT record `_alexandria.alexandria.ifftu.dev` = `v=alexandria-org1; name=IFFTU; key=did:key:…`; the key's seed is `.demo/org-signing-seed` in the Cloud checkout |
| Cloud access requests and grants | Cloud → **Hiring → Access** (`/access`), and the **Access** panel on a person record |
| Panel seat | The panelist's join link, `alexandria://interview/join?cloud=…&id=…&panelist=…&token=…`, opens `/panel/:id` in the desktop app; the review is `/panel/:id/review`. Neither needs instructor mode |
| Candidate's interview room | `/interview-room/:id`, from **Join interview** under *Upcoming interviews* |
| Credentials | Alexandria → **Skills & Credentials** (`/credentials`); a skill card lists the credentials behind it, detail at `/credentials/:id`; **Export all** downloads `alexandria-credentials-<date>.json` |
| Public listing consent | Alexandria → **My profile → Let employers find you** |
| Dev test data | Alexandria → **Settings → Developer → Test data** (debug or `dev-seeding` builds only) |
| Sentinel live view | ⌘⇧S inside the unlocked app |
| Cloud talent search | Cloud → **Talent** (`/talent`) |
| Cloud people / candidate record | Cloud → **People** (`/candidates`, `/candidates/:id`) |
| Cloud assessment runs | Cloud → **Assessments** (`/runs`, `/runs/:id`) |
| Cloud interviews and offers | Cloud → **Hiring → Interviews and offers** (`/interviews`, `/interviews/:id`) |
| Cloud validity pilot | Cloud → **Operations → Validity pilot** (`/pilot`, `/pilot/:id`) |
| Independent verifier | `scripts/demo/verify-credential.mjs` (Node standard library only; see §6) |
| Hosted status list | `http://127.0.0.1:8787/status-lists/<issuer did>/1` — the URL every credential issued while the app points at the local Cloud names in `credentialStatus`; `GET` it in a browser |
| Status list host setting | Alexandria → **Settings → Privacy → Status list host** (empty = the Alexandria Cloud the app talks to stands in, so the local Cloud is used) |
| Third-party DID resolver | <https://dev.uniresolver.io/> — paste the issuer `did:key:…` |

Bundled assessable skills: **JavaScript** (`skill_javascript`) and **Big-O
Analysis** (`skill_big_o`). Three questions each, 70% to pass, so all three
must be right. Use a fresh profile or alternate the two skills; the normal
attempt cooldown applies.

## 0. Before the audience arrives (10 minutes)

1. Start Docker Desktop. In the Cloud repository run `scripts/demo-assessment.sh`.
   Wait for "listening on 127.0.0.1:8787". The script creates the empty demo
   organisation, activates the exported taxonomy, builds the console if
   `web/dist` is missing, and starts Ollama for the role-requirements step. It
   then gives the organisation the signing key in `.demo/org-signing-seed`
   (when that file exists), claims `alexandria.ifftu.dev` for it as **IFFTU**,
   and verifies the claim; a verified claim renames the organisation to
   **IFFTU**. Nothing else is seeded: no candidates, credentials, interviews or
   pilots.
   - **Prerequisite:** the DNS TXT record `_alexandria.alexandria.ifftu.dev`
     must read `v=alexandria-org1; name=IFFTU; key=<did:key of that seed>`.
     Cloud → **Settings → Organisation identity** shows the exact record to
     publish. If the record is missing or lists another key, the script prints a
     warning and keeps going, but Cloud refuses to send assessment requests,
     access requests, interview invitations and offers until the claim verifies.
   - Both Cloud and the app read the record from public DNS (the app through
     the system's resolver). The demo machine needs a network connection for
     that lookup, even though everything else is on loopback.
2. Open <http://127.0.0.1:8787>, press **Continue**. You are signed in as the
   demo organisation. **Settings → Organisation identity** should say
   **Verified**.
3. Open Alexandria (built from this repository), unlock the demo profile.
   **Settings → Developer → Alexandria Cloud address** → `http://127.0.0.1:8787`
   (it saves on Enter or when you leave the field). Then **Settings →
   Organisations**: *Receive requests from organisations* is on, and the
   section shows *Requests* rather than "No Alexandria Cloud is set up".
4. Optional: **Settings → Developer → Test data → Seed everything** for courses,
   discussions and proposals to show around the assessment. The two assessable
   banks are installed when the profile is created, in every build; seeds never
   create credentials, attempts, votes or posts.
5. Rehearse the whole path in one command from this repository:

   ```
   scripts/demo/rehearse.sh            # starts Cloud if it is not up
   scripts/demo/rehearse.sh --fresh    # and drops the demo database first
   ```

   It expects the Cloud checkout at `../alexandria-cloud` (set
   `ALEXANDRIA_CLOUD_CHECKOUT` to use another). It runs two ignored tests
   against the live Cloud with throwaway learner keys: one through the
   functions behind the buttons, one through the real Tauri commands the
   screens invoke (profile online, dev seed, assessment, grading, consent,
   publish, exchange, access request and grant with sync, sync off, stop
   sharing, hiring, a panel seat handing its room to the candidate, a
   scorecard, revoke, export). Each prints one line per step, including `4b.`
   for access; two `test result: ok` lines mean every step below works on this
   machine right now. Each run leaves a learner, a recorded assessment, a
   revoked access grant, an interview with a scorecard, an offer and (first
   test only) a pilot in the demo organisation — `--fresh` before the audience
   arrives.
6. Optional, from the Cloud repository: `node web/scripts/console-walkthrough.mjs`
   opens the console in your Chrome and walks sign-in, Talent, a run receipt,
   an interview and the pilot page; `--write` also fills the request,
   invitation and pilot forms. It prints one line per screen.
7. Have a terminal open in this repository for §6.

## 1. Learner earns a credential (app)

**Goals → Engineering Manager → View path → Big-O Analysis → Assess.**
Press ⌘⇧S → **Start camera preview** if you want the Sentinel view visible.
Answer, submit. On a pass one credential is issued on the device.

Open it from the result (or **Skills & Credentials**). Point at: credential id, issuer
DID, subject DID (the same key — this is *learner-issued*), skill, score,
Bloom level **Remember**, the signed integrity summary, evidence references.

Say: "The assessment ran here. The credential was signed here. No server was
involved."

## 2. Learner makes it public (app)

**My profile → Let employers find you**: choose Big-O Analysis, optional
display name, review the exact record, save consent, then **Publish now**.
Turning on **Keep my listing up to date** republishes it whenever the chosen
skills change, without asking each time; it is off by default.

Say: "Publishing is a summary — skill, level, how many independent issuers —
signed by the learner. The full credential is only ever sent to one named
organisation, on request."

## 3. Organisation finds the learner (Cloud)

**Talent**: skill Big-O Analysis, Bloom **Remember**, **0 minimum issuer
clusters** → the listing appears → **Add to candidates**. Open the person
record: the evidence passport shows the published skills.

Say: "Zero issuer clusters is honest: nobody independent has vouched yet."

## 4. Organisation requests another assessment; learner completes it (both)

Cloud → person record → **Invite to assess or share a credential**. Choose
**JavaScript** (the other skill, to avoid the cooldown), keep **Require a new
assessment** checked → **Create request**. The run page says the learner will
see it in Alexandria under Organisations, and has a copyable `alexandria://`
link.

Alexandria → **Settings → Organisations → Requests → Check now**. The request
card shows **Verified: owns alexandria.ifftu.dev** under the organisation's
name — the app looked up that TXT record itself, so the name is the one the
domain publishes, not one Cloud relayed. **Take requested assessment**. The
attempt is bound to this request's id and nonce. Pass, return to the same
card, **Choose a credential** → **Preview what will be sent** (show the exact
payload: a W3C Verifiable Presentation whose proof names the organisation's
audience, `urn:alexandria:org:alexandria.ifftu.dev`, as `domain` and the
request nonce as `challenge`, five-minute validity) → **Share with this
organisation**.

Cloud → the run → **Refresh result**: credential id, **accept**, learner-issued
caveat, integrity status.

Say: "An older credential cannot satisfy a request that asked for a new
assessment. The share is valid for five minutes and only to this audience."

## 4b. Organisation asks for access; learner shares, keeps it in sync, stops (both)

Cloud → person record → **Access → Request access**: tick **Big-O Analysis**
and **JavaScript**, write why you are asking (at least ten characters) →
**Request access**. The request is signed with the organisation's key and stays
open for 14 days. (For somebody found in Talent but not yet in People, use
**Hiring → Access → Ask by identity key**.)

Alexandria → **Settings → Organisations → Requests → Check now**. The card
reads *Wants to see your skills*, with the verified badge, the purpose, and a
checklist of the requested skills with their Bloom level and score. A skill
with no evidence yet is shown but cannot be ticked. Choose **Share and keep in
sync** (**Share** is a one-time copy; **Not now** only hides the card until the
app restarts, and the organisation is not told).

The grant is a statement signed by the learner, and what goes with it is a
signed record narrowed to the granted skills and the display name — never the
bio. *Who has access* now lists IFFTU: purpose, skill chips, *Shared …*, the
**Keep up to date** switch on, and *Updated …*.

Cloud → the person record → **Access**: the granted snapshot, *kept in sync*.
Opening it is recorded; in Alexandria, **Sync now** (or reopening the section)
shows *Viewed once · last …* on the grant.

Keeping in sync means the app sends a fresh record whenever what it would send
changes — it runs after grading, issuance and revocation, and in the
background pass — and at least weekly even when nothing changed, so the
organisation's copy never lapses. Switch **Keep up to date** off to freeze it
as a one-time share.

**Stop sharing** → confirm (*IFFTU will no longer see these skills. Alexandria
Cloud deletes what it holds for them.*). The revoke is signed too. Cloud →
**Refresh** the person record: the grant shows as revoked and the snapshot is
gone. In Alexandria it moves to *No longer shared*.

Say: "The organisation asked; the learner chose which skills, for how long,
and can see every time it looked. Stopping is one button, and Cloud deletes
what it held."

## 5. Interview held in Alexandria, and an offer (both)

The call runs inside Alexandria, peer to peer between the panel's and the
candidate's apps, using the live tutoring room. The panel uses the interview
assistant (consent gate, attributed captions and transcript, criteria
coverage, follow-up suggestions, private notes, review). A panelist needs a
second machine: one app has one unlocked profile at a time, and the learner's
profile is the candidate.

Cloud → run page (or person record) → **Invite to interview**: role, message,
**Where: In Alexandria** (the default), one to eight proposed times, *What the
panel assesses* (one criterion per line, prefilled with defaults or the role's
skills), and the panel — one to six rows of name and optional email.
Propose at least one time within the next quarter of an hour: the candidate's
**Join interview** opens 15 minutes before the chosen time. **Send
invitation** → each panelist's join link is shown once, with **Copy** (*Links
are shown only now.*). Copy the link to the panelist's machine. The interview
page shows *awaiting answer*.

Alexandria (learner) → **Settings → Organisations → Requests → Check now**.
The invitation shows the message, *Where: In Alexandria*, and that it follows
the assessment. **Choose a time → Accept** (or **Decline**). The answer is
signed with the learner's key over the entire invitation and sent only to that
organisation. It also appears under **Your answers**, and under *Upcoming
interviews* with **Join interview**.

Cloud → interview page → **Refresh**: *accepted* with the chosen time and the
signed answer. The *Panel* section lists the panelists, with *not yet* under *Joined*.

Alexandria (panelist's machine) → open the join link (from a browser, or
`open 'alexandria://interview/join?…'` in Terminal). The app fetches the
interview with the panelist's token and opens `/panel/:id`: organisation,
role and candidate, time, the panel, whether the room is open, and the skills
the candidate has shared with this organisation. Record each participant's
choices at the consent gate (**Record choices**), then **Start interview**. The first panelist in opens the
room and hands Cloud its ticket; any later panelist joins that room. Sentinel
is not started on a panel seat — integrity monitoring belongs on the
candidate's device, not a panelist's.

Alexandria (learner) → **Join interview**. The page waits for the panel, asking
Cloud every few seconds with the learner's own signed proof (only the
candidate can learn the ticket), then **Join interview** enters the live room.

Hold the conversation. On the panelist's machine, show captions attributed to
each speaker, criteria being marked, follow-up suggestions and a private note.
End the call → the review opens. **Scorecard for IFFTU**: a 1–4 rating per criterion
(Weak / Mixed / Good / Strong, or not rated), a recommendation (Strong yes /
Yes / No / Strong no), notes for the organisation → **Send to IFFTU**. Only the
ratings, each criterion's coverage, the recommendation and those notes are
sent; the transcript and private notes stay on the panelist's device.

Cloud → interview page → **Refresh**: *1 of 1 scorecards in*, a grid of
criterion × panelist with rating and coverage, the recommendation and notes.
**Add panelist** issues a further link, also shown once. Then **Record the
interview**: outcome **Advance to offer**, notes → **Save interview record**.

Cloud → same page → **Make an offer**: terms, optional start date → **Send
offer**. Alexandria (learner) → **Requests → Check now** → the offer with its
terms → **Accept offer**. Cloud → **Refresh** → *accepted*.

Say: "Cloud schedules and never carries the call or records it; the panel and
the candidate talk directly. Cloud holds the signed invitation, the learner's
answer, the room ticket, the scorecards each panelist chose to send, and the
offer's answer. The transcript stays with the panelist."

Limits worth knowing: invitations and offers expire unanswered after 14 days;
answers must be signed within five minutes of being made; an exact retry is
accepted again, a changed answer is refused (409); one open or accepted offer
per interview; an offer only follows a recorded interview.

## 6. Verify the credential without Alexandria (terminal)

Alexandria → **Skills & Credentials → Export all**. Then, from this repository:

```
node scripts/demo/verify-credential.mjs ~/Downloads/alexandria-credentials-<date>.json
```

The script is ~300 lines, imports only `node:fs`, `node:crypto` and
`node:zlib`, and implements the published algorithm: JCS-canonicalise the
credential and its proof options, verify the `eddsa-jcs-2022` Data Integrity
proof against the key decoded from the issuer's `did:key`, check the validity
window, and read the revocation bit. When the credential names its list by
URL, which every credential issued while the app points at the local Cloud
does, the bit
comes from a plain `GET` of that URL, after checking the document that came
back is that list signed by that issuer. The fetched list replaces the copy the
bundle carries unless it is older, so a revocation made after the export still
shows. If the host cannot be reached, or with `--offline`, the bundle's copy is
used. It prints signature, validity and status as three separate answers, then
ACCEPT / PENDING / REJECT, and says where each list came from: fetched from the
host, or the exported bundle as of its date.

Then revoke a credential in Alexandria (**Skills & Credentials → the skill card → the credential → Revoke**,
and confirm), run the same command on the same export again, and its status
flips to `revoked` without re-exporting anything: the app pushed the new list to
Cloud as part of the revocation. Open the list URL in a browser to show the
signed `BitstringStatusListCredential` being served.

A bare credential verifies on its own too, with Cloud running. Without Cloud,
or with `--offline`, a bare credential reports its status as pending:

```
jq '.credentials[] | select(.credentialSubject.skillId=="skill_big_o")' \
  ~/Downloads/alexandria-credentials-<date>.json > credential.json
node scripts/demo/verify-credential.mjs credential.json
```

To show the key resolution is not ours either: paste the issuer DID into
<https://dev.uniresolver.io/> — the Universal Resolver returns the same
Ed25519 public key the script decoded.

What to say: the credential is a W3C Verifiable Credential (Data Model 2.0)
with a standard Data Integrity proof, cryptosuite `eddsa-jcs-2022`, and a
Bitstring Status List entry. Any conforming Data Integrity verifier — for
example `@digitalbazaar/data-integrity` with
`@digitalbazaar/eddsa-jcs-2022-cryptosuite` — accepts it with no Alexandria
code; the script exists so the audience can read the whole check in one file.
The twelve signed test vectors in
`crates/alexandria-verify/tests/vectors/README.md` are the contract.

To prove the script agrees with the reference verifier on every vector:
`node --test scripts/demo/verify-credential.test.mjs`.

## 7. The validity pilot (Cloud)

Cloud → **Operations → Validity pilot → Create study** (defaults: 60
participants, 45 completed, practical pass score 70). Open it.

1. **Outside reviewer sign-off** — enter the reviewer's name. Until this is
   recorded the **Enrol** button stays disabled and the API refuses enrolment.
2. **Reviewers** — register at least: two *screens the conventional arm*, two
   *screens the capability arm*, two *scores the practical task (blind)*. One
   name, one role; the server refuses a second role for the same name.
3. **Enrol a participant** — tick *Consent recorded* (required), optionally the
   participant's `did:key`. The server assigns the arm with balanced
   randomisation (never more than one apart) and issues the code `P-001`,
   `P-002`, … The code is the only identifier that leaves the database.
4. **Screening decision** — pick a participant; the reviewer list shows only
   the reviewers registered for that participant's arm. Advance or reject with
   a rationale. One decision per reviewer per participant.
5. **Practical task score** — scorers see codes and arms, never a screening
   decision (`/api/pilots/{id}/practical-queue?scorer=…` is the blind queue).
   A screening reviewer cannot score; a scorer cannot screen.
6. **Payment** — hours per participant, recorded regardless of score.
7. **Employer feedback** — per employer, keep going or stop, with notes.
8. The arm cards update live: enrolled, completed assessments (participants
   whose DID has a recorded assessment run in this organisation), advance
   rate, screener agreement (percent and Cohen's κ, in words), practical mean,
   and **overlooked strong** — rejected at screening yet at or above the pass
   score, with the codes.
9. **Download de-identified export** — codes, arms, decisions, scores. No DID,
   name or email; each export is an audit event.

Say: "This is the study the Emergent Ventures application describes, as
software: random assignment, separate reviewers per arm, blind practical
scoring, agreement and completion measured, published without identifiers."

## 8. What to answer when asked

- **Who holds the issuing key?** The learner. Assessment credentials are
  self-issued by the learner's own `did:key`, so the subject and issuer are the
  same key. What attests beyond the learner's signature: the Sentinel integrity
  summary signed inside the credential; the request/nonce binding when an
  organisation asked for the assessment; and endorsements from trusted
  instructors, which raise the independent issuer-cluster count that Cloud
  shows. A learner-issued credential is labelled as such everywhere and is
  evidence of a proctored local assessment, not an institutional award.
- **Can it be verified offline?** Yes — §6, with the bundle the app exports.
- **Does Cloud see raw camera or keystrokes?** No. The credential carries a
  signed integrity summary only; raw captures never leave the device unless
  the learner releases evidence to contest a flag.
- **How does the app know the request is really from IFFTU?** Each request,
  invitation and offer carries a proof signed with the organisation's key. The
  app reads `_alexandria.<domain>` from DNS itself, checks the key is listed
  there and the signature holds, and shows the name the domain publishes. An
  unverified request is still shown, with a warning, but nothing can be sent
  back to it.
- **Does Cloud hear the interview?** No. The call runs directly between the
  apps over the tutoring room. Cloud passes the room ticket to the candidate
  and receives each panelist's scorecard; the transcript and private notes
  stay on the panelist's device.
- **What is Jev?** Not named in the UI. The optional decision checks are
  labelled "Optional AI checks" and are off for this demo.

## 9. When something does not arrive

- **Request / invitation not in the app:** press **Check now** in **Settings →
  Organisations**; the app pulls, nothing pushes. Confirm *Receive requests from
  organisations* is on and **Settings → Developer → Alexandria Cloud address**
  is `http://127.0.0.1:8787` exactly. Confirm the Cloud candidate's DID is this
  profile's DID.
- **Cloud refuses to send a request, invitation or offer (409, "Verify your
  organisation's domain before contacting candidates"):** the claim is not
  verified. Cloud → **Settings → Organisation identity** shows the last error
  and the record to publish; **Check now** after fixing DNS.
- **"Couldn't verify who sent this" on a card, actions disabled:** the app could
  not read `_alexandria.alexandria.ifftu.dev`, or the record does not list the
  key the request was signed with. Check the machine can resolve DNS. A record
  is cached for an hour, but a request signed with a key the cached copy does
  not list makes the app look again at once.
- **Join interview says "Opens 15 minutes before":** the chosen time is further
  away. Invite again with a time in the next quarter of an hour.
- **Candidate stays on "Waiting for the panel to open the room":** no panelist
  has pressed **Start interview** yet; the page checks every few seconds.
- **The join link does nothing:** it must open in the desktop app, which
  registers `alexandria://`. A panelist's link is shown only once; **Add
  panelist** on the interview page issues a new one.
- **Share refused:** the five-minute preview expired — make a new preview. A
  request requiring a new assessment refuses a credential issued before the
  invitation.
- **Interview answer refused:** the invitation expired or was already
  answered; the app only lists open ones after a refresh.
- **Pilot enrol disabled:** sign-off not recorded, or consent box unticked.
- **Nothing in Talent:** the listing was not published, or expired; **Publish
  now** again, or turn on **Keep my listing up to date**.
- **Cloud 401 on `/for/{did}` routes:** the proof is single-use and five
  minutes old at most; the app signs a fresh one per refresh. Check the clock.
- **Verifier says PENDING:** the status list could not be read. For a bare
  credential naming a `urn:` list, export the bundle instead. For a URL list,
  Cloud is not running, or the list was never pushed: Alexandria →
  **Skills & Credentials → the skill card → the credential → Revoke** reports the push result, and the
  background pass retries every minute.
- **Credential names a `urn:` list, not the Cloud URL:** it was issued before
  the **Alexandria Cloud address** was set (or before a **Status list host**
  was set). Issue a fresh one; ids already in credentials
  are never rewritten.

## 10. Tests behind each step

| Step | Where |
|---|---|
| Assessment, grading, issuance | `src-tauri/src/commands/assessment.rs` tests (29) |
| Exchange binding and share | `assessment.rs::native_assessment_is_discoverable_and_verifies_as_a_bound_share`; Cloud `tests/assessment_exchange.rs` |
| Interview / offer wire contract | app `src-tauri/src/commands/hiring.rs` tests; Cloud `src/hiring.rs` tests |
| Interview / offer end to end | Cloud `tests/hiring.rs` (tenant isolation, holder proof, signed answers, idempotency, conflict, audit, expiry sweep) |
| Organisation identity | `crates/alexandria-verify/src/org.rs` tests (TXT parsing, proof signing and verification); app `src-tauri/src/org_trust.rs` tests; Cloud `tests/org_identity.rs` |
| Access requests, grants, sync, revoke | `crates/alexandria-verify/src/access.rs` tests (signed statements, a grant answers its request, an update cannot widen it, the snapshot holds only granted skills); app `src-tauri/src/commands/organisations.rs` tests; Cloud `tests/access_grants.rs` |
| Interview held in Alexandria | app `src-tauri/src/commands/interview_panel.rs` tests (join link, panel seat session, scorecard body); Cloud `tests/interview_panel.rs` |
| Learner inbox and access UI | `src/components/organisations/OrgRequests.test.ts`, `OrgAccess.test.ts`, `OrganisationsSettings.test.ts`; `src/components/profile/TalentIndexConsent.test.ts` |
| Panel scorecard and candidate room UI | `src/pages/interviews/Review.test.ts`, `src/pages/interviews/CandidateRoom.test.ts` |
| Cloud console | `web/src/**/*.test.ts` (`npm test` in `web/`) |
| Pilot arithmetic | Cloud `src/pilot.rs` tests (balanced arms, κ, agreement, overlooked) |
| Pilot protocol end to end | Cloud `tests/pilot.rs` |
| Independent verifier | `node --test scripts/demo/verify-credential.test.mjs` against all twelve vectors, plus a served status list |
| Hosted status lists | app `credentials.rs` tests (host, URL ids, pending, push); Cloud `tests/status_lists.rs` (publish, serve, forgery, rollback, share refusal) |
| The whole path, live | `src-tauri/src/commands/live_demo.rs`, two tests: inner functions, and the real Tauri commands via a mock app — `scripts/demo/rehearse.sh`, see §0 step 5 |
| The desktop screens | Driven once by hand through macOS accessibility on 2026-10-09 (onboarding, directory, Big-O assess, revoke, requested JavaScript assess, share, interview, offer, consent, publish, search); not automated in CI |
| The Cloud console screens | Cloud `web/scripts/console-walkthrough.mjs --write` (Chrome via playwright-core): sign-in, Talent, receipt, interview, request and invitation forms, a pilot study through its forms |
| Revoke screen | `src/pages/dashboard/CredentialDetail.test.ts` (revoke → publish → where the list landed) |
