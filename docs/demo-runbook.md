# Demo runbook — learner to offer, and the validity pilot

Everything needed to run the full demo without getting lost: what is where, in
which order to click, what to say, and what to do when something does not
arrive. The product narrative is in [`assessment-demo.md`](assessment-demo.md);
this is the operator's sheet.

## Where everything is

| Thing | Location |
|---|---|
| Learner app source | worktree `worktrees/alexandria-assessment-demo`, branch `codex/assessment-demo` |
| Cloud source | worktree `worktrees/alexandria-cloud-assessment-demo`, branch `codex/assessment-demo` |
| Cloud console | <http://127.0.0.1:8787> (sign-in page → **Continue**; loopback dev sign-in only) |
| Cloud Postgres | Docker container `alexandria-assessment-demo-db`, loopback port 5544, database `alexandria_demo` |
| Cloud launcher | `scripts/demo-assessment.sh` in the Cloud worktree (needs Docker Desktop running) |
| App directory setting | Alexandria → **Settings → Directories** → add `http://127.0.0.1:8787` named **Local demo** |
| Learner inbox (requests, interviews, offers) | Alexandria → **Settings → Directories**, three panels: *Assessment and credential requests*, *Interview invitations and offers*, and your own answers under *Your answers* |
| Credentials | Alexandria → **Credentials** (`/credentials`); detail at `/credentials/:id`; **Export** downloads `alexandria-credentials-<date>.json` |
| Public listing consent | Alexandria → **My profile → Let employers find you** |
| Dev test data | Alexandria → **Settings → Developer → Test data** (debug or `dev-seeding` builds only) |
| Sentinel live view | ⌘⇧S inside the unlocked app |
| Cloud talent search | Cloud → **Talent** (`/talent`) |
| Cloud people / candidate record | Cloud → **People** (`/candidates`, `/candidates/:id`) |
| Cloud assessment runs | Cloud → **Assessments** (`/runs`, `/runs/:id`) |
| Cloud interviews and offers | Cloud → **Hiring → Interviews and offers** (`/interviews`, `/interviews/:id`) |
| Cloud validity pilot | Cloud → **Operations → Validity pilot** (`/pilot`, `/pilot/:id`) |
| Independent verifier | `scripts/demo/verify-credential.mjs` (Node standard library only; see §6) |
| Hosted status list | `http://127.0.0.1:8787/status-lists/<issuer did>/1` — the URL every credential issued with **Local demo** configured names in `credentialStatus`; `GET` it in a browser |
| Status list host setting | Alexandria → **Settings → Privacy → Status list host** (empty = the first directory stands in, so **Local demo** is used) |
| Third-party DID resolver | <https://dev.uniresolver.io/> — paste the issuer `did:key:…` |

Bundled assessable skills: **JavaScript** (`skill_javascript`) and **Big-O
Analysis** (`skill_big_o`). Three questions each, 70% to pass, so all three
must be right. Use a fresh profile or alternate the two skills; the normal
attempt cooldown applies.

## 0. Before the audience arrives (10 minutes)

1. Start Docker Desktop. In the Cloud worktree run `scripts/demo-assessment.sh`.
   Wait for "listening on 127.0.0.1:8787". The script creates the empty
   **Alexandria Demo** organisation, activates the exported taxonomy, builds the
   console if `web/dist` is missing, and starts Ollama for the role-requirements
   step. Nothing else is seeded: no candidates, credentials, interviews or pilots.
2. Open <http://127.0.0.1:8787>, press **Continue**. You are signed in as the demo organisation.
3. Open Alexandria (built from the app worktree), unlock the demo profile.
   **Settings → Directories** → confirm **Local demo** is listed. If not, add it.
4. **Settings → Developer → Test data**: tick **Assessments** (JavaScript
   fundamentals, Big-O & complexity) and run. Required in a debug or
   `tauri dev` build — a development build installs only the taxonomy when a
   profile is created, so without this step the assessment screen says "no
   assessment available". A release build ships the banks and skips this.
   **Seed everything** adds courses, discussions and proposals to show around
   the assessment; seeds never create credentials, attempts, votes or posts.
