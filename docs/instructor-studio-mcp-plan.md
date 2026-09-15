# Instructor Studio and MCP implementation

Updated: 2026-09-15

## Workspace and scope

- App worktree: `worktrees/instructor-studio-mcp`
- Branch: `codex/instructor-studio-mcp`, based on `e4e9248`.
- Existing `assessment-remediation` / `rebuild/foundation` is untouched.
- Design reference: `work/instructor-studio/instructor-studio-v4.html` in the parent workspace.
- MCP reference: `/Users/hack/Desktop/Alexandria-MCP-Execution-Plan.md`.
- User authorized maintaining this plan. This does not authorize changing other documentation, deploying, or publishing real courses.

## Accepted product decisions

- Course creation: Brief → Sources → Curriculum → Review, with editable stages, contextual AI, and explicit publication review. Retain the application's fonts, tokens, and components.
- Workspace navigation owns Workflows and AI settings. Course settings owns learner assistance.
- Local models and cloud keys; different models per role and optional overrides per workflow step.
- Instructor-editable initial instructions at role, workflow, course, step, and run levels. Show effective prompts before execution; freeze them in each run.
- Models generate proposals. Instructors review, edit, approve, and can undo; stale updates must conflict.
- Learner BYOM with optional instructor/institution sponsorship. Sponsorship requires a server that never distributes the sponsor's key.
- Local learning and verification cannot depend on Cloud.

## Dependency sequence

| Slice | Dependencies | Implementation and acceptance | Status |
|---|---|---|---|
| S1 — Persistent authoring foundation | Existing course editors | Typed shared Rust services, encrypted per-profile storage, brief/source/prompt persistence, ownership and revision checks | Implemented; targeted tests pass |
| S2 — Composer and workspace UI | S1 | Approved stage navigation, library, role prompts, connections, step editing, route guards, localized labels | Implemented; strict typecheck, production build, component tests and browser fixture checks pass |
| S3 — Asynchronous text workflows | S1, S2 | Snapshot prompts/models/context; persistent runs; explicit start, stop/resume, review/apply/undo; invalidate on profile lock | Implemented; shared-service and controlled HTTP tests pass; paid provider and native worker lifecycle smoke remain open |
| M0/M1 — MCP baseline and verifier | Existing verifier | Pin SDK, isolated standalone executable, real stdio tests, independent client, bounded input, no vault startup | M1 verifier exit checks pass, including pinned Inspector CLI; broader M0 authorization/schema ledger remains open |
| S4 — MCP broker and instructor draft tools | S1, M1 | OS-local broker, trusted-app grants, profile epoch, revocation, read text drafts, idempotent proposals into the same review queue | Adapter, grants, UI and review queue implemented; policy/stdio tests pass; real native two-profile lifecycle validation remains open |
| S5 — Media creation | S3, content storage | Capability-specific image/audio/video adapters, artifact provenance, accessible alternatives, provider failures, approval before attachment | Pending; text endpoints currently produce media briefs/scripts only |
| S6 — Learner tutor | S1, S3, learner player | Signed public course policy, learner local/cloud model setup, lesson threads, assessment isolation, policy tests | Implemented for learner BYOM; native provider smoke and sponsored access remain open |
| S7 — Sponsored access | S6, Cloud delegated auth | Server-held sponsor keys, per-course budgets/limits, learner entitlement, no key disclosure | Pending; unavailable until service exists |
| M2–M8 — Full MCP Release 1 | M0/M1 and grants | General learner reads, Cloud OAuth/tenant tools, isolated fixtures, full conformance/CI, Inspector and real assistant client | Pending beyond the narrow instructor integration |
| M9+ — Further MCP writes | M2–M8 and domain services | Other reversible writes, consented disclosures, organizational actions | Pending; see Desktop plan |

### Work that can proceed asynchronously

- Local verifier/conformance tests can run while the authoring UI is implemented.
- Shared draft services precede both UI approval and MCP proposals; neither adapter owns a second database or independent policy.
- Local broker validation and provider adapters can proceed independently after S1/S3.
- Cloud OAuth/tenant work is independent of local authoring. Alexandria Cloud has its own Git repository and therefore needs a separate linked Cloud worktree when that slice starts; its code cannot be committed into this app Git history.
- Sponsored learner access waits for Cloud authorization and budget enforcement. Do not emulate it by returning a sponsor key.
- No background coding job is implied by this file. Persistent workflow jobs are application features; this table records implementation dependencies and continuation state.

## Current implementation contracts

### Persistence and prompts

`crates/alexandria-studio` owns typed data, validation, optimistic revisions, source selection, model requests, and draft apply/undo. Migration 084 installs its tables in the existing profile SQLCipher database. Keys live in a separate local-only table; public connection DTOs contain only a `has_key` flag. No sync/export path was added for keys or private source notes.

