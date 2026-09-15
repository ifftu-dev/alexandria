# Instructor Studio and MCP implementation

Updated: 2026-09-15

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
| M0/M1 — MCP baseline and verifier | Existing verifier | Pin SDK, isolated standalone executable, real stdio tests, independent client, bounded input, no vault startup | M1 exit checks pass; stdio conformance ledger, pinned 2026-07-28 schema tests and Inspector 2.6.0 CLI (`--strict`) pass; Cloud OAuth provider selected (Keycloak 26.7.3, spike verified); Cloud implementation (M4) pending |
| S4 — MCP broker and instructor draft tools | S1, M1 | OS-local broker, trusted-app grants, profile epoch, revocation, read text drafts, idempotent proposals into the same review queue | Adapter, grants, UI and review queue implemented; shared broker core passes native two-profile lifecycle tests with real MCP client processes; packaged-app smoke and Windows transport remain open |
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
- Authoritative schema pinned: `schema/2026-07-28/schema.json` at commit `271ecc9accafdd9b83a3c869fa67c22953b2af80` (SHA-256 `ef70b61f99b6d2e5e3b46863822eab08dff6a45bedc7a08914e0e5b133f40203`), vendored unmodified in `crates/alexandria-mcp/tests/schema/` with its NOTICE and license (Apache-2.0, older contributions MIT). `/specification/latest` rechecked as 2026-07-28 on 2026-09-15.
- Tested the real executable through an rmcp client, an independent raw Rust JSON-RPC driver that validates every stdout message against the pinned schema, the Python raw client, and Inspector 2.6.0 CLI (valid/tampered calls; `--strict` with and without a configured broker). See the conformance ledger below.
- Dual-era server: 2026-07-28 per-request metadata, plus legacy `initialize` for 2024-11-05, 2025-03-26, 2025-06-18 and 2025-11-25 as advertised by rmcp 3.3.0.
- **Not a full conformance claim:** legacy revisions are checked by assertions rather than their own pinned schemas; Inspector graphical checks, supported assistant-client smoke testing, Streamable HTTP and Cloud OAuth conformance remain outstanding.

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

Provider tests use a controlled loopback HTTP server. No paid model or actual assistant account has been tested. Fixtures use in-memory databases and supplied verifier vectors; real profiles have not been opened or modified by the tests.

## Integration and next work

1. **Migration collision resolved:** `main`'s interview support (migration 083, `interview_assistant`) is merged; Instructor Studio uses 084. The migration runner checks each recorded version rather than only `MAX(version)`, and `interview_migration_applies_after_instructor_studio` proves a profile that applied 084 first still applies 083.
2. **Native broker lifecycle validated at the shared-core level** (see validation record). Connection files are removed on revoke, on expiry pruning (grant issue and access listing, under the grants lock), on profile lock and at app startup; startup also removes sockets of exited brokers. Revoke accepts only canonical UUID grant IDs as file names, and malformed frames receive `invalid_input`. Grants last one hour, clear on profile invalidation, and use private 0700 directories / 0600 files; the executable checks file permissions and refuses symlink opens. Remaining: packaged-app smoke through the real Tauri profile lock/switch flow, and a Windows named-pipe transport (currently reports unavailable; not compiled or tested on Windows).
3. **M0 complete for selection:** stdio conformance ledger done and Cloud OAuth provider selected (Keycloak; see Cloud OAuth provider section). Before M4 implementation, decide the tenant IdP source of truth listed there. M1's offline verifier is independently usable now. Do not claim full M2–M8 conformance or Cloud completion from the narrow draft integration.
4. Continue S5 capability-specific media adapters, then S7 sponsored service. The app currently supports text workflow calls only. Image/audio/video roles produce text briefs/scripts/storyboards until binary provider and artifact-review contracts are implemented.
5. Assessment **authoring** and learner-feedback improvement now use explicit domain policies and the instructor approval queue. Current MCP context deliberately excludes assessments and answers; no learner grading or credential decisions are delegated to a model. Define a signed transport before claiming learner feedback reaches authors across devices.
6. Actual provider interoperability, native packaged-app smoke, full supported assistant-client smoke and translations remain acceptance work. The standalone MCP executable is built with `cargo +1.91.0 build -p alexandria-mcp`; connection UI shows a client configuration, but an app-bundled sidecar/installer is not yet supplied.

### Publishing change