5. Rehearse the whole path in one command from the app worktree:

   ```
   scripts/demo/rehearse.sh            # starts Cloud if it is not up
   scripts/demo/rehearse.sh --fresh    # and drops the demo database first
   ```

   It runs two ignored tests against the live Cloud with throwaway learner
   keys: one through the functions behind the buttons, one through the real
   Tauri commands the screens invoke (profile online, dev seed, assessment,
   grading, consent, publish, exchange, hiring, revoke, export). Each prints
   one line per step; two `test result: ok` lines mean every step below works
   on this machine right now. Each run leaves a learner, a recorded
   assessment, an interview, an offer and (first test only) a pilot in the
   demo organisation — `--fresh` before the audience arrives.
5. Have a terminal open in the app worktree for §6.

## 1. Learner earns a credential (app)

**Goals → Engineering Manager → View path → Big-O Analysis → Assess.**
Press ⌘⇧S → **Start camera preview** if you want the Sentinel view visible.
Answer, submit. On a pass one credential is issued on the device.

Open it from the result (or **Credentials**). Point at: credential id, issuer
DID, subject DID (the same key — this is *learner-issued*), skill, score,
Bloom level **Remember**, the signed integrity summary, evidence references.

Say: "The assessment ran here. The credential was signed here. No server was
involved."

## 2. Learner makes it public (app)

**My profile → Let employers find you**: choose Big-O Analysis, optional
display name, review the exact record, save consent.
**Settings → Directories → Publish listing** to **Local demo**.

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
assessment** checked → **Create request**. The run page shows the directory
origin and a copyable `alexandria://` link.

Alexandria → **Settings → Directories → Assessment and credential requests →
Refresh** → **Take requested assessment**. The attempt is bound to this
request's id and nonce. Pass, return to the same panel, **Choose a credential**
→ **Preview disclosure** (show the exact payload: a W3C Verifiable
Presentation whose proof names the organisation as `domain` and the request
nonce as `challenge`, five-minute validity) → **Share with this organisation**.

Cloud → the run → **Refresh result**: credential id, **accept**, learner-issued
caveat, integrity status.

Say: "An older credential cannot satisfy a request that asked for a new
assessment. The share is valid for five minutes and only to this audience."

## 5. Interview and offer (both)

Cloud → run page (or person record) → **Invite to interview**: role, message,
how (video / call / in person), one to eight proposed times, your own meeting
link (`https://` or `tel:`). **Send invitation** → the interview page shows
*awaiting answer*.

Alexandria → **Settings → Directories → Interview invitations and offers →
Refresh**. The invitation shows the message, the meeting link, and that it
follows the assessment. **Choose a time → Accept** (or **Decline**). The answer
is signed with the learner's key over the entire invitation and sent only to
that directory. It also appears under **Your answers** with the meeting link.

Cloud → interview page → **Refresh**: *accepted* with the chosen time and the
signed answer. Hold the conversation on your meeting link. Then **Record the
interview**: outcome **Advance to offer**, notes → **Save interview record**.

Cloud → same page → **Make an offer**: terms, optional start date → **Send
offer**. Alexandria → same panel → **Refresh** → the offer with its terms →
**Accept offer**. Cloud → **Refresh** → *accepted*.

Say: "Cloud never hosts a call and never records one. It holds three signed
facts: the invitation, the learner's answer, the offer's answer. Everything
else is the organisation's own notes."

Limits worth knowing: invitations and offers expire unanswered after 14 days;
answers must be signed within five minutes of being made; an exact retry is
accepted again, a changed answer is refused (409); one open or accepted offer
per interview; an offer only follows a recorded interview.

## 6. Verify the credential without Alexandria (terminal)

Alexandria → **Credentials → Export**. Then, from the app worktree:

```
node scripts/demo/verify-credential.mjs ~/Downloads/alexandria-credentials-2026-10-09.json
```