Effective instruction order: application constraints → role → workflow → course → step → run. Source excerpts, lesson text, and previous outputs are provided as reference data. Prompts cannot grant MCP permissions, publish, or approve their own output. Each prepared run retains its effective prompts, connection identifiers/models/endpoints, workflow revision, source context, and target fingerprint.

The initial provider adapter uses explicit OpenAI-compatible text endpoints (`/v1/chat/completions`), including local Ollama/LM Studio. Local URLs must be loopback and bypass environment HTTP proxies; cloud URLs require HTTPS. Redirects are disabled. Requests have time/size limits and at most 4,096 output tokens per step. Cloud cost is provider-dependent; no fabricated currency estimates are shown. No automatic retry or provider fallback occurs.

### Run lifecycle

`prepared → running → review → applied → undone`, with `paused` and `failed` states that require explicit resume. Persistent snapshots survive navigation and app restart. Starting a worker and persisting its running state share one database lease. Both run listing and direct run polling recover abandoned running jobs as paused. Profile locking aborts workers and invalidates authorization. An interrupted request may have been processed/billed remotely even when no response was saved; resuming explicitly retries that step.

Application of a generated proposal is restricted to an owned text or supported assessment element. Assessment workflows receive a strict JSON output contract and validate the proposal again at apply time. MCP draft reads and proposals remain text-only so assessment answers never cross that external boundary. Apply checks the saved target fingerprint and updates the draft atomically. Undo checks that no newer edit has overwritten the applied content. External proposals must enter the same review queue; no MCP publish/apply endpoint is permitted.

### Learner tutor and feedback loop

Instructor course settings now hold a learner tutor policy: enabled state, Socratic/balanced/direct guidance, and an editable initial prompt. Enabled policies are signed and published with the course; disabled policies are omitted to preserve verification of older signed documents. Private model connections and keys are never published. Enrolled learners choose their own enabled text connection, with local and cloud data-flow disclosures in the player. Tutor context is restricted to the current published text lesson and a 20-message lesson thread. The tutor is not mounted for assessments, and backend checks independently reject non-text elements.

Learners can rate and comment on individual non-assessment lessons. Instructors see the feedback in the Review stage and can open an improvement workflow for that lesson. Feedback is visible in the frozen run context and treated as untrusted reference data. This first implementation stores feedback in the active profile database; cross-device feedback transport still needs a signed P2P or Cloud contract before network-wide feedback can appear.

### MCP compatibility

- Latest stable specification checked: **2026-07-28**, official `/specification/latest`, 2026-09-15.
- Official Rust SDK: **rmcp 3.3.0**, pinned. Its declared MSRV is 1.88; use the app's already-installed **Rust 1.91.0** toolchain.
- Node installed: **22.21.1**. Inspector **2.6.0**, pinned launcher; declared Node requirement ≥22.19.0.
- Standalone `alexandria-mcp` bypasses application CLI initialization and profile discovery.
- `verify_credential` uses `alexandria-verify` with caller-supplied JSON. It explicitly reports unknown revocation/accreditation; the library acceptance decision is not an unconditional validity claim.
- Tested the real executable through an rmcp client, independent Python raw JSON-RPC client, and Inspector 2.6.0 CLI. Inspector strict portability checks pass for all four registered tool schemas; its verifier calls pass valid/tampered fixtures. Latest discovery, tools schemas, valid/tampered results, missing request metadata, unavailable writes, malformed credential input, and oversized framing are covered.
- **Not a full conformance claim:** authoritative-schema pinning, full protocol matrix/cancellation/error details, Inspector graphical checks, supported assistant-client smoke testing, and Cloud OAuth conformance remain outstanding.

## Validation record

Baseline:

- `cargo +1.91.0 test -p alexandria-verify --lib`: 61 passed.
- `npx vue-tsc -b --noEmit`: passed before changes.

Latest changed-code checks (2026-09-15):