Approved inline text is materialized as public content-addressed text blobs before signing the release. Signed published course documents now use the public blob path so another learner does not need the author's profile content key. Enabled tutor guidance and its instructor-authored public prompt are included in the signed payload and hydrated with the course. Private authoring prompts, source notes, model keys, tutor threads and run history are not included. Publication checks author ownership, profile epoch and whether the captured inline text changed before updating the published course. This does not repair pre-existing publication behavior for other inline element types or guarantee a complete release snapshot under arbitrary concurrent outline edits.

### MCP contract and client configuration

Without `ALEXANDRIA_MCP_CONNECTION_FILE`, the binary exposes only offline `verify_credential` and does not discover profiles. With the connection file produced by AI settings → Assistant access, it additionally advertises `list_course_drafts`, `read_lesson_draft`, and `propose_lesson_draft`. Every private call is reauthorized by the running app against scope, expiry and profile epoch. The executable itself never opens a profile database or vault. A read lease spans app authorization, database work and response writing; lock/revoke obtains the exclusive lease before invalidation. No new response/write can proceed under a stale grant after that invalidation point.

A proposal only creates a review item under Workflows → Runs. Retry identity includes the grant, client name, request ID and target lesson; exact retries return the prior outcome even after instructor application, while changed payloads and stale target fingerprints conflict. There are no MCP apply, publish, grade or credential-issuance tools.

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
| C24 | Streamable HTTP, header validation, Origin, OAuth | transports, authorization | Cloud (M4) | — | Not applicable to stdio; provider selected (see Cloud OAuth provider section); implementation pending M4 |
| C25 | Independent clients | Desktop plan §5 | — | rmcp client (`stdio.rs`, `broker_lifecycle.rs`), raw Rust driver (`conformance.rs`), Python (`smoke.py`), Inspector 2.6.0 CLI | Pass; no real assistant client tested |

### Cloud OAuth provider (M0)

**Decision (2026-09-16):** Keycloak, pinned to `quay.io/keycloak/keycloak:26.7.3`, started with `--features=cimd` only. One dedicated realm per Cloud deployment region is the MCP authorization server; Cloud `/mcp` is the resource server. Selected by the user after the comparison below.