The script is ~300 lines, imports only `node:fs`, `node:crypto` and
`node:zlib`, and implements the published algorithm: JCS-canonicalise the
credential and its proof options, verify the `eddsa-jcs-2022` Data Integrity
proof against the key decoded from the issuer's `did:key`, check the validity
window, and read the revocation bit. The bit comes from the status list the
bundle carries, or — when the credential names its list by URL, which every
credential issued with **Local demo** configured does — from a plain `GET` of
that URL, after checking the document that came back is that list signed by
that issuer. It prints signature, validity and status as three separate
answers, then ACCEPT / PENDING / REJECT, and says where the list came from.

A bare credential now verifies on its own, with Cloud running:

```
node scripts/demo/verify-credential.mjs credential.json
```

Then revoke it in Alexandria (**Credentials → the credential → Revoke**), run
the same command again, and the status flips to `revoked` without re-exporting
anything: the app pushed the new list to Cloud as part of the revocation. Open
the list URL in a browser to show the signed `BitstringStatusListCredential`
being served. `--offline` skips the fetch and reports the list as pending.

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
- **What is Jev?** Not named in the UI. The optional decision checks are
  labelled "Optional AI checks" and are off for this demo.

## 9. When something does not arrive

- **Request / invitation not in the app:** press **Refresh** in the panel; the
  app pulls, nothing pushes. Confirm **Local demo** is `http://127.0.0.1:8787`
  exactly. Confirm the Cloud candidate's DID is this profile's DID.
- **Share refused:** the five-minute preview expired — make a new preview. A
  request requiring a new assessment refuses a credential issued before the
  invitation.
- **Interview answer refused:** the invitation expired or was already
  answered; the app only lists open ones after a refresh.
- **Pilot enrol disabled:** sign-off not recorded, or consent box unticked.
- **Nothing in Talent:** the listing was not published, or expired; republish.
- **Cloud 401 on `/for/{did}` routes:** the proof is single-use and five
  minutes old at most; the app signs a fresh one per refresh. Check the clock.
- **Verifier says PENDING:** the status list could not be read. For a bare
  credential naming a `urn:` list, export the bundle instead. For a URL list,
  Cloud is not running, or the list was never pushed: Alexandria →
  **Credentials → the credential → Revoke** reports the push result, and the
  background pass retries every minute.
- **Credential names a `urn:` list, not the Cloud URL:** it was issued before
  **Local demo** was added under **Settings → Directories** (or before a
  **Status list host** was set). Issue a fresh one; ids already in credentials
  are never rewritten.

## 10. Tests behind each step

| Step | Where |
|---|---|
| Assessment, grading, issuance | `src-tauri/src/commands/assessment.rs` tests (29) |
| Exchange binding and share | `assessment.rs::native_assessment_is_discoverable_and_verifies_as_a_bound_share`; Cloud `tests/assessment_exchange.rs` |
| Interview / offer wire contract | app `src-tauri/src/commands/hiring.rs` tests; Cloud `src/hiring.rs` tests |
| Interview / offer end to end | Cloud `tests/hiring.rs` (tenant isolation, holder proof, signed answers, idempotency, conflict, audit, expiry sweep) |
| Learner inbox UI | `src/components/profile/HiringInbox.test.ts` |
| Cloud console | `web/src/**/*.test.ts` (`npm test` in `web/`) |
| Pilot arithmetic | Cloud `src/pilot.rs` tests (balanced arms, κ, agreement, overlooked) |
| Pilot protocol end to end | Cloud `tests/pilot.rs` |
| Independent verifier | `node --test scripts/demo/verify-credential.test.mjs` against all twelve vectors, plus a served status list |
| Hosted status lists | app `credentials.rs` tests (host, URL ids, pending, push); Cloud `tests/status_lists.rs` (publish, serve, forgery, rollback, share refusal) |
| The whole path, live | `src-tauri/src/commands/live_demo.rs`, two tests: inner functions, and the real Tauri commands via a mock app — `scripts/demo/rehearse.sh`, see §0 step 5 |
| Revoke screen | `src/pages/dashboard/CredentialDetail.test.ts` (revoke → publish → where the list landed) |