- `cargo +1.91.0 test -p alexandria-studio -p alexandria-mcp`: 1 grant unit test, 9 Studio integration tests, and 3 MCP stdio integration tests passed. Coverage includes assessment contracts, tutor policy/access/thread conflicts, learner feedback in workflow context, broker revocation, and valid/tampered verification.
- `cargo +1.91.0 test -p alexandria-node commands::studio::tests --lib`: the tutor constraint test passed; persistent interrupted-run recovery and grant invalidation/reactivation passed earlier in this worktree.
- `cargo +1.91.0 test -p alexandria-node content_store::course::tests --lib`: 11 passed. Tests cover public text resolution after removing the author's content key and prove an enabled tutor policy is signed and tamper-evident. Assessment elements cannot be materialized through the text helper.
- `cargo +1.91.0 test -p alexandria-node missing_lower_migrations_run_after_a_higher_version --lib`: passed; migration 084 is applied even when a higher branch version is already recorded.
- `cargo +1.91.0 clippy -p alexandria-node -p alexandria-studio -p alexandria-mcp --all-targets -- -D warnings`: passed. Existing warnings from the patched `tao` dependency remain; affected application crates pass.
- `cargo +1.91.0 fmt --check`: passed.
- `npm test`: all 77 tests passed, including 5 Studio/tutor tests for review/approval/conflict behavior, disabled tutor behavior, cloud disclosure, and explicit learner submission.
- `npx vue-tsc -b --noEmit` and `npm run build`: passed. Existing bundle-size warnings remain.
- Tauri command guard: 374 registered / 291 invoked / 83 allowlisted; passed.
- Locale parity and no-raw-text checks: passed. New messages are English in the other locale catalogs until translated; translation work is still needed.
- `python3 scripts/mcp/smoke.py`: passed.
- Inspector 2.6.0 CLI: strict tool schema portability passed with and without a configured broker; valid and tampered verifier calls passed. Supply `credential_json` as a JSON string (the CLI parses each `--tool-arg` value).
- Browser QA used the real Composer, Workflows, AI settings, run panel, app CSS and fonts with a temporary mock IPC fixture. Checked prompt editing/saving, unsaved workflow discard, prepare → review → apply → undo, and 390px layout (390px content width; no horizontal overflow). Confirmed Inter is used. This is UI integration evidence, not a native or paid-model test. Temporary fixture files were removed from the worktree.

Provider tests use a controlled loopback HTTP server. No paid model or actual assistant account has been tested. Fixtures use in-memory databases and supplied verifier vectors; real profiles have not been opened or modified by the tests.

## Integration and next work

1. **Migration collision resolved locally:** the main `alexandria/` checkout has uncommitted interview support using migration 083 (`interview_assistant`). Instructor Studio now uses 084. The migration runner checks each recorded version rather than only `MAX(version)`, so a profile that applied 084 before the interview branch is integrated will still apply a missing 083 later. The other session's checkout remains untouched. Re-run the populated migration test after integrating its work.
2. Native broker lifecycle validation: two temporary profiles and two clients, in-flight read/write barriers, lock/switch/revoke/expiry, restart, malformed frames, unsupported platform behavior. Grants currently last one hour, clear on profile invalidation, and use private 0700 directories / 0600 files. The executable checks file permissions and refuses symlink opens. Revoked/expired connection files can remain on disk but are unusable; cleanup is a remaining lifecycle task.
3. Complete the M0 authoritative schema ledger and Cloud OAuth provider selection from the Desktop plan. M1's offline verifier is independently usable now. Do not claim full M2–M8 conformance or Cloud completion from the narrow draft integration.
4. Continue S5 capability-specific media adapters, then S7 sponsored service. The app currently supports text workflow calls only. Image/audio/video roles produce text briefs/scripts/storyboards until binary provider and artifact-review contracts are implemented.
5. Assessment **authoring** and learner-feedback improvement now use explicit domain policies and the instructor approval queue. Current MCP context deliberately excludes assessments and answers; no learner grading or credential decisions are delegated to a model. Define a signed transport before claiming learner feedback reaches authors across devices.
6. Actual provider interoperability, native packaged-app smoke, full supported assistant-client smoke and translations remain acceptance work. The standalone MCP executable is built with `cargo +1.91.0 build -p alexandria-mcp`; connection UI shows a client configuration, but an app-bundled sidecar/installer is not yet supplied.

### Publishing change

Approved inline text is materialized as public content-addressed text blobs before signing the release. Signed published course documents now use the public blob path so another learner does not need the author's profile content key. Enabled tutor guidance and its instructor-authored public prompt are included in the signed payload and hydrated with the course. Private authoring prompts, source notes, model keys, tutor threads and run history are not included. Publication checks author ownership, profile epoch and whether the captured inline text changed before updating the published course. This does not repair pre-existing publication behavior for other inline element types or guarantee a complete release snapshot under arbitrary concurrent outline edits.

### MCP contract and client configuration

Without `ALEXANDRIA_MCP_CONNECTION_FILE`, the binary exposes only offline `verify_credential` and does not discover profiles. With the connection file produced by AI settings → Assistant access, it additionally advertises `list_course_drafts`, `read_lesson_draft`, and `propose_lesson_draft`. Every private call is reauthorized by the running app against scope, expiry and profile epoch. The executable itself never opens a profile database or vault. A read lease spans app authorization, database work and response writing; lock/revoke obtains the exclusive lease before invalidation. No new response/write can proceed under a stale grant after that invalidation point.

A proposal only creates a review item under Workflows → Runs. Retry identity includes the grant, client name, request ID and target lesson; exact retries return the prior outcome even after instructor application, while changed payloads and stale target fingerprints conflict. There are no MCP apply, publish, grade or credential-issuance tools.

The Desktop reference plan was rechecked on 2026-09-15; its mtime was 13:56 local. The other checkout's interview changes and Alexandria Cloud's existing `.DS_Store` were left untouched. No background implementation job or completion of later slices is implied by this plan.
