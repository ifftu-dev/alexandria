# Assessment → credential → Cloud demo

This demo uses the `codex/assessment-demo` worktrees of Alexandria and Alexandria Cloud. It supports both a learner publishing an earned skill and an organization requesting an assessment or an existing credential. It does not require Jev or an AI provider key.

## Prepare this Mac

1. Open the installed **Alexandria** application and unlock the demo profile. Profile activation runs migrations and the current bundled seeds automatically: the public taxonomy, synonyms, goal templates, question banks/items, bootstrap trust data, and bundled plugins. There is no separate `db seed` CLI command in this branch. Unlock must succeed before assessment content is ready. Existing learner attempts and credentials are preserved.
2. Start Docker Desktop. From the Cloud checkout run `scripts/demo-assessment.sh`. Open <http://127.0.0.1:8787> and choose **Continue**. This is a loopback-only development sign-in, not a production identity-provider deployment.
3. In Alexandria, open **Settings → Directories** and add `http://127.0.0.1:8787`, named **Local demo**. Both applications use the exported bundled taxonomy. The launcher creates only an organization; it does not seed candidate assessment results.
4. Use **JavaScript** (`skill_javascript`) or **Big-O Analysis** (`skill_big_o`). These have bundled MCQ banks. Each attempt draws three questions and requires at least 70%; with three equally weighted questions, all three must be correct. Questions/options are randomized.
5. Use a fresh demo profile or a skill with no recent attempt when rehearsing. Normal attempt limits and cooldowns apply. An interrupted attempt counts; do not repeatedly reload to change the draw.

The launcher keeps Postgres in the `alexandria-assessment-demo-db` Docker container on loopback port 5544. The database `alexandria_demo` is separate from `alexandria_demo_test`. Local session configuration and the taxonomy export live under the ignored `.demo/` directory. Stopping the server/container preserves the demo data. No other development database is reset.

## Show Sentinel Dev

During an assessment, click **Sentinel Dev · ⌘⇧S**, or press **Command–Shift–S** while the unlocked app has keyboard focus. This passive panel is available in release builds, requires no developer setting, and displays the real active session's telemetry. Opening it neither ends the assessment nor starts a separate camera. Close it with its × button or the shortcut.

To demonstrate camera landmarks and gaze preview separately, enter **Diagnostics** from the profile menu, then use **Sentinel live view** in the diagnostics banner and start its camera preview. Exit diagnostics before starting a credential-bearing assessment. Diagnostics entry ends an open assessment; it is not the way to display the passive panel during the assessment. Camera preview requires the Mac's normal camera permission. Standalone MCQ assessments do not automatically request a camera.

Low-data signals can show unavailable rather than a score. Three MCQ clicks do not supply sufficient typing data to train or demonstrate every behavioral model. Sentinel reports local integrity observations; it does not establish independent identity or an independently proctored result.

## Demo A — learner first

1. In Alexandria, open **Skills**, choose JavaScript (or Big-O), and start its assessment.
2. Open the passive Sentinel Dev panel. Answer the questions and submit. Answers freeze before the last telemetry snapshot and session finalization; grading then issues one credential on a pass.
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
