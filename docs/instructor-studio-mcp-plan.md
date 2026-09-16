# Instructor Studio and MCP implementation

Updated: 2026-09-17

## Workspace and scope

- App worktree: `worktrees/instructor-studio-mcp`
- Branch: `codex/instructor-studio-mcp`, based on `e4e9248`.
- Existing `assessment-remediation` / `rebuild/foundation` is untouched.
- Design reference: `work/instructor-studio/instructor-studio-v4.html` in the parent workspace.
- MCP reference: `/Users/hack/Desktop/Alexandria-MCP-Execution-Plan - Astra.md` (renamed from `Alexandria-MCP-Execution-Plan.md`).
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
| M0/M1 — MCP baseline and verifier | Existing verifier | Pin SDK, isolated standalone executable, real stdio tests, independent client, bounded input, no vault startup | M1 exit checks pass; stdio conformance ledger, pinned 2026-07-28 schema tests and Inspector 2.6.0 CLI (`--strict`) pass; Cloud OAuth provider selected (Keycloak 26.7.3, spike verified); Cloud M4 foundation implemented on Cloud branch `codex/cloud-mcp-authz` (local commit), fixture end-to-end passes; remote connectors await staging |
| S4 — MCP broker and instructor draft tools | S1, M1 | OS-local broker, trusted-app grants, profile epoch, revocation, read text drafts, idempotent proposals into the same review queue | Adapter, grants, UI and review queue implemented; shared broker core passes native two-profile lifecycle tests with real MCP client processes; packaged-app smoke and Windows transport remain open |
| S5 — Media creation | S3, content storage | Capability-specific image/audio/video adapters, artifact provenance, accessible alternatives, provider failures, approval before attachment | Pending; text endpoints currently produce media briefs/scripts only |
| S6 — Learner tutor | S1, S3, learner player | Signed public course policy, learner local/cloud model setup, lesson threads, assessment isolation, policy tests | Implemented for learner BYOM; native provider smoke and sponsored access remain open |
| S7 — Sponsored access | S6, Cloud delegated auth | Server-held sponsor keys, per-course budgets/limits, learner entitlement, no key disclosure | Pending; unavailable until service exists |
| M2–M8 — Full MCP Release 1 | M0/M1 and grants | General learner reads, Cloud OAuth/tenant tools, isolated fixtures, full conformance/CI, Inspector and real assistant client | M3 complete (catalog, course and lesson reads; skill graph, progress, goals and learning path; credential summaries and presentation verification); M5 complete on the Cloud branch; M6 complete (fixtures for both servers, remaining gaps closed under M7); M7 complete (acceptance matrix on both sides, both in CI); M8 in progress (Claude Code and Codex CLI verified against both fixtures, including each client's own OAuth sign-in to Cloud; assistants pre-registered by decision while Keycloak cannot read CIMD documents; ChatGPT outstanding) |
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
- Authoritative schema pinned: `schema/2026-07-28/schema.json` at commit `271ecc9accafdd9b83a3c869fa67c22953b2af80` (SHA-256 `ef70b61f99b6d2e5e3b46863822eab08dff6a45bedc7a08914e0e5b133f40203`), vendored unmodified in `crates/alexandria-mcp/tests/schema/` with its NOTICE and license (Apache-2.0, older contributions MIT). `/specification/latest` rechecked as 2026-07-28 on 2026-09-15.
- Tested the real executable through an rmcp client, an independent raw Rust JSON-RPC driver that validates every stdout message against the pinned schema, the Python raw client, and Inspector 2.6.0 CLI (valid/tampered calls; `--strict` with and without a configured broker). See the conformance ledger below.
- Dual-era server: 2026-07-28 per-request metadata, plus legacy `initialize` for 2024-11-05, 2025-03-26, 2025-06-18 and 2025-11-25 as advertised by rmcp 3.3.0.
- Cloud `/mcp` (Streamable HTTP) is validated against the same pinned schema, vendored into the Cloud repository (`tests/mcp_conformance.rs`), and Inspector 2.6.0 CLI `--strict` passes against both servers, with every tool called through it. Claude Code and Codex CLI are verified against both, including their own OAuth sign-in to Cloud.
- **Not a full conformance claim:** legacy revisions are checked by assertions rather than their own pinned schemas; Inspector's graphical interface is not exercised; remote connectors (ChatGPT, Claude on the web) and a hosted confidential client await a staging deployment; Client ID Metadata Document sign-in is blocked by Keycloak (see the 2026-09-16 decision).

## Validation record

Baseline:

- `cargo +1.91.0 test -p alexandria-verify --lib`: 61 passed.
- `npx vue-tsc -b --noEmit`: passed before changes.

Latest changed-code checks (2026-09-15):

- `cargo +1.91.0 test -p alexandria-studio -p alexandria-mcp`: 1 grant unit test, 9 Studio integration tests, 3 MCP stdio integration tests, and 4 broker lifecycle tests passed. Coverage includes assessment contracts, tutor policy/access/thread conflicts, learner feedback in workflow context, broker revocation, and valid/tampered verification.
- `crates/alexandria-mcp/tests/broker_lifecycle.rs` drives the shared `alexandria_studio::broker` core (also used by the app) with two fixture profiles and real `alexandria-mcp` client processes: profile and scope isolation, cross-profile tokens, assessment exclusion, idempotent and conflicting proposals, revoke/expiry/lock/reactivation, in-flight read and proposal barriers, requests queued behind invalidation, restart, malformed/incomplete/oversized frames, and the concurrency limit. Passed 5 consecutive runs (~1 s each). Mutation check: removing the request lease makes the in-flight barrier test fail.
- Merged `main` (interview assistant, migration 083) on 2026-09-15. After the merge: `cargo +1.91.0 test -p alexandria-node --lib -- db:: commands::studio` passed (34), including `interview_migration_applies_after_instructor_studio`.
- M0 conformance (2026-09-15): `cargo +1.91.0 test -p alexandria-mcp` passed 16 tests (9 conformance, 4 broker lifecycle, 3 stdio); the conformance suite passed 3 consecutive runs; `cargo +1.91.0 clippy -p alexandria-mcp --all-targets -- -D warnings` and `python3 scripts/mcp/smoke.py` passed. A deliberately non-conforming discovery result fails pinned-schema validation. Inspector 2.6.0 CLI: valid/tampered verifier calls pass; `tools/list --strict` reports 0 errors and 0 warnings with and without a configured broker.
- `cargo +1.91.0 test -p alexandria-node commands::studio::tests --lib`: the tutor constraint test passed; persistent interrupted-run recovery and grant invalidation/reactivation passed earlier in this worktree.
- `cargo +1.91.0 test -p alexandria-node content_store::course::tests --lib`: 11 passed. Tests cover public text resolution after removing the author's content key and prove an enabled tutor policy is signed and tamper-evident. Assessment elements cannot be materialized through the text helper.
- `cargo +1.91.0 test -p alexandria-node missing_lower_migrations_run_after_a_higher_version --lib`: passed; migration 084 is applied even when a higher branch version is already recorded.
- `cargo +1.91.0 clippy -p alexandria-node -p alexandria-studio -p alexandria-mcp --all-targets -- -D warnings`: passed. Existing warnings from the patched `tao` dependency remain; affected application crates pass.
- `cargo +1.91.0 fmt --check`: passed.
- `npm test`: all 77 tests passed, including 5 Studio/tutor tests for review/approval/conflict behavior, disabled tutor behavior, cloud disclosure, and explicit learner submission.
- `npx vue-tsc -b --noEmit` and `npm run build`: passed. Existing bundle-size warnings remain.
- Tauri command guard (after merging `main`): 389 registered / 306 invoked / 83 allowlisted; passed.
- Locale parity and no-raw-text checks: passed. New messages are English in the other locale catalogs until translated; translation work is still needed.
- `python3 scripts/mcp/smoke.py`: passed.
- Inspector 2.6.0 CLI: strict tool schema portability passed with and without a configured broker; valid and tampered verifier calls passed. Supply `credential_json` as a JSON string (the CLI parses each `--tool-arg` value).
- Browser QA used the real Composer, Workflows, AI settings, run panel, app CSS and fonts with a temporary mock IPC fixture. Checked prompt editing/saving, unsaved workflow discard, prepare → review → apply → undo, and 390px layout (390px content width; no horizontal overflow). Confirmed Inter is used. This is UI integration evidence, not a native or paid-model test. Temporary fixture files were removed from the worktree.

M3 slice 1 (2026-09-16):

- `cargo +1.91.0 test --no-fail-fast -p alexandria-studio -p alexandria-mcp`: all passed — studio unit 6 (answer stripping for quiz, both multiple-choice shapes and essays; unrecognised formats; character sections; LIKE escaping), policy 9, broker lifecycle 5, conformance 9 (seven tools with a broker, schemas valid, no type arrays), stdio 3.
- `learners_read_published_content_without_answers_or_drafts` drives real `alexandria-mcp` processes: catalog order, stored/announced courses, skill filter, literal wildcards; the profile's own course and another author's draft answer `not_found`; inline and fetched text, an unfetchable lesson reported unavailable, quiz and multiple-choice options without answers, a withheld assessment, video chapters; learning and draft grants refuse each other's tools; no output contains answers, explanations, assessment or draft text; a profile lock completes within 2 s while a fetch is held, and the held request then fails without returning the fetched text.
- `cargo +1.91.0 clippy -p alexandria-node -p alexandria-studio -p alexandria-mcp --all-targets -- -D warnings` and `cargo +1.91.0 fmt --check`: passed (existing warnings from the patched `tao` only).
- `npx vue-tsc -b --noEmit`, `npm run build`, `npm test` (77), `i18n:parity` (8 locales, 2724 keys), `i18n:no-raw-text` and the Tauri command guard (389/306/83): passed. New strings are English in other locales until translated.
- Not yet done: browser check of the Settings section, and a packaged-app run of the resolver-backed fetch against real peers. (Inspector against these tools: done 2026-09-17.)

M3 slice 2 (2026-09-16):

- `cargo +1.91.0 test --no-fail-fast -p alexandria-studio -p alexandria-mcp`: all passed — studio 17 (graph visibility, prerequisite ordering and published-course recommendations, template versus text goal resolution, refused oversized input, progress with observed lessons), broker lifecycle 6, conformance 9 (eleven tools with a broker), stdio 3, policy 9.
- `learners_read_their_own_graph_progress_and_goals` drives real `alexandria-mcp` processes: the owner's graph keeps a private skill flagged `public: false` with its prerequisite edge; progress reports both lessons with one completed and its score; an exam template resolves to its target skill while text returns suggestions and saves nothing; the path orders prerequisites first with the published course recommended for the unearned goal; an unknown goal kind is refused; and a drafts-only grant is refused all four tools.
- Regression check that the extraction changed nothing: `cargo +1.91.0 test -p alexandria-node --lib` passed 1183 with 13 ignored, including the existing `commands::graph`, `p2p::graph_fetch` and `commands::goal_templates` suites that now exercise the shared implementation. One P2P test, `content_store::resolver::tests::resolve_blake3_fetches_from_peer_before_url`, failed once during that 280-second run and passed 10/10 when its module was run alone; it starts two real network nodes and touches no changed code. It was not shown to be reliable under full-suite load.
- `cargo +1.91.0 clippy -p alexandria-node -p alexandria-studio -p alexandria-mcp --all-targets -- -D warnings` and `cargo +1.91.0 fmt --check`: passed (existing warnings from the patched `tao` only).
- No frontend change in this slice, so the Vue and locale checks were not rerun.

M3 slice 3 (2026-09-16):

- `cargo +1.91.0 test --no-fail-fast -p alexandria-studio -p alexandria-mcp`: all passed — studio 20 (summaries carry provenance but no signed document, another subject's credential is not found, a presentation is accepted once and then replayed, tampered and malformed ones are refused and record nothing), broker lifecycle 7, conformance 9 (fourteen tools with a broker), stdio 3, policy 9.
- `credential_summaries_never_carry_the_signed_document` drives real `alexandria-mcp` processes: revoked credentials and other subjects are left out until asked for, a foreign credential reports `not_found`, a presentation for another audience is refused, an unverifiable one is refused without recording a nonce, a learning grant is refused all three tools, and no output contains the signed document.
- Regression check for the delegation: `cargo +1.91.0 test -p alexandria-node --lib` passed 1184 with 13 ignored, including the existing presentation suite now running against the shared implementation. The P2P resolver test that failed once during slice 2 passed here.
- `cargo +1.91.0 clippy -p alexandria-node -p alexandria-studio -p alexandria-mcp --all-targets -- -D warnings` and `cargo +1.91.0 fmt --check`: passed (existing warnings from the patched `tao` only).
- `npx vue-tsc -b --noEmit`, `npm test` (77), `i18n:parity` (8 locales, 2726 keys), `i18n:no-raw-text` and the Tauri command guard (389/306/83): passed. The fourth permission's strings are English in the other locales until translated.
- Not yet done: a browser check of the four-permission panel. (Inspector against the credential tools: done 2026-09-17.)

M5 slice 1 (2026-09-16, Cloud worktree):

- `cargo +1.91.0 test --no-fail-fast` against local Postgres: 33 suites, 337 passed, 0 failed. The MCP gate suite now covers the five-tool listing, the widened protected-resource metadata, and a token holding only `candidates:read` being refused `compare_candidate_to_role` with a challenge naming `roles:read`.
- `a_name_reaches_an_assistant_only_from_someone_who_answered_this_organisation` tests the mapping directly: a stranger's match carries no name, a match from somebody who answered carries theirs.
- `cargo +1.91.0 clippy --all-targets -- -D warnings` and `cargo +1.91.0 fmt --check`: passed.
- Cloud `docs/deployment.md` gained a tools-and-scopes section stating the three rules enforced in code; `README.md` updated to match.
- `python3 scripts/mcp/e2e.py` against fixture Keycloak 26.7.3: passed, now exercising all five tools with a live token. It seeds a role, two listed people who both clear its bar and a candidate who is one of them, then checks that a name is shown for somebody who answered this organisation and withheld for somebody who has not, that the search is written to the organisation's log, that the candidate summary carries no contact address, that the comparison reads requirement by requirement (`skill_rust` met, `skill_sql` reachable) and reports `listed_in_index: false` for a candidate with no listing, and that a token holding only `candidates:read` is refused the comparison with a challenge naming `roles:read`. Seeded identities carry the run marker and are removed afterwards, because the talent index is not organisation-scoped.
- Not yet done: the rest of M8 — ChatGPT, which can only reach a public HTTPS endpoint and so waits for an approved staging deployment. It will need a hosted pre-registration (`provision mcp client add --redirect`), whose confidential-client path has not been exercised by a real connector.

Step 1 of the 2026-09-17 reassessment (both worktrees):

- Cloud `tests/mcp_conformance.rs` against the pinned 2026-07-28 schema (vendored unmodified, SHA-256 `ef70b61f…0203`, NOTICE and licence included): discovery, `tools/list` and all nine tool results conform; each result satisfies its tool's declared `outputSchema` and mirrors itself as text; no tool schema uses a `type` array. It found two defects, both fixed: `resources/list`, `resources/templates/list` and `prompts/list` returned empty lists although only tools are advertised (now method not found, as on the local server), and a non-JSON body got the SDK's 415 plain-text answer (now 400 with JSON-RPC -32700).
- Inspector 2.6.0 CLI: `tools/list --strict` reports no portability problems on either server; all fourteen app tools and all nine Cloud tools (over HTTP with a fixture token) answer through it, with nothing withheld appearing. The app fixture gained an owner-authored draft course so `list_course_drafts`, `read_lesson_draft` and `propose_lesson_draft` can be exercised; `fixture_check.py` now asserts the draft is listed only to its author, absent from the catalog, readable with a fingerprint, that a proposal waits for review, an exact retry repeats its outcome, a stale fingerprint conflicts and nothing changes until applied.
- Cloud `scripts/test.sh` runs the suites against a freshly recreated `alexandria_cloud_test`: 36 binaries passed, the development database's organisation count unchanged. The README's testing section points at it.
- Cloud lockfile: rustls 0.23.43 → 0.23.45 for RUSTSEC-2026-0285; `cargo audit` clean. The app branch's lockfile carried 0.23.43 only until it merged the local `main`, which already has 0.23.45; `cargo audit` on `main` passes.
- Cloud clippy and fmt clean; app `clippy -p alexandria-studio --examples` and fmt clean.

- **Decided — assistant clients (2026-09-16):** pre-register now, return to Client ID Metadata Documents later. Evidence: the documents Codex (`https://chatgpt.com/oauth/codex/client.json`) and ChatGPT (`https://chatgpt.com/oauth/CrXSqmg5eT2O/client.json`) publish both carry `token_endpoint_auth_methods_supported`, and Keycloak refuses them with "Client Metadata fetch failed" on 26.7.3 and on 26.7.4 (checked against a temporary image bump, reverted). Upstream: issue keycloak#51236 open, fix PR #51235 open, earlier PR #51040 closed unmerged. Alternatives considered and not taken: a patched Keycloak image (fork upkeep on an identity server), a sanitising HTTPS fetch proxy (Keycloak trusting our CA in the sign-in path), waiting (ChatGPT blocked indefinitely). Claude.ai's document has not been checked.

Assistant pre-registration (2026-09-16, Cloud worktree):

- `MCP_ALLOWED_CLIENT_IDS` is accepted in release builds; URL-shaped entries are refused (`a_pre_registered_client_id_cannot_pose_as_a_metadata_document`).
- `provision mcp client add | list | remove`, through the Keycloak administrator. Loopback assistants are public, PKCE, callbacks limited to `127.0.0.1`/`localhost` on any port and any `/callback/*` segment (`a_loopback_assistant_holds_no_secret_and_returns_only_to_the_machine`); hosted assistants need exact https callbacks and get a secret printed once (`a_hosted_assistant_returns_only_to_exact_https_callbacks`). Removal refuses clients Cloud did not register.
- `python3 scripts/mcp/e2e.py`: passed. It registers its own client through the command, adds, lists and removes a hosted one, and checks five refusals each for their stated reason (URL-shaped ID, plain-http callback, wildcard callback, duplicate ID, removing the realm's `account` client). The canary asked Keycloak to read Codex's real document and confirmed it is still refused for the unknown field; it fails once that changes.
- `cargo +1.91.0 test`: 35 binaries, 0 failed. Clippy and fmt clean. `docs/deployment.md` gained an "Assistant clients" section; `README.md` updated.

M8 part (2026-09-16, both worktrees). Clients chosen by the user: Claude Code, Codex CLI, ChatGPT.

- Claude Code 2.1.273, headless (`claude -p` with `--mcp-config` and `--strict-mcp-config`, config in a temporary directory, no user or project configuration touched), against the app fixture over stdio: server connected, all fourteen tools discovered, seven calls made and every result checked in the event stream — two published courses, the assessment withheld with its reason, the quiz question without answer or explanation, the skill graph with alpha only, the path alpha earned / beta available / gamma locked, one credential summary without its signed document. Asked which quiz option was correct, it said no tool had told it.
- Codex CLI 0.154.0, headless (`codex exec --ephemeral`, server passed as `-c` overrides), against the same fixture: the same seven calls completed with the same results and nothing withheld in any of them. Asked the same question it answered "alpha" without saying that was inferred from the skill graph; the raw tool results carry no answer key, so this is the model's inference and not a leak.
- Both clients against Cloud over Streamable HTTP, with a token from the fixture's scripted authorization-code + PKCE sign-in passed as a bearer header: connected, all nine tools discovered, nine calls completed including `get_run_summary` on the run id the list returned. No email address, integrity score, per-result detail, unanswered person's name or learner identifier appears in any raw result. Asked for the candidate's email and the run's scores, both said no tool provided them.
- This exercises each client's transport and tool handling against the real gate. It does not exercise the clients' own OAuth: Cloud accepts only clients whose ID is listed or whose client metadata document is on an allowed host, and Keycloak refuses dynamic registration, so whether each client can sign in by itself depends on it supporting client ID metadata documents — untested, and it needs a person at a browser.
- Found and fixed on the way (Cloud worktree): the MCP sync worker claimed outbox rows strictly by id, so an organisation's fresh change waited behind every other organisation's due retries; with several hundred broken test organisations in the development database the fixture's connection did not reach Keycloak within ninety seconds. Rows never tried are now claimed first, checked by `tests/mcp_sync.rs`, which fails under the old ordering. And `e2e.py setup` took Keycloak down on exit while leaving the Cloud server up, so the fixture it handed over could not sign anybody in; it now leaves both running until `cleanup`.
- Both fixtures were torn down afterwards and the temporary token files removed.
- Codex CLI's own OAuth sign-in (later the same day): Codex publishes a client ID metadata document at `https://chatgpt.com/oauth/codex/client.json`, on a host Cloud allows, but **Keycloak 26.7.3 rejects it** — its experimental parser refuses unknown fields and the document carries `token_endpoint_auth_methods_supported`, so the sign-in stops at "Client Metadata fetch failed". This matters beyond the fixture: any real client whose document has a field Keycloak does not know cannot sign in through CIMD, and ChatGPT's own document has not been checked yet. With a pre-registered public client instead (`--oauth-client-id`, config key `mcp_servers.<name>.oauth.client_id`), Codex's own login completed — its PKCE, its loopback listener on a random port and a random `/callback/<segment>` path, its code exchange, `iss` returned — and `codex exec` then made all nine calls with the token it stored, with nothing withheld appearing. Keycloak matched the portless loopback registration on any port; the path segment needed `/callback/*`. The stored credential was removed with `codex mcp logout`.
- Claude Code's own OAuth sign-in cannot run headless (the server reports `needs-auth` and no sign-in is offered). The user completed it at a browser in an interactive session (`/mcp` → Authenticate, pre-registered client `alexandria-claude-code-fixture`, callback port 18798), routed by email domain to the stand-in customer realm. The stored sign-in then carried over to a headless `claude -p` run with the same config: connected, all nine tools, nine calls, nothing withheld appearing in any raw result, and the client said no tool gave the candidate's email or the run's scores. The fixture was torn down afterwards.

M7 (2026-09-16, both worktrees):

- Cloud `cargo +1.91.0 test`: 34 suites, 0 failed, including the new `tests/mcp_acceptance.rs`. It seeds two organisations and asks one for every kind of record the other holds — role, candidate, run, verification job, comparison — and each is reported missing, with none of the other organisation's identifiers, names or credential references in the refusal; the same token reads its own role, so the refusals are the boundary and not a broken tool. A cohort of four is withheld with a reason while one of six reports six. A listing whose consent has lapsed is absent from search while a current one is found, and the expired name appears nowhere. Names shaped like tools (`get_run_evidence`, `read_sentinel_evidence`, `list_all_candidates`) are refused with the protocol's own code and return no score material. Fifty skill ids and a ten-thousand character identifier both fail inside a bound, and the refusal does not echo the input back. The `/mcp` harness moved to `tests/common/mod.rs`, shared with the gate test.
- App `cargo +1.91.0 test -p alexandria-mcp -p alexandria-studio -p alexandria-verify`: 116 passed, 0 failed, including the new `crates/alexandria-mcp/tests/acceptance.rs`. Hostile course text carrying a forged tool result, a forged JSON-RPC frame and an instruction to call an unscoped tool arrives whole as content; the session keeps its step afterwards, the draft tools are still refused, and the tool list is unchanged. The broker going away underneath a connected client fails the call within a timeout rather than hanging, and the client process stays a working server. All ten shared verifier vectors go through the real binary: tampered, wrong key, malformed, expired, unbound subject, revoked and rotated key are each distinguishable, none is reported valid, and revocation and accreditation are always `unknown` rather than good. The lifecycle harness moved to `crates/alexandria-mcp/tests/common/mod.rs`.
- `python3 scripts/mcp/e2e.py` against fixture Keycloak 26.7.3: passed with a second organisation, deliberately named so a person could confuse the two, with its own domain, connection and sign-in. Its token names its own organisation; every cross-organisation read is reported missing and discloses nothing; another organisation's module reports nobody; and it reads its own empty list of roles. Only this proves the `organization` claim is what scopes a read — the in-process tests mint their own tokens. Every organisation a run creates is now removed afterwards; the development database was checked and left as found.
- CI: Cloud gained an `mcp-sign-in` job that runs that sign-in end to end against a disposable Keycloak on every push and pull request. The app's CI change filter did not list `crates/`, so a change confined to the MCP crates ran no tests at all; it does now.
- `cargo +1.91.0 clippy --all-targets -- -D warnings` and `cargo +1.91.0 fmt --check`: passed in both worktrees.

M6 (2026-09-16):

- `python3 scripts/mcp/fixture_check.py`: passed. Fourteen tools offered with a grant; the two published courses found; the assessment marked withheld in the outline and withheld when read; the quiz question returned without its answer or explanation; the skill graph showing only the assessed skill; the goal resolving to gamma; the path ordering alpha earned, beta available, gamma locked with a course recommended for the unlocked step; one credential summary without its signed document; and `verify_credential` accepting a supplied vector while reporting revocation unknown.
- `python3 scripts/mcp/e2e.py setup` then `cleanup`: verified by hand — the metadata endpoint answered while it was up, and afterwards the server, containers, organisation, index listings and state file were gone with the development database untouched.
- `python3 scripts/mcp/e2e.py`: the default check still passes with the modes added.

M5 slice 3 (2026-09-16, Cloud worktree):

- `cargo +1.91.0 test --no-fail-fast`: 33 suites, 339 passed, 0 failed, including `a_small_cohort_is_reported_as_withheld_rather_than_left_out`, which checks that every cell is present whether reported or withheld and that a withheld one carries a reason instead of a number.
- `python3 scripts/mcp/e2e.py`: passed with nine tools. It seeds a module of six learners and another of three, plus a bulk job with one valid and one invalid entry, then checks that the report counts the six and names nobody, that the module of three is withheld rather than omitted, that each aggregate read is logged, and that the job reports per-entry validity with no `detail` reaching the assistant.
- `cargo +1.91.0 clippy --all-targets -- -D warnings` and `cargo +1.91.0 fmt --check`: passed. Cloud's deployment doc and README record both tools and the cohort rule.

M5 slice 2 (2026-09-16, Cloud worktree):

- `cargo +1.91.0 test --no-fail-fast`: 33 suites, 338 passed, 0 failed, including `a_run_summary_carries_no_integrity_scores`, which hands the mapping a full set of scores and asserts none of it appears in the result.
- `python3 scripts/mcp/e2e.py`: passed with seven tools. It seeds a recorded, flagged run carrying `composite` and `gaze` scores, then checks that `list_assessment_runs` finds it when filtering to flagged runs, that `get_run_summary` returns its status and credential reference, that no score value appears anywhere in either response, and that reading the run wrote a `run.viewed` row to the organisation's audit trail.
- `cargo +1.91.0 clippy --all-targets -- -D warnings` and `cargo +1.91.0 fmt --check`: passed. Cloud's deployment doc and README record the new tools and the integrity rule.

Provider tests use a controlled loopback HTTP server. No paid model or actual assistant account has been tested. Fixtures use in-memory databases and supplied verifier vectors; real profiles have not been opened or modified by the tests.

## Integration and next work

1. **Migration collision resolved:** `main`'s interview support (migration 083, `interview_assistant`) is merged; Instructor Studio uses 084. The migration runner checks each recorded version rather than only `MAX(version)`, and `interview_migration_applies_after_instructor_studio` proves a profile that applied 084 first still applies 083.
2. **Native broker lifecycle validated at the shared-core level** (see validation record). Connection files are removed on revoke, on expiry pruning (grant issue and access listing, under the grants lock), on profile lock and at app startup; startup also removes sockets of exited brokers. Revoke accepts only canonical UUID grant IDs as file names, and malformed frames receive `invalid_input`. Grants last one hour, clear on profile invalidation, and use private 0700 directories / 0600 files; the executable checks file permissions and refuses symlink opens. Remaining: packaged-app smoke through the real Tauri profile lock/switch flow, and a Windows named-pipe transport (currently reports unavailable; not compiled or tested on Windows).
3. **MCP Release 1 state (2026-09-17):** M0–M7 complete on both branches; M8 in progress. Both servers pass pinned-schema conformance, Inspector 2.6.0 `--strict` with every tool called, and Claude Code plus Codex CLI end to end (Cloud including each client's own OAuth sign-in through a pre-registered client). Remaining for M8: a staging deployment, to be approved, for ChatGPT and Claude on the web, which also exercises a hosted confidential registration for the first time. Under review: Cloud branch `codex/cloud-mcp-authz` as ifftu-dev/alexandria-cloud#1. The app branch `codex/instructor-studio-mcp` was opened as ifftu-dev/alexandria#264, then merged into the local `main` (which already held the rebuild and an earlier merge of this branch) at `c03b07c`; `main` is pushed from there, and #264 closes as merged when it lands. Merge the Cloud PR before staging so it deploys code `main` has. Open risks: Keycloak CIMD (keycloak#51236, canary in `scripts/mcp/e2e.py`); Keycloak 26.8 changes organisation-domain routing; the 2,146 organisations earlier test runs left in the Cloud development database were deleted on 2026-09-17 with their sync rows (suites now run against their own database via `scripts/test.sh`). RUSTSEC-2026-0285 (rustls 0.23.43) is fixed on both: the Cloud branch by a lockfile bump, the app by the local `main` it merged.
4. Continue S5 capability-specific media adapters, then S7 sponsored service. The app currently supports text workflow calls only. Image/audio/video roles produce text briefs/scripts/storyboards until binary provider and artifact-review contracts are implemented.
5. Assessment **authoring** and learner-feedback improvement now use explicit domain policies and the instructor approval queue. Current MCP context deliberately excludes assessments and answers; no learner grading or credential decisions are delegated to a model. Define a signed transport before claiming learner feedback reaches authors across devices.
6. Actual provider interoperability, native packaged-app smoke, remote assistant-client smoke (staging) and translations remain acceptance work. The standalone MCP executable is built with `cargo +1.91.0 build -p alexandria-mcp`; connection UI shows a client configuration, but an app-bundled sidecar/installer is not yet supplied.

### Publishing change

Approved inline text is materialized as public content-addressed text blobs before signing the release. Signed published course documents now use the public blob path so another learner does not need the author's profile content key. Enabled tutor guidance and its instructor-authored public prompt are included in the signed payload and hydrated with the course. Private authoring prompts, source notes, model keys, tutor threads and run history are not included. Publication checks author ownership, profile epoch and whether the captured inline text changed before updating the published course. This does not repair pre-existing publication behavior for other inline element types or guarantee a complete release snapshot under arbitrary concurrent outline edits.

### MCP contract and client configuration

Without `ALEXANDRIA_MCP_CONNECTION_FILE`, the binary exposes only offline `verify_credential` and does not discover profiles. With the connection file produced by Settings → Assistant access, it additionally advertises `search_catalog`, `get_course`, `read_lesson`, `get_skill_graph`, `get_learning_progress`, `resolve_goal` and `compute_learning_path` (`learning:read`), `list_my_credentials`, `get_credential` and `verify_presentation` (`credentials:read`) and `list_course_drafts`, `read_lesson_draft` and `propose_lesson_draft` (`drafts:read`, `drafts:propose`); a grant carries any combination of the four scopes. Every private call is reauthorized by the running app against scope, expiry and profile epoch. The executable itself never opens a profile database or vault. A read lease spans app authorization, database work and response writing; lock/revoke obtains the exclusive lease before invalidation. No new response/write can proceed under a stale grant after that invalidation point.

A proposal only creates a review item under Workflows → Runs. Retry identity includes the grant, client name, request ID and target lesson; exact retries return the prior outcome even after instructor application, while changed payloads and stale target fingerprints conflict. There are no MCP apply, publish, grade or credential-issuance tools.

### Learner reads (M3 slice 1, 2026-09-16)

Decided with the user:

- **Grant UI:** Assistant access is its own Settings section (`/settings/assistants`, `AssistantAccessPanel.vue`) with separate learning, draft-reading and proposal permissions; Instructor → AI settings links there.
- **Visibility:** `learning:read` sees published courses written by someone else, plus catalog announcements. A profile's own courses, whose local copy may hold unpublished edits, stay behind `drafts:read`. Hidden courses and lessons answer `not_found`, the same as missing ones.
- **Lesson content:** text (60,000 characters per call, continued with `next_start`); video chapter markers; quiz, multiple-choice and essay questions rebuilt from an allowlist of learner-visible fields (prompt, context, options, points, essay guidelines, word limits, rubric criteria), so answers, explanations and any field a new format adds are never copied. Credential-bearing assessments, interactive elements and plugins are withheld. Practice quizzes do feed completion and skill evidence; returning their questions without answers was the user's explicit choice.
- **Network:** a lesson body not stored inline is fetched through the content resolver (device store, then peers, then a mapped URL) with a 10-second limit; `read_lesson` is marked open-world and says so. The fetch runs outside the broker lease and the request is authorized again under the lease before rendering, so a slow fetch cannot delay a profile lock or return content after it. The broker's per-request limit is now 15 s and the client's 20 s.

Implementation: `crates/alexandria-studio/src/learning.rs` (queries and rendering), `broker.rs` (`learning:read`, two-phase `read_lesson`, `BrokerHost::fetch_content`), `grants.rs` (three scopes), `alexandria-mcp` tools with typed schemas, `src-tauri/src/commands/studio_mcp.rs` (resolver-backed fetch). Slice 3 follows below.

### Learner skills and goals (M3 slice 2, 2026-09-16)

Decided with the user:

- **Private skills:** the learner's own assistant, running on their own device, receives every earned skill with a `public` flag; anything that leaves the device — the P2P graph service, and later Cloud and instructors — still sends public skills only. The tool description says private skills are included and must not be republished.
- **Goal input:** `resolve_goal` accepts a curated `exam`, `job_role` or `curriculum` template key, or job-description text supplied by the caller. It takes no URL, so no model can make this device issue an outbound request; the app's own UI keeps its link-fetching path.

Shared services (the plan's "do not duplicate graph traversal" rule): `alexandria-studio::skills` now owns the skill-graph build, the learning-path computation with its course recommendations, goal-template lookup, skill entries and progress; `alexandria-studio::jd_parser` owns the on-device text matcher. The app's `p2p/graph_fetch.rs`, `commands/graph.rs`, `commands/goal_templates.rs` and `goals/jd_parser.rs` now call or re-export those, keeping their existing DTOs, public paths and behaviour. Settings the graph needs (`identity.local_did`, `instructor.graph_prefs`) are read from `app_settings` directly, so the studio crate stays free of app-only types.

Bounds: at most 50 goal skills, 100,000 characters of goal text, 200 enrolments and 500 lesson progress rows per call, and up to three course recommendations per unearned skill.

### Credentials and presentations (M3 slice 3, 2026-09-16)

Decided with the user:

- **Credential depth:** `list_my_credentials` and `get_credential` return issuer, subject, type, claim kind, skill, dates, revocation state, whether a status list exists, supersession and the integrity hash — never `signed_vc_json`. The signed document *is* the credential: anyone holding it can present it onward, so handing it to an assistant would send the whole claim to that assistant's model provider. Only the learner's own credentials are listed; another subject's reports `not_found` rather than a refusal, so the tool cannot be used to probe what else the device holds.
- **Presentation verification:** broker-backed, with the real replay check. This is the one tool in the learner set that writes: accepting a presentation records its `(audience, nonce)` pair in `presentations_seen`, so the same presentation cannot be accepted twice. Its annotations say so (`read_only_hint = false`), and it sits behind `credentials:read` because it reads and changes device state.
- **Permission:** a fourth grant scope, `credentials:read`, with its own checkbox in Settings → Assistant access. It is never implied by `learning:read`.

Shared services: `alexandria-studio::credentials` owns the summaries and the verification, which keeps the original's deliberate ordering — audience, then the replay probe, then the signature — so response timing cannot reveal whether a nonce was used, and nothing is recorded unless a signature verifies. The envelope and verdict types moved there and the app re-exports them, so `commands/presentation.rs` keeps its public surface and IPC payloads unchanged while delegating. Studio gained `alexandria-verify` (for `did:key` resolution and base64url) and `ed25519-dalek`; the permissive verifier crate was deliberately left untouched rather than grown an app-specific presentation format.

Bounds: 50 summaries per page, a 256,000-byte presentation payload, and a 300-character audience.

### MCP 2026-07-28 conformance ledger (stdio)

Revision: 2026-07-28 (latest stable, checked 2026-09-15). Schema: pinned commit `271ecc9`. SDK: rmcp 3.3.0. Tests are in `crates/alexandria-mcp/tests/` unless noted; "SDK" means rmcp behaviour verified by these tests, not assumed from SDK claims.

| ID | Requirement | Spec | Owner and location | Test or evidence | Status |
|---|---|---|---|---|---|
| C1 | Latest stable revision identified | `/specification/latest` | — | Rechecked 2026-09-15: 2026-07-28 | Verified |
| C2 | Validate against the authoritative schema, pinned | `schema/2026-07-28` | `tests/schema/` (NOTICE, SHA-256) | `conformance::pinned_schema_rejects_nonconforming_results`; every modern message in `conformance.rs` | Pass |
| C3 | stdout carries only valid MCP messages; newline-delimited, no embedded newlines | `basic/transports/stdio` | App: `src/stdio.rs` single writer | `Server::recv` validates each line as `JSONRPCMessage`; `scripts/mcp/smoke.py` | Pass |
| C4 | MUST implement `server/discover`; `DiscoverResult`, cache hints, `serverInfo` | `server/discover`, `server/utilities/caching` | SDK | `discovery_versions_and_request_metadata` | Pass |
| C5 | Missing required `_meta` fields → `-32602` | `basic#_meta` | SDK | same | Pass |
| C6 | Unsupported version → `-32022` with `requested`/`supported` | `basic/versioning` | SDK | same (`UnsupportedProtocolVersionError`) | Pass |
| C7 | Results carry `resultType` | `basic` | SDK | Schema validation of every modern result | Pass |
| C8 | `tools` capability; `tools/list` `ttlMs`/`cacheScope`; deterministic order; grant-dependent list is `private` | `server/tools`, caching | App: `list_tools` override in `src/main.rs` | `tools_list_and_call_conform_to_schema` (with and without broker) | Pass |
| C9 | Tool `inputSchema`/`outputSchema` valid 2020-12; `structuredContent` conforms; text mirror (SHOULD) | `server/tools`, `basic#json-schema-usage` | App (schemars) | same | Pass |
| C10 | Unknown tool → protocol error; tool failures → `isError` result | `server/tools#error-handling` | SDK + app | same | Pass |
| C11 | Unadvertised features and removed methods are not served | `basic`, changelog | App overrides (resources, templates, prompts, completion); SDK (`ping`, `logging/setLevel`, reads, custom) | `unadvertised_and_removed_methods_are_not_found` (`-32601`) | Pass |
| C12 | JSON-RPC parse error `-32700`, invalid request `-32600` (including null or non-integer IDs), session continues | JSON-RPC 2.0, `basic#messages` | App: `src/stdio.rs` (rmcp's codec ended the session) | `invalid_messages_are_rejected_without_ending_the_session` | Pass |
| C13 | Notifications never receive responses | `basic#notifications` | SDK + app | same; `messages_before_the_first_request_do_not_end_the_session` | Pass |
| C14 | Stateless: no reliance on prior connection state | `basic#statelessness` | App workaround: rmcp 3.3.0 ends serving unless the first message is a request, so framing drops earlier notifications/responses | `messages_before_the_first_request_do_not_end_the_session` | Pass (SDK gap worked around) |
| C15 | stdio cancellation: no further messages for a cancelled request; SHOULD stop work | `basic/patterns/cancellation`, stdio | SDK suppresses the response; an already-started broker exchange completes | `cancelled_requests_receive_no_response` | Pass; SHOULD departure: started proposal writes are allowed to finish so they commit once or not at all |
| C16 | Exit promptly on stdin EOF | stdio shutdown | App | `stdin_eof_ends_the_process_cleanly` | Pass |
| C17 | Bounded input | App policy | `src/stdio.rs` 256 KiB frames; tool-level limits | `stdio::oversized_stdio_frame_is_bounded_and_process_stops` | Pass |
| C18 | Dual-era: each advertised legacy revision via `initialize`, without modern-only fields | `basic/versioning#backward-compatibility` | SDK | `each_advertised_legacy_revision_initializes_independently` | Pass (assertions; legacy schemas not pinned) |
| C19 | `_meta.io.modelcontextprotocol/serverInfo` on results (SHOULD) | `basic#_meta` | SDK (discover); app (`tools/list`, `tools/call`) | discovery and tools tests | Pass |
| C20 | Tool naming (SHOULD) | `server/tools#tool-names` | App | tools test | Pass |
| C21 | Tool security MUSTs: validate inputs, access control, sanitize outputs, rate limit | `server/tools#security-considerations` | App: typed inputs and bounds; broker grants; broker concurrency limit 8 | `broker_lifecycle.rs`, `stdio.rs` | Partial: no per-request rate limit inside the single-client stdio process |
| C22 | Client compatibility portability (not normative): no JSON Schema `type` arrays | Inspector `--strict` | App schemars | tools test; Inspector 2.6.0 `--strict` | Pass |
| C23 | Optional features not advertised: resources, prompts, completion, logging, `subscriptions/listen`, MRTR/elicitation, Tasks extension | capabilities | — | `capabilities` is exactly `{"tools":{}}` | Omitted |
| C24 | Streamable HTTP, header validation, Origin, OAuth | transports, authorization | Cloud (M4) | — | Not applicable to stdio. Cloud `/mcp` (M4 foundation): Streamable HTTP with rmcp 3.3.0, no sessions, Host allowlist, bearer challenge with protected resource metadata, audience/issuer/client checks, `insufficient_scope` step-up; Cloud `tests/mcp_gate.rs` and `scripts/mcp/e2e.py` pass. Pinned-schema validation of discovery, tool list, all nine results (each also against its declared `outputSchema`) and error envelopes (invalid params, method not found for unadvertised capabilities, parse error, header mismatch, unsupported version) in `tests/mcp_conformance.rs`; Inspector 2.6.0 CLI over HTTP `--strict` passes with every tool called (2026-09-17) |
| C25 | Independent clients | Desktop plan §5 | — | rmcp client (`stdio.rs`, `broker_lifecycle.rs`), raw Rust driver (`conformance.rs`), Python (`smoke.py`), Inspector 2.6.0 CLI | Pass; no real assistant client tested |

### Cloud OAuth provider (M0)

**Decision (2026-09-16):** Keycloak, pinned to `quay.io/keycloak/keycloak:26.7.3`, started with `--features=cimd` only. One dedicated realm per Cloud deployment region is the MCP authorization server; Cloud `/mcp` is the resource server. Selected by the user after the comparison below.

Verified realm design (local spike, 2026-09-16). The spike scripts were deleted once the Cloud fixture (`scripts/mcp/compose.yml`, `scripts/mcp/e2e.py`) covered the same checks.

- **Discovery and `iss`:** RFC 8414 metadata at `/.well-known/oauth-authorization-server/realms/<realm>` and OIDC discovery advertise `client_id_metadata_document_supported` and `authorization_response_iss_parameter_supported`. `iss` was returned on success and error redirects and matched the issuer.
- **Registration:** CIMD through a client policy: condition `client-id-uri` (`client-id-uri-scheme`, `client-id-uri-allow-permitted-domains`) with profile executor `client-id-metadata-document`. `cimd-allow-permitted-domains` must cover the `client_id` host and the redirect hosts; `cimd-allow-http-scheme` is for local tests only. Anonymous DCR stays disabled (the default Trusted Hosts policy rejects it); pre-registered clients remain possible.
- **PKCE:** a realm-wide policy, condition `any-client` with executor `pkce-enforcer` (`auto-configure`), is required. The enforcer inside the `client-id-uri` policy is not applied to CIMD clients ([keycloak#52795](https://github.com/keycloak/keycloak/issues/52795), open, no milestone); without the realm-wide policy a CIMD client obtained a token with no PKCE. With it, missing PKCE was refused.
- **Audience:** a realm default client scope with `oidc-audience-mapper` (`included.custom.audience` = canonical MCP URI). CIMD clients, a tenant-brokered user and registered clients received `aud` equal to the MCP URI. The realm serves only this resource, so binding does not depend on RFC 8707 `resource` (clients still send it; Keycloak ignores it with the flag off). Cloud must reject any token whose `aud` is not its canonical URI.
- **Do not enable `resource-indicators` (experimental):** with it, CIMD token requests carrying `resource` failed on 26.7.3 (HTTP 500, `NullPointerException` in `ResourceIndicatorsPostProcessor`; the null check is on `main`) and still failed on nightly `dd597f8` (`invalid_target`). DCR and `client_credentials` requests also failed on 26.7.3. Only admin-registered clients were bound correctly.
- **CIMD scope snapshot:** a CIMD client's scopes are fixed when Keycloak first fetches its document (`min-cache-time` 300 s, `max-cache-time` 259200 s). A client first seen before the audience scope existed received tokens without `aud` and `invalid_scope` for the new scope; fresh clients worked. Configure the realm before exposure and re-verify refresh behaviour before changing scopes in production.
- **Tenant routing:** Keycloak Organizations. An organization holds the tenant's verified domain; its linked OIDC identity provider sets `kc.org.domain` and `kc.org.broker.redirect.mode.email-matches=true`. Identity-first login for `ada@acme.test` redirected to the tenant IdP (a second realm standing in for Entra or Google), created a brokered user linked to it and an organization member, and the token carried `organization: ["acme-org"]` and `email` when the `organization` scope was requested.

M4 obligations and open questions:

- **Principal resolution:** Cloud resolves the actor from server state: token `organization` alias to `organizations.id`, brokered identity or email to a `users` row in that organization, then the existing status, role, module scope and residency checks on every request. The token's organization is never trusted without a membership row.
- **Decided (2026-09-16) — tenant IdP source of truth:** Cloud's `idp_connections` and `idp_login_domains` remain the only place per-tenant SSO is configured; Keycloak Organizations are a derived copy. Console sign-in stays on Cloud's existing OIDC path. Today these rows are written by the `provision` CLI (`src/bin/provision.rs`), `rekey.rs` and seeders, not a console screen.
- **Decided — sync:** each SSO write also inserts an outbox row in the same transaction; a worker applies it to Keycloak through the Admin API with retries, and a periodic reconciler compares Cloud and Keycloak and repairs drift. MCP sign-in may lag a change briefly; sync state must be visible and alerting on failures is required.
- **Decided — Keycloak's copy of tenant secrets:** written by the sync worker into the Keycloak IdP configuration through the Admin API. Keycloak uses its own Postgres instance, encrypted at rest with encrypted backups; realm exports are disabled or scrubbed and Keycloak admin rights are limited to the sync service account. A vault backend was rejected because Cloud runs as stateless replicas with no shared disk or Kubernetes assumption. Residual risk: plaintext secrets inside Keycloak's database and backups.
- **Decided — user mapping:** the sync configures, per tenant IdP, a Keycloak importer that copies the same stable subject claim Cloud's `auth/profile.rs` uses (`oid` for Entra, `sub` otherwise) onto the brokered user, and a token mapper that emits it. Cloud resolves the actor by `users (idp_connection, idp_subject)` — the key console sign-in already uses — then applies status, role, module and residency checks. No email matching and no user creation through MCP sign-in; a wrong claim configuration fails closed.
- **Decided — assistant clients** *(superseded in part on 2026-09-16: Keycloak cannot read the Codex and ChatGPT documents, so assistants are pre-registered until it can; the allowlist below stays for when it does)*: CIMD `client_id` domains are an explicit, configured allowlist; unknown domains are refused. Initial allowlist: Claude Code (`https://claude.ai/oauth/claude-code-client-metadata`, loopback redirects matched port-agnostically), VS Code (`https://vscode.dev/oauth/client-metadata.json`), Claude web/Desktop connectors (callback `https://claude.ai/api/mcp/auth_callback`), ChatGPT connectors (`https://chatgpt.com/oauth/client.json`) and Codex CLI (`https://chatgpt.com/oauth/codex/client.json` or a callback-specific chatgpt.com document). MCP Inspector 2.6.0 has no CIMD document and uses a pre-registered public client that exists only in fixture realms. Trusted domains must also cover every redirect, `client_uri`, `logo_uri` and `jwks_uri` host in those documents: `claude.ai`, `vscode.dev`, `code.visualstudio.com`, `chatgpt.com`, `persistent.oaistatic.com`, `localhost` and `127.0.0.1` (documents fetched 2026-09-16). ChatGPT's document declares a confidential client (`private_key_jwt` with `https://chatgpt.com/oauth/jwks.json`); Codex and Claude Code are public loopback clients. Claude's hosted-connector CIMD document URL is not published; the `claude.ai` domain entry covers it and it is verified at staging. Keycloak 26.7.3 advertises `none` in `token_endpoint_auth_methods_supported` and `offline_access` in `scopes_supported`, which Claude and ChatGPT require for CIMD and refresh tokens.
- **Decided — clients without CIMD** (OpenCode, Cline, Continue, LibreChat and others): unsupported until they ship CIMD; dynamic client registration stays disabled. Goose and Zed publish CIMD documents but are not on the initial list.
- **Decided — remote connector testing:** Claude web/Desktop and ChatGPT cannot reach a localhost fixture; they are configured but verified only after an explicitly approved staging deployment (M8). Local clients are tested against the fixture.
- **Decided — `list_roles` content:** roles, requirements and the console's talent-index counts (per-requirement meeting count, coverage and the all-requirements ceiling). In MCP results, counts from 1 to 4 are returned as suppressed (cohort `MIN_CELL` = 5), and coverage and ceiling values derived from a suppressed count are suppressed with it. The console is unchanged.
- **Decided — operator CLI:** `provision mcp status --org` (Keycloak organization, IdP, importer claim, last sync result, outbox backlog), `provision mcp resync --org` (re-queue a full sync) and `provision mcp revoke --org --email` (delete the brokered Keycloak user and sessions), audited like other provisioning actions; no console screen yet.
- **Decided — local Keycloak:** a separate MCP fixture Compose project (Keycloak 26.7.3, its own Postgres, a stand-in tenant IdP realm, ephemeral ports); Cloud's existing `docker-compose.yml` is unchanged.
- **Decided — sync worker placement:** a background task inside each Cloud server replica, like retention; outbox rows are claimed with `FOR UPDATE SKIP LOCKED` and the reconciler runs under a Postgres advisory lock. Metrics and alerts use the existing `/metrics` and `deploy/alerts.yml`.
- **Decided — sync credential:** a confidential service-account client inside the MCP realm with only the realm-management roles it needs (client credentials); never a master-realm admin.
- **Decided — realm setup:** an idempotent `provision mcp bootstrap` applies the realm configuration (audience scope, realm-wide PKCE policy, CIMD trust policy and allowlist, Organizations, user-profile attribute, sync service account) through the Admin API; the reconciler re-verifies the security-critical parts and `/mcp` refuses to serve when they are missing.
- **Decided — token lifetimes:** access tokens 15 minutes (so `mcp revoke` fully applies within 15 minutes; Cloud still rechecks user status and membership on every request); refresh tokens rotate on use with 8-hour idle and 30-day maximum session lifetime. Verify that CIMD clients receive refresh tokens; the spike's CIMD token responses had none.
- **Build risk (resolved):** Keycloak's declarative user profile drops undeclared attributes; bootstrap declares `upstream_sub` (admin-only), and the fixture confirmed the brokered user carries it and the access token emits it.
- **Decided — toolchain:** raise Cloud's declared `rust-version` from 1.83.0 to 1.91.0, matching CI and the app (rmcp requires 1.88).
- **Decided — delivery:** the first M4 slice includes the whole foundation: Keycloak fixture and realm bootstrap, `/mcp` resource server and `list_roles`, link-once mapping, outbox sync worker, reconciler, vault writer and provisioning changes.
- **Customer onboarding impact:** Keycloak brokers at its own callback (`/realms/<realm>/broker/<alias>/endpoint`), so each customer must add that redirect URI to their IdP application registration before MCP sign-in works for their organization.
- **Scopes:** create the Release 1 scopes (`roles:read`, …) as realm client scopes before any CIMD client is fetched, and advertise them in `WWW-Authenticate` and `scopes_supported`.
- **CIMD trust policy:** production allows only `https` and an explicit allowlist of assistant client domains; evaluate `cimd-restrict-same-domain` against the loopback redirect URIs desktop clients use.
- **Operations:** Keycloak per region with its own Postgres database (not the Cloud tenant schema); Admin API credentials kept apart from Cloud's restricted database role; Cloud refuses to serve `/mcp` if the realm lacks the PKCE and audience policies.
- **Upgrade risk:** Keycloak 26.8 (milestone due 2026-09-30) migrates domain-to-IdP routing from `kc.org.domain` to organization domains (`MigrateTo26_8_0`) and plans to promote CIMD and resource indicators to preview. Rerun the spike scripts before upgrading.
- **Local tests (M6/M7):** run the pinned container in the isolated fixture Compose project with a stand-in tenant-IdP realm; the spike scripts already cover discovery, CIMD, PKCE, audience, `iss` and tenant routing.

### Cloud M4 foundation (2026-09-16)

Cloud branch `codex/cloud-mcp-authz`, committed locally only (not pushed). Cloud `rust-version` is now 1.91.0.

- **Resource server:** `src/http/mcp.rs` mounts `/mcp` (rmcp 3.3.0 Streamable HTTP, JSON responses, no sessions, 1 MiB bodies, Host allowlist from `MCP_PUBLIC_URL`) and `/.well-known/oauth-protected-resource/mcp`. A gate before rmcp verifies the bearer (`src/mcp/token.rs`: asymmetric algorithms, `kid`, issuer, audience, expiry, `typ: Bearer`, `azp` either an allowlisted CIMD host or a fixture client ID refused in release builds, one organization), resolves the principal from Cloud state (`src/mcp/principal.rs`: connection, upstream subject and organization must match an active `users` row in this deployment's region) and requires `roles:read` for `list_roles`. JWKS are cached for 5 minutes and refetched on an unknown key. `/mcp` has its own rate-limit class (120/min).
- **`list_roles`:** `src/roles.rs` now serves both the console (unchanged, exact counts) and MCP, where counts 1–4 and values derived from them are suppressed.
- **Keycloak:** `src/keycloak.rs`. `provision mcp bootstrap` converges the realm; `provision mcp status|resync|revoke` operate it. Tenant sync creates, per Cloud connection, an OIDC identity provider (`c-<connection>`, discovery import, PKCE, signature validation, the tenant claim filter console sign-in enforces, upstream-subject importer) and an organization (`<org uuid>`) holding every Cloud sign-in domain.
- **Sync:** migration 0032 adds `mcp_sync_outbox` and `mcp_sync_state` (RLS, not readable by `alexandria_app`). `provision idp add|set-secret` and `domain add|remove` enqueue in the same tenant transaction. Each server replica drains with `FOR UPDATE SKIP LOCKED` and exponential backoff (30 s to 1 h); an hourly reconciler under an advisory lock requeues every organization and removes Keycloak organizations that no longer exist in Cloud. Metrics `alexandria_mcp_sync_*` and alerts `McpSyncFailing`, `McpReconcileStopped`. Audit action `mcp.revoked`.
- **Found by the fixture and fixed:** (1) a realm's default roles include the `account` client roles, so tokens also carried `aud: account` and were valid at Keycloak's account API; bootstrap removes client roles from the default roles and tokens now carry only the MCP resource. (2) Enabling Organizations makes the `organization` scope a realm *optional* scope; assigning it as default returned 409, which bootstrap had treated as success, so tokens had no `organization` claim. Bootstrap now moves scopes between the default and optional lists.
- **Known limitations:** Keycloak's `kc.org.domain` takes a single domain, so only a connection's first domain routes identity-first sign-in (the organization still holds all domains); Keycloak 26.8 changes this. Sync refuses connections whose client secret predates sealing (`provision idp set-secret` first). CIMD refresh-token issuance, real CIMD clients and remote connectors are unverified. The `/mcp` refusal when bootstrap is missing is enforced by failing token checks, not a separate realm probe.
- **Docs:** Cloud `docs/deployment.md` (MCP for organisations) and `README.md` updated; `.env.example` lists the variables.
- **Scopes (decided 2026-09-16, Cloud commit `6d1d051`):** bootstrap registers every Release 1 scope (`roles:read`, `talent:read`, `candidates:read`, `runs:read`, `cohorts:read`, `verification:read`) before any client connects, avoiding the CIMD scope snapshot; protected resource metadata and challenges advertise only scopes with tools (`roles:read`). Fixture end-to-end rerun passed.

Validation (2026-09-16, Cloud worktree):

- `cargo +1.91.0 fmt --check` and `cargo +1.91.0 clippy --all-targets -- -D warnings`: passed.
- `cargo +1.91.0 test --no-fail-fast` against the local Postgres: 33 suites, 336 tests passed, including `mcp_gate` (challenge, refusals, admission with locally signed tokens), `secrets` and `tenancy` guards for the new tables, route-authorisation and metrics exposition.
- `python3 scripts/mcp/e2e.py` (fixture Keycloak 26.7.3 plus a stand-in customer realm, fixture-only credentials, torn down afterwards): passed — realm bootstrap; outbox sync created the identity provider; protected resource metadata; unauthenticated 401 challenge; identity-first sign-in by email domain through the customer realm with PKCE, `iss` and `state`; token `aud` exactly the resource, `organization`, `idp`, `upstream_sub` and `roles:read`; `tools/list` and `list_roles`; 403 `insufficient_scope` without `roles:read`; a suspended user's valid token refused; `provision mcp revoke` invalidates the refresh token.

### Cloud evidence tools (M5 slice 1, 2026-09-16)

Decided with the user:

- **Names in `search_talent`:** a match carries its DID, its hits and its gaps; the published display name is included only for somebody who has already sent this organisation a verified presentation. Being listed in the index is consent to be found, not consent to be named to a third-party model. The console screen is unchanged and still shows names.
- **Candidate summaries:** no email address. Id, DID, name, when they were added, and their assessment runs with status and integrity flag. The contact address, and the evidence behind a flag, stay in the console.

Implemented: `get_role`, `search_talent`, `get_candidate_summary` and `compare_candidate_to_role`, beside `list_roles`. Scopes are `roles:read`, `talent:read` and `candidates:read`; the comparison reads a candidate *and* a role, so the gate now requires every scope a tool names rather than a single one, and the challenge names the missing one.

Shared services: `src/talent_index.rs` (search with its `talent_searches` log, one person's held skills, the prerequisite graph) and `src/candidates.rs` (a candidate with their runs), plus `roles::one`. The console's search handler reads through the same service, so a recruiter and their assistant see the same people under the same rules; its JSON is unchanged.

Worth stating: `search_talent` writes the search log, so it is annotated as not read-only. A comparison reports `listed_in_index: false` when somebody has no current listing — absent evidence, not evidence of absence — and reports per-requirement standings (met, short on evidence, short on Bloom, reachable, or blocked with the prerequisites that come first) plus a count of assessments that would close the gaps. Never a single suitability score.

### Cloud assessment runs (M5 slice 2, 2026-09-16)

Decided with the user: `get_run_summary` returns the integrity **flag** and no score numbers — not even the composite the console's run list shows. An assistant can say that a run completed and that it was flagged; what a flag means is read in the console, by a named person, with the adjudication path around it. A model ranking people by an integrity number is the inference the design exists to prevent.

Evidence a candidate released (`evidence_releases`: camera frames, keystroke, mouse, gaze) is not reachable through `/mcp` at all, and its existence is not reported either. That table is candidate-initiated by design: there is deliberately no endpoint an organisation can call to ask for it and no record of asking, because a request that can be refused leaks the refusal.

Implemented: `list_assessment_runs` (filter by candidate, status or flagged only) and `get_run_summary`, under `runs:read`. Scores travel only when a caller asks: the console's screens do, `/mcp` does not, and `RunSummary` has no field for them, so the exclusion is structural rather than a mapping line that could be edited later. Reading a run writes the same `run.viewed` audit row the console writes, naming the person whose token was used. `src/runs.rs` is the shared service; the console's run list and detail now read through it, with paging made explicit — no limit for the screen, a page for assistants.

### Cloud cohorts and verification jobs (M5 slice 3, 2026-09-16)

Decided with the user:

- **Cohort reports are aggregate only, with no cohort filter.** Counts and the Bloom spread for a module, nobody named — even where that reader's own role would show names in the console. Dropping the filter removes the differencing problem rather than mitigating it: suppression answers a single cell, and module-minus-cohort can imply a group under the threshold while both cells clear it.
- **Verification jobs report state, progress and per-entry validity**, never the per-result `detail`. That blob is an open-shaped record of how a verification went, so whatever it grows to later would travel with it.

Cells below five are returned marked withheld, with the reason, rather than omitted — a reader cannot infer a value from which rows are missing, and an institution learns its cohort is too small to report on. Each aggregate read writes a `cohort_access_log` row, because an aggregate that included somebody is still a use of their data.

`src/cohort.rs` gained the shared row query, `summarise` and the aggregate `module_report`; `src/verification.rs` is new. The console's cohort screen and job screen read through them, keeping their named lists, per-learner logs, cohort filter and result detail, so screen and assistant count a module one way.

### Local fixtures (M6, 2026-09-16)

Decided with the user: the app-side fixture is a **headless host**, not the desktop application. `crates/alexandria-studio/examples/fixture_host.rs` builds a throwaway profile, serves it over the same broker core the app runs, issues a grant and prints what to paste into an MCP client. It proves the tools and the broker against data whose shape is known; it does not exercise the vault or the profile lock/switch lifecycle, and says so.

- App: `scripts/mcp/fixture.sh` serves it; `scripts/mcp/fixture_check.py` drives the real stdio binary against the grant and asserts every claim the fixture prints. The fixture lives under `/tmp/alexandria-mcp-fixture` because a Unix socket path has a hard length limit this checkout exceeds.
- Cloud: `scripts/mcp/e2e.py` gained `setup` and `cleanup` beside the default check. Setup leaves the stack running and prints the resource, the realm, the sign-in it created and the seeded identifiers; cleanup stops the server, removes the fixture Compose project, and deletes the organisation and global index listings that run created. Verified by running setup, calling the metadata endpoint, then cleaning up and confirming the server, containers, organisation and state file were gone while the development database was untouched.

Fixture data present: two published courses; the alpha → beta → gamma chain with the owner assessed on alpha only, so the goal is two steps away; a quiz whose answers exist in the database and must not be returned; an assessment that must never be shared; a Cloud organisation with a role, two listed people (one who answered it, one who did not), a candidate in the index and one outside it, a flagged run carrying scores, a module of six and one of three, and a verification job with one valid and one invalid entry.

Closed under M7 (2026-09-16): two Cloud organisations with deliberately similar names, separate candidates and separate verification jobs, live in `scripts/mcp/e2e.py` and in process in `tests/mcp_acceptance.rs`; cohorts of exactly four and six; a listing whose consent has lapsed, absent from search; two learner profiles on the app side, already covered by the lifecycle suite's two isolated profiles; and all ten shared verifier vectors — tampered, wrong key, malformed, expired, unbound subject, revoked, rotated key — driven through the real binary.

One item on the original list does not apply: distinct recruiter and instructor identities with *different module scopes*. Cohort reporting takes no cohort filter by decision (suppression answers one cell, not the difference between two), so there is no per-module scope to differ on. What the fixture checks instead is the scope step-up path — a token with none of the scopes, and a token holding one of the two a comparison needs — with a live token each time.

Comparison evidence: oidc-provider 9.12.2 (MIT) passed every check, including a delegated-login authorization code flow with a resource-bound JWT and code-replay rejection. Ory Hydra v26.2.0 advertised no CIMD, no `iss` parameter and no RFC 8707 support.

The Desktop reference plan was rechecked on 2026-09-15; its mtime was 13:56 local. The other checkout's interview changes and Alexandria Cloud's existing `.DS_Store` were left untouched. No background implementation job or completion of later slices is implied by this plan.