Verified realm design (local spike, 2026-09-16). Scripts: `worktrees/alexandria-cloud-mcp/scripts/mcp/keycloak-spike/` on Cloud branch `codex/cloud-mcp-authz` (uncommitted; `KC_URL` selects the instance).

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
- **Decided — assistant clients:** CIMD `client_id` domains are an explicit, configured allowlist; unknown domains are refused. Initial allowlist: Claude Code (`https://claude.ai/oauth/claude-code-client-metadata`, loopback redirects matched port-agnostically), VS Code (`https://vscode.dev/oauth/client-metadata.json`), Claude web/Desktop connectors (callback `https://claude.ai/api/mcp/auth_callback`), ChatGPT connectors (`https://chatgpt.com/oauth/client.json`) and Codex CLI (`https://chatgpt.com/oauth/codex/client.json` or a callback-specific chatgpt.com document). MCP Inspector 2.6.0 has no CIMD document and uses a pre-registered public client that exists only in fixture realms. Trusted domains must also cover every redirect, `client_uri`, `logo_uri` and `jwks_uri` host in those documents: `claude.ai`, `vscode.dev`, `code.visualstudio.com`, `chatgpt.com`, `persistent.oaistatic.com`, `localhost` and `127.0.0.1` (documents fetched 2026-09-16). ChatGPT's document declares a confidential client (`private_key_jwt` with `https://chatgpt.com/oauth/jwks.json`); Codex and Claude Code are public loopback clients. Claude's hosted-connector CIMD document URL is not published; the `claude.ai` domain entry covers it and it is verified at staging. Keycloak 26.7.3 advertises `none` in `token_endpoint_auth_methods_supported` and `offline_access` in `scopes_supported`, which Claude and ChatGPT require for CIMD and refresh tokens.
- **Decided — clients without CIMD** (OpenCode, Cline, Continue, LibreChat and others): unsupported until they ship CIMD; dynamic client registration stays disabled. Goose and Zed publish CIMD documents but are not on the initial list.
- **Decided — remote connector testing:** Claude web/Desktop and ChatGPT cannot reach a localhost fixture; they are configured but verified only after an explicitly approved staging deployment (M8). Local clients are tested against the fixture.
- **Decided — `list_roles` content:** roles, requirements and the console's talent-index counts (per-requirement meeting count, coverage and the all-requirements ceiling). In MCP results, counts from 1 to 4 are returned as suppressed (cohort `MIN_CELL` = 5), and coverage and ceiling values derived from a suppressed count are suppressed with it. The console is unchanged.
- **Decided — operator CLI:** `provision mcp status --org` (Keycloak organization, IdP, importer claim, last sync result, outbox backlog), `provision mcp resync --org` (re-queue a full sync) and `provision mcp revoke --org --email` (delete the brokered Keycloak user and sessions), audited like other provisioning actions; no console screen yet.
- **Decided — local Keycloak:** a separate MCP fixture Compose project (Keycloak 26.7.3, its own Postgres, a stand-in tenant IdP realm, ephemeral ports); Cloud's existing `docker-compose.yml` is unchanged.
- **Decided — sync worker placement:** a background task inside each Cloud server replica, like retention; outbox rows are claimed with `FOR UPDATE SKIP LOCKED` and the reconciler runs under a Postgres advisory lock. Metrics and alerts use the existing `/metrics` and `deploy/alerts.yml`.
- **Decided — sync credential:** a confidential service-account client inside the MCP realm with only the realm-management roles it needs (client credentials); never a master-realm admin.
- **Decided — realm setup:** an idempotent `provision mcp bootstrap` applies the realm configuration (audience scope, realm-wide PKCE policy, CIMD trust policy and allowlist, Organizations, user-profile attribute, sync service account) through the Admin API; the reconciler re-verifies the security-critical parts and `/mcp` refuses to serve when they are missing.
- **Decided — token lifetimes:** access tokens 15 minutes (so `mcp revoke` fully applies within 15 minutes; Cloud still rechecks user status and membership on every request); refresh tokens rotate on use with 8-hour idle and 30-day maximum session lifetime. Verify that CIMD clients receive refresh tokens; the spike's CIMD token responses had none.
- **Build risk:** Keycloak's declarative user profile drops undeclared attributes, so the imported upstream-subject attribute (`oidc-user-attribute-idp-mapper` `claim` → `user.attribute`, emitted by `oidc-usermodel-attribute-mapper`) must be declared or unmanaged attributes allowed; verify in the fixture.
- **Decided — toolchain:** raise Cloud's declared `rust-version` from 1.83.0 to 1.91.0, matching CI and the app (rmcp requires 1.88).
- **Decided — delivery:** the first M4 slice includes the whole foundation: Keycloak fixture and realm bootstrap, `/mcp` resource server and `list_roles`, link-once mapping, outbox sync worker, reconciler, vault writer and provisioning changes.
- **Customer onboarding impact:** Keycloak brokers at its own callback (`/realms/<realm>/broker/<alias>/endpoint`), so each customer must add that redirect URI to their IdP application registration before MCP sign-in works for their organization.
- **Scopes:** create the Release 1 scopes (`roles:read`, …) as realm client scopes before any CIMD client is fetched, and advertise them in `WWW-Authenticate` and `scopes_supported`.
- **CIMD trust policy:** production allows only `https` and an explicit allowlist of assistant client domains; evaluate `cimd-restrict-same-domain` against the loopback redirect URIs desktop clients use.
- **Operations:** Keycloak per region with its own Postgres database (not the Cloud tenant schema); Admin API credentials kept apart from Cloud's restricted database role; Cloud refuses to serve `/mcp` if the realm lacks the PKCE and audience policies.
- **Upgrade risk:** Keycloak 26.8 (milestone due 2026-09-30) migrates domain-to-IdP routing from `kc.org.domain` to organization domains (`MigrateTo26_8_0`) and plans to promote CIMD and resource indicators to preview. Rerun the spike scripts before upgrading.
- **Local tests (M6/M7):** run the pinned container in the isolated fixture Compose project with a stand-in tenant-IdP realm; the spike scripts already cover discovery, CIMD, PKCE, audience, `iss` and tenant routing.

Comparison evidence: oidc-provider 9.12.2 (MIT) passed every check, including a delegated-login authorization code flow with a resource-bound JWT and code-replay rejection. Ory Hydra v26.2.0 advertised no CIMD, no `iss` parameter and no RFC 8707 support.

The Desktop reference plan was rechecked on 2026-09-15; its mtime was 13:56 local. The other checkout's interview changes and Alexandria Cloud's existing `.DS_Store` were left untouched. No background implementation job or completion of later slices is implied by this plan.
