# Alexandria AI Chat and Tutoring — Feature Architecture

Status: Proposed architecture, not an implementation claim. Existing capabilities are identified separately below. The UI mocks use illustrative data and scripted responses.

Updated: 24 October 2026 (revision 3: Phase 1 implemented; section 26 records where the implementation differs from earlier revisions)

## 1. Product objective

Provide one persistent AI conversation experience accessible from every authenticated page in Alexandria. Users bring their own supported provider connection and choose a model. Alexandria owns the conversation, source references, context assembly, tutoring behavior, practice history, and approved learning memory.

The experience supports three modes:

- **Ask:** direct explanations, questions, and exploration.
- **Learn:** guided explanations, worked examples, and checks for understanding.
- **Practice:** questions, learner attempts, progressive hints, feedback, and independent follow-up problems.

Changing pages, closing the panel, or changing models must not discard the conversation or draft. Choosing a different model does not guarantee equivalent capability or tutoring quality.

“Choose any provider/model” means an extensible adapter architecture, supported protocol connections, model discovery where available, and manual model IDs where necessary. It does not mean every consumer subscription, proprietary protocol, or model capability works automatically.

## 2. Architecture decisions

1. Mount chat in the application shell, outside route-specific page lifecycles.
2. Use Vue composable singletons for frontend state, consistent with the app; do not introduce Pinia or Vuex.
3. Put provider requests, secrets, context authorization, and persistence in Rust. The webview does not call providers directly.
4. Create a shared AI runtime used by global chat and existing Studio/lesson-tutor entry points. Avoid parallel connection stores and competing provider implementations.
5. Keep canonical conversation history provider-independent. Provider-specific IDs and request formats belong in adapters and run metadata.
6. Make context explicit, bounded, inspectable, and tied to the exact destination of a request.
7. Store conversations and learning memory in the active profile’s encrypted local database. Do not gossip or synchronize private chat data by default.
8. Separate formative AI practice from credential-producing assessments.
9. Start with text chat and read-only learning context. Add richer media and action integrations through capability-gated extensions.
10. Reuse actual app styles and UI components in production. The mock styling is a review artifact, not a second design system.
11. One set of rules decides what any AI surface may see. In-app chat and the external-assistant (MCP) broker both resolve lessons through `alexandria_studio::learning` (`lesson_target`, `content_kind`, `render_lesson`); no surface keeps a private copy of the "withheld" rules. Chat may be stricter than the broker, never looser.
12. AI assistance is unavailable while an assessment attempt or interview check is active. The gate is `alexandria_studio::gate` and applies to chat, the lesson tutor, Studio runs, practice, and the MCP broker. Route guards and panel visibility are presentation only.
13. Phase 1 ships the compatible chat-completion adapter only. Native adapters are added when a capability requires them, not for parity.

## 3. Existing implementation and reuse

The following observations are from the current repository and should be rechecked before implementation if the code changes.

| Existing component | Observed behavior | Architectural treatment |
| --- | --- | --- |
| `src/layouts/AppLayout.vue` | Owns the topbar, sidebar, route content, global overlays, and mobile navigation. | Mount the global chat host alongside these persistent surfaces. |
| `src/composables/useLocalApi.ts` | Wraps Tauri invocation, attaches the profile-session header, and rejects results from a changed session. | All chat commands use this bridge. |
| `src-tauri/src/profile/scope.rs` | Provides profile admission, leases, and cancellation through `run_until_closed`. | Use these mechanisms for commands and streaming jobs; do not invent a weaker profile check. |
| `crates/alexandria-studio/src/provider.rs` | Supports bounded, non-streaming OpenAI-compatible text completions. Local endpoints are loopback; cloud endpoints require HTTPS. Redirects are disabled and local requests bypass proxies. | Extract compatible behavior into a shared provider layer, then add streaming and additional adapters. |
| `StudioConnection` and `useStudio.ts` | Store a connection name, endpoint, model ID, local/cloud location, text capability, enabled state, and key-presence metadata. | Migrate into a shared connection/catalog model while retaining legacy IDs and compatibility. |
| `studio_secrets` | Stores secret values separately from connection documents in the profile database. | Reuse through a Rust-only secret-store interface. Do not return secret values in connection reads. |
| `src-tauri/src/db/mod.rs` | Uses SQLCipher for persistent profile databases. | Preserve the existing encryption and profile lifecycle; do not create an unencrypted chat database. |
| `studio_ask_tutor` | Checks tutor access/policy, loads a text lesson and history, calls the provider, checks the runtime epoch, and persists the exchange. | Keep its access checks while moving generation and persistence into the shared runtime. |
| `studio_tutor_threads` | Stores JSON message history with a unique course/element association. | Import existing threads without pretending old messages have per-message model attribution that was never recorded. |
| Goals and `LearningPathView` | Expose proven, available, and locked skills, course recommendations, and assessment affordances. | Supply authorized goal context; reuse domain actions for learning and assessment. |
| `crates/alexandria-decisions` and `src-tauri/src/commands/decisions.rs` | A third provider path: its own `reqwest` client, a fixed endpoint, and an API key stored under `studio_secrets('jev')`. Surfaced in the UI as "Optional AI checks". | Inventory it as a shared-runtime client. Migrate its secret into the shared secret store and its HTTP policy into `alexandria-ai`. Do not name the underlying service in UI copy. |
| `crates/alexandria-mcp` and `src-tauri/src/commands/studio_mcp.rs` | A stdio MCP server for external assistants. It reaches the app over a Unix socket through a per-grant token in a 0600 connection file. Grants carry scopes (`learning:read`, `credentials:read`, `drafts:read`, `drafts:propose`); lock, profile switch, revocation, and expiry remove the file. Thirteen read/propose tools withhold assessments and answer keys. | Keep as the inbound external-assistant surface. Share its app-side tool handlers and withholding rules with chat (see sections 16 and 13A). Add the assessment gate. |
| `src/composables/useKeyboardShortcuts.ts` | Central registry with `mod`/`shift`/`alt` combos. | `Cmd/Ctrl+J` is unbound today; register the chat toggle there. |
| `src/utils/markdown.ts` and `src/utils/sanitize.ts` | Markdown rendering through DOMPurify. | Render assistant output through these; do not add a second renderer. |
| `src-tauri/tauri.conf.json` CSP | `img-src 'self' data: blob:`; `connect-src` has no provider origins. | Remote images in model output are already blocked at the platform layer. Provider calls must stay in Rust; do not widen `connect-src`. |
| `src-tauri/src/commands/dev_seeds.rs` | Uses `tauri::ipc::Channel` for progress streaming. | Precedent for the chat streaming channel. |
| `src-tauri/src/commands/assessment.rs` and `src-tauri/src/commands/integrity.rs` | Attempt lifecycle (`assessment_start_attempt`, `assessment_recover`, `assessment_submit_answers`) with cooldown, max-attempt, and window policy; a randomized draw; a severity-mapped flag catalog. Sentinel ships overlay, process, hotkey, display, gaze, liveness, and paste-classifier signals. | Source of the "attempt active" state for the AI gate; destination for new integrity flags. |

The current provider implementation is not already a general native Anthropic/Gemini adapter or a streaming global-chat service. The current lesson-tutor behavior does not already provide the proposed explicit context-review flow. `studio_ask_tutor` already releases the database before the provider call and re-checks the runtime epoch afterwards; the shared runtime must preserve that pattern.

## 4. System structure

```text
Vue application
  AppLayout
    AppTopBar / mobile entry
    Router content
      Lesson / Goals / Skills pages
        -> explicit context offers
    GlobalChatHost
      ChatPanel / full workspace / mobile sheet
      ThreadHistory / Composer / ContextReview / ModelPicker
      PracticeCard / MemoryReview / ArtifactPreview
            |
            | useLocalApi + scoped Tauri IPC channel
            v
Rust command layer
  profile admission + argument validation
            |
            v
ChatService
  ConversationStore ---- RunCoordinator ---- ProviderRegistry
          |                     |                 |
          |               ContextAssembler        +-- OpenAI adapter
          |                     |                 +-- Anthropic adapter
          |               LearningServices        +-- Gemini adapter
          |                     |                 +-- Compatible API adapter
          |               PermissionChecks        +-- Ollama adapter
          |                     |                         |
          v                     v                         v
Encrypted profile DB     Authorized local sources     Provider API
Private attachment store Goals / published lessons    or local runtime
SecretStore              approved memory / practice
```

Studio workflows and existing course-tutor commands become additional clients of the shared provider/runtime layer. Their authoring permissions and course policies remain separate from ordinary learner chat.

No Alexandria-hosted inference proxy is required for the initial BYO-provider design. Requests go from the Rust application directly to the configured endpoint.

## 5. Frontend architecture

### 5.1 Persistent shell

Add `GlobalChatHost.vue` to `AppLayout.vue` outside the routed page slot. It renders one active chat surface rather than mounting independent panels on every page.

Presentation modes:

- Desktop: resizable docked side panel by default.
- Expanded: a full chat workspace backed by the same state.
- Mobile: a sheet or full-screen surface using the same conversation components.
- Minimized: a global Chat entry that restores the active thread and draft.

Floating and bottom-drawer layouts remain optional presentation variants; they do not require separate conversation services.

Use the existing keyboard-shortcut registry to assign a conflict-free toggle. `Cmd/Ctrl+J` in the mocks is illustrative until checked against the app’s shortcuts. Coordinate overlays with dialogs, live-tutoring picture-in-picture, and mobile navigation. A chat panel must not cover a live-call control or trap focus behind another modal.

Chat is available across authenticated app pages. Profile selection, locked screens, and onboarding without an active profile do not expose a previous profile’s conversation.

On assessment and interview-check routes `GlobalChatHost` is unmounted, not hidden. The draft and thread survive in the profile database; nothing chat-related remains in the DOM. This is presentation only: the backend gate in section 13A is what actually refuses assistance while an attempt is active.

### 5.2 State ownership

Proposed composables:

| Composable | Responsibility |
| --- | --- |
| `useChat()` | Active thread, paginated messages, draft, selected mode/model, run status, and send/stop/retry actions. |
| `useChatPanel()` | Open/minimized/expanded state, dimensions, and responsive presentation. |
| `useAiConnections()` | Sanitized connection metadata, catalog entries, connection validation state. |
| `useChatContext()` | Explicit attachments, preview of the next request, context revisions, and destination approval. |
| `useLearningMemory()` | Approved memory, proposed additions, edit/forget actions. |
| `useChatPractice()` | Practice-session display and interaction with the backend’s validated practice state. |

Use module-level refs exposed as `readonly()`, with controlled methods for mutations. Clear profile-owned state on profile lock/switch and reject stale async completions. Keep UI state distinct from the persisted source of truth.

Debounce draft persistence to the encrypted profile store. Do not store API keys, conversation text, or learning memory in browser local storage. Persist panel preferences separately from conversation data.

Streaming render budget: coalesce `text_delta` events per animation frame, re-parse only the trailing Markdown block while streaming, and run one full sanitized parse on `run_completed`. Re-parsing the whole message on every delta is quadratic and must not ship. Chat composables never pass message bodies to the `frontend_log` bridge.

### 5.3 Page integration contract

A page may register an offer such as “Attach this lesson” or “Use this goal.” Registration alone does not attach or transmit anything.

An offer carries an entity reference, source kind, version/revision, and optional selected range. The backend resolves and authorizes the reference. It does not trust a page-supplied claim that the user owns a resource.

Changing routes unregisters the current page’s offers but leaves already accepted attachments and the conversation intact. No whole-DOM scraping or automatic attachment of every page visited.

### 5.4 UI components and styles

Use `AppButton`, `AppInput`, `AppTextarea`, `AppTabs`, `AppModal`, badges, and existing empty/error states. Use `main.css` tokens, the bundled Inter font, and established page-title styling where applicable.

All new copy must use the existing localization system. Support keyboard navigation, visible focus, accessible streaming announcements, reduced motion, and touch targets. Announce response status without reading every token through an aria-live region.

## 6. Backend modules

Proposed module boundaries; exact filenames may change during implementation without changing the ownership model:

```text
crates/alexandria-ai/
  model.rs                 Provider-independent request/response types
  provider/mod.rs          Adapter interface and registry
  provider/compatible.rs   Compatible chat-completion protocol
  provider/openai.rs       Native OpenAI protocol support
  provider/anthropic.rs    Native Anthropic protocol support
  provider/gemini.rs       Native Gemini protocol support
  provider/ollama.rs       Local runtime protocol support
  stream.rs                Normalized stream events and bounded decoding
  errors.rs                Sanitized, typed provider errors

src-tauri/src/ai_chat/
  service.rs               Prepare/send/cancel orchestration
  runtime.rs               Profile-scoped runs and cancellation
  context.rs               Authorized, budgeted request assembly
  learning.rs              Modes, practice, learning summaries
  memory.rs                Proposed/approved memory lifecycle
  sources.rs               Source resolution and stable citation anchors
  artifacts.rs             Notes/flashcards and proposed goal actions
  store.rs                 Profile database queries and transactions
  secrets.rs               Rust-only access to encrypted secret records

src-tauri/src/commands/ai_chat.rs
src-tauri/src/commands/ai_connections.rs
src-tauri/src/domain/ai_chat.rs
src/components/chat/
src/composables/useChat.ts
```

As built in Phase 1, `src-tauri/src/ai_chat/` holds `store.rs` (persistence), `context.rs` (assembly, budgets, digests) and the run registry in `mod.rs`; the commands live in `src-tauri/src/commands/ai_chat.rs`. The assessment gate lives in `crates/alexandria-studio/src/gate.rs` because the broker, which runs in that crate, needs it too.

The shared crate must not depend on Vue, Tauri UI state, or learner database schemas. The host resolves context and obtains secrets, then passes an immutable request to the adapter.

Use `reqwest`/Tokio in Rust, bounded response decoding, cancellation-aware streams, and platform-conditional integration where needed. Do not hold a SQLite mutex across network calls or streamed generation.

Register commands in `lib.rs`, update generated command types through the existing generation tooling, and classify commands correctly in the profile command policy. Keep Rust/TypeScript domain shapes in sync with `snake_case` fields.

## 7. Provider connections and model catalog

### 7.1 Separate connection from model

Phase 1 stores connections as the existing Studio connection documents (`studio_documents` kind `connection`, keys in `studio_secrets`) rather than new `ai_connections` tables. Chat, Studio, and the lesson tutor therefore share one list with no migration. The document gained a `private_network` location and an optional `context_tokens`. A thread stores its own model override, so `(connection_id, model_id)` is still the selection.


A connection represents a configured provider account or endpoint. A model selection is `(connection_id, model_id)`. Multiple accounts for the same provider and multiple models on one connection are supported.

Connection metadata:

- Stable ID, display name, provider adapter kind, normalized endpoint.
- Execution location: on-device, remote, or explicitly configured private-network endpoint.
- Authentication kind and key-presence metadata; never a returned key value.
- Enabled state, configuration revision, last validation result/time.
- Optional account/project settings that the selected adapter explicitly supports.

Model catalog entries:

- Provider model ID and display label.
- Text/image/audio input support, streaming support, tool support, structured-output support.
- Known input/output limits and the source of that information.
- Capability provenance: discovered, adapter-maintained, or user-specified.
- Availability and last refresh time. Unknown values stay unknown.

Do not assume that all compatible endpoints implement every field, discoverable catalog, tokenizer, or tool protocol. Allow a manual model ID and a conservative capability configuration. Distinguish a successful connection test from a guarantee that every model operation will work.

### 7.2 Adapter contract

The adapter exposes:

- `validate_connection`: validate syntax, destination, authentication requirements, and supported options.
- `list_models`: discover a catalog when supported; return an explicit unsupported result otherwise.
- `capabilities`: report known capabilities for the selected model/configuration.
- `generate`: accept a canonical request and produce normalized events.
- Cancellation and error mapping through a common runtime contract.

Canonical requests contain ordered role/content messages, generation settings, source metadata, and explicitly enabled tool schemas. Adapters translate role conventions, authentication, media formats, and stream framing.

Canonical content supports text and attachment references, with media blocks added when supported. Provider reasoning traces, opaque conversation state, and provider-native tool objects are not assumed to transfer between providers. Store only explicitly supported response content; do not depend on hidden reasoning.

### 7.3 Connection UX

1. Choose a supported provider or compatible/custom endpoint.
2. Enter the endpoint/account configuration and secret when required.
3. Test the connection through Rust. A metadata/catalog test sends no learning context. If an inference test is needed, disclose the fixed test prompt and possible provider usage.
4. Discover or enter a model; show capability limitations.
5. Save and choose a default model.

Choosing a model does not itself transmit a conversation. A disabled/deleted connection remains identifiable in historical messages but cannot start new runs.

Consumer chat subscriptions and API access are separate connection types. Do not promise subscription reuse through arbitrary APIs. Installed assistant runtimes, OAuth, or ACP-style agent connections are later extensions requiring their own authentication, lifecycle, platform support, and permission boundary.

### 7.4 Endpoint policy

Preserve current loopback validation for on-device endpoints, HTTPS for ordinary remote endpoints, disabled redirects, and proxy bypass for loopback requests. Reject URL credentials and unexpected query/fragment fields. Bind credentials to the configured origin; never forward them to a redirect destination.

Private-network servers are a distinct, explicitly configured option, not a loophole in the “local” label. A request to an inference server on the LAN leaves the device.

Resolve the private-network hostname at validation time, require every resolved address to be in a private or link-local range, pin the chosen address for the request (connect to the pinned IP and send the configured hostname in the `Host`/SNI fields), and re-resolve only on the next validation. Checking the address once and then letting `reqwest` resolve again at send time is a DNS-rebinding hole.

On a phone, localhost refers to the phone. Reaching a model on a desktop requires an explicit remote/private-network connection. Endpoint location does not prove that the server itself never makes downstream cloud calls.

## 8. Canonical data model and persistence

All learner records below belong to the active, network-bound profile. Use that profile’s SQLCipher database and existing migration infrastructure. Suggested table names are proposals; do not hard-code a migration number before checking the current schema.

| Entity | Important fields and invariants |
| --- | --- |
| `ai_connections` | ID, adapter kind, endpoint, location, auth metadata, enabled state, config revision. Device-specific configuration is not automatically synchronized. |
| `ai_model_catalog` | Connection ID + model ID, capability metadata, provenance, limits, refresh time. |
| `chat_threads` | ID, title, mode, default connection/model, active branch, revision, timestamps, archive state. A thread is not keyed by provider. |
| `chat_messages` | ID, thread ID, parent/branch position, role, ordered content blocks, status, originating run ID, timestamps. Completed content is immutable. |
| `chat_runs` | ID, thread/user-message IDs, client idempotency key, selected model/connection revision, context snapshot ID, state, usage, sanitized error, timing. |
| `chat_context_bindings` | Thread ID, source reference, source version, selected range, enabled state, revision. Explicitly attached sources only. |
| `chat_context_snapshots` | Run/prepare ID, source manifest, model/destination binding, dependency revisions, prompt version, budget decisions, digest. Contains no API keys. Excerpt text is not stored inline: the snapshot references `chat_excerpt_blobs`. |
| `chat_excerpt_blobs` | Content hash, excerpt text, reference count. Content-addressed so a 200 KB lesson attached across twenty sends is stored once. Deleted when the count reaches zero through the same cleanup plan as threads. |
| `chat_run_checkpoints` | Run ID, latest persisted output, last event sequence, terminal status. Supports recovery without storing every token as a separate row. |
| `chat_summaries` | Thread/branch, covered message IDs, summary text, dependency/source lineage, generation method, prompt/model version if generated. |
| `learning_memory` | ID, type, content, scope, approved/proposed status, source, revision, timestamps, optional expiry. |
| `chat_practice_sessions` | Thread, skill references, question/version, attempts, hints used, feedback, validator type, independent-check result. No credential authority. |
| `chat_artifacts` | Draft/saved note or flashcard, source message IDs, source anchors, destination ID, user acceptance state. |
| `chat_drafts` | Thread ID, text, attachment selections, updated time. Local encrypted storage only. |

Use foreign keys and transactions for thread/message/run relationships, unique client request keys for deduplication, and indices for thread pagination and active-run lookup.

The schema is phased with the features. Phase 1 (migration 19, `src-tauri/src/db/ai_chat.sql`) creates `chat_threads`, `chat_messages`, `chat_bindings` (the context bindings), `chat_excerpts` and `chat_snapshot_excerpts` (content-addressed excerpts, collected when no snapshot references them rather than reference-counted), `chat_snapshots`, `chat_runs`, and `chat_drafts`. Checkpoints are written to the streaming assistant message itself; there is no separate checkpoint table. Connection and catalog tables were not needed (section 7.1). Summaries, learning memory, practice sessions, and artifacts arrive with Phase 2. Phase 1 threads are linear: a retry creates a new assistant message version linked to the same user message rather than a branch. Branch position columns are added in Phase 2 if the branch picker ships. Phase 2 also adds an FTS5 index over `chat_messages` for thread search; SQLCipher supports it.

Store profile ID/network ID in runtime identities and secret namespaces even when the database is already profile-specific. Never reuse an active profile’s open database handle after profile teardown.

Attachments that do not fit comfortably in the database use a private, profile-owned encrypted file store. Existing public/P2P blob storage is not automatically suitable for private uploads. Staging, extraction caches, thumbnails, and cleanup must preserve the same privacy boundary.

## 9. Context assembly and disclosure

### 9.1 Context sources

| Context | Contents | Rule |
| --- | --- | --- |
| Application policy | Versioned safety, source handling, and product instructions. | Added by Alexandria, not by retrieved content. |
| Mode | Ask/Learn/Practice behavior and output expectations. | Selected by the user. |
| Conversation | Recent messages plus an eligible summary of older turns. | Included when continuing the selected branch. |
| Attached material | Selected lesson passages, documents, images, or other supported files. | Explicit user attachment and backend authorization required. |
| Selected goal | Target skills, proven prerequisites, available next steps, relevant courses. | Included when attached by the user. |
| Recent practice | Relevant attempts, errors, and hint usage. | Opt-in context binding for the relevant skill/session; visible in review. |
| Learning memory | Approved preferences and learning needs. | Approved memory only, relevant to the current task and selected in context. |

Opening chat on a page is not permission to send that page. Navigation must not silently attach the new page or replace the current attachment.

### 9.2 Assembly pipeline

1. Authenticate the active profile/session and resolve the thread/branch.
2. Resolve accepted context references under current access rights and course policies.
3. Capture source versions, selected ranges, goal/credential summaries, and memory revisions.
4. Retrieve relevant excerpts only from the authorized selected set. Begin with bounded local extraction/search; embeddings are optional later.
5. Assemble application instructions, mode instructions, eligible history, selected context, and the current user message in a documented order.
6. Estimate the context budget for the selected model, reserve output space, and make any required reductions explicit.
7. Produce an immutable preview snapshot and a manifest of included/excluded material.
8. Bind approval to the snapshot, destination, model capabilities, and relevant configuration revisions.
9. Revalidate these dependencies at send time. If they changed materially, return `context_stale` and prepare/review again rather than silently rebuilding a larger request.

Context preview is local preparation, not an undisclosed provider call. Any cloud-assisted summarization or extraction is a separate disclosed operation with its own destination, input set, and usage.

### 9.3 Token and payload budgets

Compute input capacity from the known model limit minus output reserve, prompt overhead, and a safety margin. Use a model tokenizer when reliably available; otherwise use a conservative estimate and enforce byte/media limits as well. Unknown limits require a conservative configured cap.

Proposed Phase 1 defaults, adjustable per connection: estimate tokens as bytes divided by 3 for Latin-script text and bytes divided by 1.5 for CJK and mixed text; cap input at 8,000 estimated tokens when the model publishes no limit; reserve 1,024 tokens for output; apply a 10 percent safety margin; hard-cap any single request body at 1 MB. These are starting values so Phase 1 is not blocked on the open decision in section 24.

Preserve the current user question and applicable application instructions. Reduce older history and low-relevance excerpts before discarding explicit selections. Do not silently drop a required attachment, split a structured interaction into invalid pieces, or send an oversized request repeatedly.

If a request cannot fit, explain which material is too large and offer a narrower excerpt, shorter history, or a fresh conversation. Show omissions and summaries in Review context.

Summaries have source/message lineage and are invalidated when their dependencies change. A summary must not smuggle excluded material into a later request. It is a lossy representation, not an authoritative record of what the learner knows.

### 9.4 Review context

Show the provider/endpoint destination, model, conversation coverage, attachments/excerpts, approved memory, relevant practice, and estimated size. Allow inspection and removal of optional context.

Normal repeat sends can reuse the user’s chosen context scope with visible chips. A full review is required on exactly these events: the first send in a thread, a destination (connection or model) change, a newly added attachment or memory item, a scope expansion such as "search my library", and a `context_stale` result. Every other send shows chips only. Do not require a redundant modal for every unchanged message.

Removing an attachment stops its future attachment. It does not remove text already embedded in old messages or erase what a provider previously received. Explain this and offer a fresh conversation or an explicitly selected history branch.

## 10. Sources, citations, and retrieval

Represent sources with a stable entity ID, content hash/revision, and an anchor such as a section, text range, page, or timestamp. The UI resolves citations through Alexandria routes, not arbitrary model-produced URLs.

Provide the model with bounded source IDs and excerpts. Validate returned citation IDs against the request manifest. Label unsupported citations instead of presenting them as verified. A reference establishes where an excerpt came from; it does not by itself prove that the model’s interpretation is correct.

Retrieval is limited to explicitly attached materials and separately approved collections. “Search my library” is an explicit context expansion. No global search over all private material by default.

Do not call a cloud embedding, transcription, OCR, or reranking service silently. Such a call is another external disclosure. Local deterministic extraction is the initial default. Unsupported file types or image inputs receive a clear capability error rather than silent omission.

## 11. Request lifecycle, streaming, and cancellation

### 11.1 Prepare and send

```text
Draft -> Preparing -> Ready for review/send
                     |
                     v
                 Accepted -> Connecting -> Streaming -> Completed
                     |            |            |
                     +------------+------------+-> Failed / Cancelled / Interrupted
```

1. Frontend prepares a request with a client-generated idempotency key, expected thread revision, and context selections.
2. Backend creates a bounded preview with an expiring prepare ID/digest. No inference has occurred.
3. On Send, backend verifies the prepared request and profile lease.
4. In one transaction, persist the user message, request snapshot, run record, and assistant placeholder; enforce one active run per thread.
5. Start the provider operation under the captured profile lease and a cancellation token.
6. Stream normalized events to the initiating frontend channel. Persist output checkpoints at a bounded cadence rather than a transaction per token.
7. Finalize content, usage, and run status atomically before emitting the terminal completion event.

The connection/model is pinned for the run. Changing the composer’s model affects the next run only.

### 11.2 Event contract

Use a Tauri IPC `Channel` through `useLocalApi()` for request-scoped streaming, following the `dev_seeds` precedent. Avoid broadcasting private message bodies through an unrestricted global event topic.

`useLocalApi.invoke` currently rejects a command result if the profile session changed before completion. A streaming command needs the same check per event: the channel wrapper captures the session token at start and drops every event that arrives after the token changes, not only the final result.

Every event carries `thread_id`, `run_id`, a monotonic `seq`, and a frontend generation/session correlation value. Correlation values do not replace backend profile authorization and must not expose a reusable profile-session secret.

Event kinds:

- `run_started`
- `text_delta`
- `citation_added`
- `artifact_proposed` or a validated practice update
- `usage_updated`, when available
- `run_completed`
- `run_failed`
- `run_cancelled`

The frontend ignores stale-generation and duplicate events and detects sequence gaps. A reconnect fetches the canonical persisted checkpoint/run state rather than assuming the IPC channel offers durable replay.

### 11.3 Concurrency and recovery

- One active generation per thread prevents ambiguous ordering.
- Use a small configurable profile-level concurrency limit; default to two simultaneous runs across different threads.
- Maintain bounded stream buffers. Slow rendering must not permit unlimited memory growth.
- Persist a checkpoint every 500 ms or every 2 KB of new output, whichever comes first, and once on any terminal state. WAL mode is already enabled on the profile database.
- Stop cancels the local stream/request and marks the run appropriately. It cannot guarantee that the provider stopped computation or billing.
- If the app exits or the OS suspends a run, mark it interrupted on recovery. Show saved partial output and let the user retry.
- If provider acceptance is uncertain, do not silently repeat a potentially billable request.
- Reusing the same client idempotency key returns the existing run; it does not create a second user message or provider request.
- Retry/regenerate creates a new run/response branch linked to the original message. Preserve the old response and its model attribution.

On profile lock/switch, close admission, cancel and join owned work using the existing profile lease mechanism, clear webview state, and prevent stale writes/events into the next profile. Do not hold a DB mutex while waiting for cancellation or let an unjoined task retain a profile resource after teardown.

## 12. Proposed IPC surface

All names below are proposed. Commands use the established profile-scoped wrapper and return sanitized failures. Business errors should have a typed payload; unexpected command failures can retain the repository’s `Result<..., String>` convention.

| Command group | Operations |
| --- | --- |
| Connections | `ai_list_connections`, `ai_save_connection`, `ai_test_connection`, `ai_delete_connection`, `ai_list_models` |
| Conversations | `chat_create_thread`, `chat_list_threads`, `chat_get_thread`, `chat_rename_thread`, `chat_archive_thread`, `chat_delete_thread` |
| Draft/context | `chat_save_draft`, `chat_set_context`, `chat_prepare_message`, `chat_get_context_snapshot` |
| Generation | `chat_start_run` with an IPC channel, `chat_cancel_run`, `chat_get_run`, `chat_retry_run` |
| Memory | `chat_list_memory`, `chat_propose_memory`, `chat_approve_memory`, `chat_edit_memory`, `chat_forget_memory` |
| Practice/artifacts | `chat_submit_practice_attempt`, `chat_save_artifact`, `chat_get_artifact` |
| Portability | `chat_export_thread`, `chat_export_memory`, explicit deletion operations |

Representative prepare request:

```json
{
  "thread_id": "thread-id",
  "expected_thread_revision": 12,
  "client_request_id": "client-generated-uuid",
  "connection_id": "connection-id",
  "model_id": "provider-model-id",
  "mode": "learn",
  "message": { "text": "Why is this the next skill?" },
  "context_revision": 4
}
```

Representative prepared result:

```json
{
  "prepare_id": "opaque-id",
  "request_digest": "digest-of-canonical-request",
  "expires_at": "ISO-8601 timestamp",
  "destination": { "connection_id": "connection-id", "location": "remote" },
  "context_items": [],
  "estimated_input_tokens": 1800,
  "estimate_kind": "conservative",
  "omissions": [],
  "requires_review": true
}
```

The start command references the prepared request, matching digest, expected revision, and IPC channel. It does not accept arbitrary credentials or client-crafted system instructions as authorization. The backend verifies all references and constructs the final provider request.

## 13. Tutoring and practice engine

Mode instructions are versioned application policy, applied consistently across adapters. Instructor guidance is included only for an authorized course context and cannot override access, assessment, or privacy rules.

**Ask:** answer the question directly, cite attached material when relevant, and allow follow-up exploration.

**Learn:** explain one concept, show a worked example, invite an attempt, identify the misconception, and offer a next step. Let the learner request a direct explanation; do not force an endless sequence of questions.

**Practice:** use a validated practice object rather than trying to infer all state from chat prose. Each question records the skill, prompt, answer/validator where available, hints, source, and version. A typical loop is:

```text
Question -> Attempt -> Targeted hint or feedback -> Revised attempt
         -> Fresh independent question -> Optional saved learning need
```

Use deterministic validators for supported numeric, fraction, multiple-choice, or structured tasks. Numeric and multiple-choice validation is inline; code and structured tasks reuse the existing WASM grader plugin pipeline with a practice-only grader registration rather than a parallel validator. For open-ended responses, clearly identify feedback as model-assessed and retain uncertainty. Validate model-generated structured objects before rendering or persisting them; malformed output falls back to a readable response without fabricating a score.

Practice questions are model-generated or drawn from a separate practice pool. They are never drawn from a ratified assessment bank, and a practice item that fuzzy-matches a bank item is rejected before it is shown (see section 13A).

Keep independent attempts distinct from attempts made after hints or answer revelation. Track whether a question was new and whether assistance was used. Do not label a student as having mastered a skill from one successful chat answer.

Protected assessment questions, answer keys, and active assessment state must not be exposed through context retrieval. The learner can enter the existing assessment flow through a normal app action; the tutor cannot grade its own practice into a verified credential.

## 13A. Assessment and skill-measurement integrity

Sentinel already detects external help: overlays, cheat-tool processes, global hotkeys, display changes, gaze, liveness, and paste anomalies. An in-app assistant and an external assistant with an MCP grant are new help channels inside the trust boundary, and chat creates new paths for item-bank leakage. The rules below close those paths. They apply to chat, the lesson tutor, practice, and the MCP broker alike.

### Attempt gate

- A live integrity session is the gate: every attempt and every interview check holds one (`integrity_sessions.ended_at IS NULL`). Nothing closes a session left open by a crash, so a session only gates for six hours after it started. While one exists for the profile, `chat_prepare_message`, `chat_start_run`, `chat_retry_run`, `studio_ask_tutor`, every practice command, and every MCP broker tool return a typed `assessment_active` (broker: `unavailable: assessment in progress`) error. The check lives in `alexandria_studio::gate` and is enforced in Rust; the route guard and panel unmount are presentation.
- Starting an attempt cancels every active chat run for the profile. A run streaming in a background thread during an attempt is a help channel.
- Starting an attempt refuses to join, and leaves, any live tutoring call. Plugin iframes and other app windows that could render AI output are closed for the attempt.
- A short post-attempt cooldown (default 15 minutes, per skill) keeps chat and the tutor from attaching material or practice for the skill just attempted. This blocks "submit, ask chat to explain the questions I just saw, retake tomorrow".
- Clear the clipboard at attempt start and end.
- Android assessment activity sets `FLAG_SECURE`; iOS already reports `screen_captured`; macOS remains detection only.

### New integrity flags

Added to the catalog in `integrity.rs` with the given severity:

| Flag | Severity | Meaning |
| --- | --- | --- |
| `in_app_assistant_during_attempt` | Critical | A chat, tutor, or practice command reached the backend while an attempt was open. Should never fire; if it does, the client is tampered. |
| `mcp_call_during_attempt` | Critical | An MCP broker tool call arrived while an attempt was open. |
| `item_text_in_chat` | Critical | Text submitted to chat, a memory proposal, or an artifact fuzzy-matched a ratified bank item. |
| `impossible_speed` | Warning | Correct answers delivered faster than a reading-time floor for the item. |
| `practice_assessment_divergence` | Info | Independent practice checks fail consistently, then an assessment on the same skill passes within hours. |

Raise `ai_assistant_running` from Info to Warning for high-stakes roles on desktop. With an in-app assistant available, an external AI tool running during an attempt is more suspicious, not less.

### Item-bank protection

- The shared lesson rules (`alexandria_studio::learning`) expose only published course elements, never `assessment_items`, banks, answer keys, attempt rows, or interview content. Assessments, interactive elements, plugins, and every quiz type (quizzes are monitored like assessments in the player) are withheld from every AI surface; chat attaches text lessons only. The deny list is test-enforced in the same style as the existing privacy invariants.
- Practice never draws from a ratified bank (section 13).
- Keep a local MinHash/shingle index over ratified bank items on the device. Check chat composer input, memory proposals, artifact text, and practice-generated questions against it. A match is refused and flagged `item_text_in_chat`. This also catches items photographed, transcribed elsewhere, and pasted back.
- Post-attempt review shows skill-level feedback, not item text, unless the bank policy explicitly allows item disclosure.
- Numeric and structured items become parameterized: seeded variables produce a fresh variant per draw, and the grader plugin receives the seed. This removes the value of memorized answers and leaked keys.
- The randomized draw excludes question IDs previously served to the profile; a pass requires at least some items never seen before by that learner. `drawn.question_ids` is already stored per attempt, so this is a filter, not a schema change.
- Each bank carries a small set of transfer items: same skill, different surface form. A pass requires the transfer items as well as the standard draw.
- Track per-item exposure across profiles through Cloud or grader attestations and retire or rotate items over a threshold. The draw becomes exposure-weighted.
- Cooldown escalates on failed attempts that also carried integrity warnings.

### Credential semantics

- Credentials carry an integrity tier claim (for example `proctored: sentinel_full | sentinel_partial | none`) so verifiers can distinguish attempts rather than every pass being presented as equal.
- Sentinel evidence for an attempt records "chat disabled" and "no AI runs during window" as attested facts, and the issuer may require them.

### What not to do

- Do not weaken Learn or Practice modes to protect assessments. Separation is the control.
- Do not attempt to detect AI-written prose in open-ended answers. The false-positive rate penalizes non-native writers; the paste classifier and keystroke timing are better signals.

## 14. Goals and learning-path integration

The Goals UI remains backed by existing goal/path/assessment services. The redesigned overview emphasizes a next available skill, full goal names, proven-skill progress, and a separate learning-path view. The combined path deduplicates shared skills.

Chat may:

- Explain why a skill is available or locked using current prerequisites.
- Suggest a course returned by the existing recommendation/path service.
- Offer practice related to a selected skill.
- Prepare a proposed new goal or change for user review.

Chat may not independently mark a goal complete, alter a credential, create an assessment result, or claim an unavailable prerequisite is proven.

Goal changes use a preview and explicit user acceptance, then call the existing validated goal commands. Re-fetch authoritative goal state afterward. Model prose is never the source of truth for progress.

Current mock examples of goal types, courses, counts, and recommendations are illustrative; implementation must render live domain data and handle missing/unavailable paths honestly.

## 15. Learning memory

Separate three kinds of information:

1. **Preferences:** language, explanation style, accessibility needs the user chooses to save.
2. **Learning observations:** a relevant difficulty or recurring misconception, recorded as an observation with evidence and confidence rather than a verified fact.
3. **Authoritative learning records:** proven skills and assessment results owned by existing domain services, not rewritten by memory generation.

A model can propose a memory item. It does not approve its own proposal. The user can accept, edit, decline, or forget it. Memory is scoped to the active profile, optionally a course/skill/goal, and included only when relevant and enabled.

Editing or forgetting memory invalidates dependent prepared requests and summaries used for future assembly. Previous conversation text may still contain the same fact; explain that distinction and offer a fresh history branch when needed. Local deletion cannot retract a request already received by a provider.

No cross-profile memory, automatic sensitive-trait inference, or silent transfer between network-bound profiles.

## 16. Study artifacts and actions

Supported initial artifacts are draft notes and flashcards derived from a conversation. They retain source references and originating message IDs. Users preview and save them explicitly.

Use an artifact/action registry so later tools have typed inputs and outputs. Read tools operate only within the approved context scope. Write actions require an accepted proposal and invoke the existing domain command with its normal authorization.

The read tools chat needs (search courses, outline a course, read a lesson, skill graph, progress, resolve goal, learning path, list credentials) already exist as app-side handlers behind the MCP broker. Chat's internal tool registry wraps those same handlers without the socket transport. One implementation, two transports; the withholding rules cannot drift between them because both go through `alexandria_studio::learning`.

Do not expose arbitrary SQL, shell execution, unrestricted filesystem access, credential issuance, or the full Studio MCP permission surface to ordinary learner chat. Existing external-assistant access and Studio authoring grants remain separate systems.

### 16A. External assistant (MCP) coexistence

The MCP server is the inbound surface: an external agent calls into Alexandria under a grant. Chat is the outbound surface: Alexandria calls a provider with assembled context. They share handlers and the authorizer but not trust models, and an MCP grant never implies permission for any outbound connector (section 7.3).

Changes to the existing MCP surface:

- Apply the attempt gate (section 13A) in the broker. Alternatively remove the connection file for the attempt and restore it afterwards, which reuses the existing lock/revoke mechanism.
- Grant-time disclosure must state plainly that everything the chosen scopes can read (private skills, progress, enrollments, credential summaries, lesson text) leaves the device to whichever assistant is connected, with no per-request review. Add a `private_skills` opt-in sub-scope so `skill_graph` returns only public skills by default.
- Keep a tool-call log per grant (tool, arguments digest, time, outcome) visible in the app, the MCP counterpart of chat's Review context.
- `verify_presentation` records a nonce and therefore mutates device state. Move it from `credentials:read` to a distinct `credentials:verify` scope so a read grant is read-only.
- `read_lesson` can trigger a peer fetch of any announced course. Add a per-grant budget (fetches per minute and total bytes) and require either `stored_on_device` or an explicit `allow_fetch: true` argument. Unbounded fetching costs disk and network and reveals the learner's interests to peers.
- Wrap returned lesson and reference content in explicit data delimiters and strip anything resembling tool-call syntax. Document in the grant UI that an external agent holding shell or filesystem tools is outside Alexandria's control.
- Confirm grants carry an expiry, are bound to profile ID and network ID, and that the socket applies a request rate limit.
- Cloud-side MCP (OAuth provider, evidence tools, assessment runs, cohorts) is a separate review. Learner chat never receives evidence scopes.

Web search, cloud OCR, voice transcription, and external tools are later integrations with their own destination and data disclosures. They must not activate simply because a provider supports tool calling.

## 17. Security and privacy boundaries

- Secrets enter through a password/private input, cross IPC once to Rust, and are cleared from frontend state after save or dismissal. Connection reads return `has_key`, not the key.
- Use the existing encrypted profile DB behind a `SecretStore` interface. An OS-keychain backend may be added, but it is not a prerequisite or a reason to duplicate credentials into unencrypted storage.
- Exclude credentials and sensitive request bodies from logs, analytics, error strings, exports, and crash breadcrumbs.
- Reauthorize course, goal, document, and profile access during preparation and before sending. Preserve course tutor opt-in and enrollment/access rules.
- Treat lesson text, documents, retrieved passages, and provider output as untrusted content. Delimit them as data, not application instructions.
- Prompt injection defenses cannot rely on a prompt alone: backend permissions and action validation enforce the boundary.
- Render sanitized Markdown through `src/utils/markdown.ts` and `sanitize.ts` (DOMPurify). Do not execute returned HTML, JavaScript, or arbitrary code. Remote images are already blocked by the CSP `img-src 'self' data: blob:`; keep that directive and do not widen `connect-src` for providers.
- Citation IDs and artifact or practice JSON that appear inside attached lesson text or retrieved passages are neutralized before assembly, so injected content cannot forge a `citation_added` or `artifact_proposed` event.
- Minors: roles already include parent/guardian and every profile records a birthdate. A cloud connection sends a child's learning data to a third party. A learner under 18 (the app's existing `is_minor`) cannot send to a remote connection unless a linked guardian approved that specific connection; on-device and private-network connections remain available. Whether guardian oversight includes chat history is a product decision listed in section 24.
- Enforce file size/type limits, bounded extraction, payload limits, timeouts, and stream limits.
- Locking a profile removes its data from visible UI and cancels owned work. Closing only the panel does not lock or stop a run.
- No new P2P gossip topics are needed. Private conversations, secrets, context snapshots, and memory are excluded from public catalog/profile/credential synchronization paths.

## 18. Error and edge-state UX

| Condition | Expected behavior |
| --- | --- |
| No connection | Preserve draft; offer connection setup. |
| Invalid credentials | Identify the connection, retain draft, offer Edit connection; show no raw secret/provider payload. |
| Model unavailable | Preserve history and partial response; offer another model with a new destination/context review if needed. |
| Unsupported attachment/capability | Explain the incompatibility before send; offer removing the attachment or selecting a compatible model. |
| Context too large | Show what needs narrowing and preserve explicit selections; do not silently send an unrelated subset. |
| Rate limit | Show retry timing when supplied; do not loop automatically. |
| Offline / local runtime stopped | Distinguish network failure from an unavailable local server. Never silently fall back to cloud. |
| Interrupted stream | Retain marked partial output and offer explicit retry. |
| Profile changed | Cancel/ignore stale work and clear the previous profile’s state. |
| Source deleted or access revoked | Mark the source unavailable and require revised context; do not substitute another document. |
| Provider reports no usage | Display usage/cost as unavailable rather than inventing a number. |
| Stale thread revision | Refresh the thread and resolve the conflict before sending. |
| App restarts mid-run | Recover the saved checkpoint as interrupted; do not claim the request completed. |

Connection checks can use bounded retries for safe metadata operations. Inference retries require explicit handling because an ambiguous network failure may still have consumed provider usage.

## 19. Storage retention, export, and deletion

Default: local history retained in the active encrypted profile until the user deletes it. Archive hides a thread without deleting it. Approved memory has a separate lifecycle from its originating conversation.

Deleting a thread removes its messages, drafts, snapshots, summaries, and owned attachment copies/checkpoints through a transactional cleanup plan. Saved artifacts and approved memory are listed separately so the user can choose whether to remove them too. References shared with other threads are reference-counted and are not prematurely deleted.

Deleting a connection removes its usable credential/configuration while preserving sanitized historical attribution. Profile deletion includes private chat files and all profile-owned secret records.

Export produces a user-requested Markdown/JSON representation with source labels and model attribution. Exclude keys, session tokens, and hidden provider internals. Make inclusion of memory and private attachments explicit.

A normal local delete does not promise secure forensic erasure from backups, previously exported files, or a provider’s systems. Reuse the application’s profile/database backup and deletion policy rather than adding contradictory guarantees.

Cross-device chat sync is deferred. If added, it needs a separate end-to-end encrypted design, explicit consent, conflict handling, and independent secret provisioning. Public P2P content transport is not that design.

## 20. Migration and backward compatibility

1. Extract provider request/validation behavior into `alexandria-ai`, with compatibility tests for existing Studio callers and for the decisions crate's client.
2. Add shared connection/catalog tables and a Rust secret-store interface. Migrate each old connection ID with its configured model as the default selection; do not merge connections solely because endpoints match. Migrate the decisions key from `studio_secrets('jev')` into the shared secret store under a connection of kind `ai_checks`; its UI label stays "AI provider API key".
3. Retain a compatibility facade for Studio command/types while its UI moves to shared connection management. Avoid double writes to independent stores.
4. Import legacy lesson threads into canonical chat threads using a deterministic migration mapping. Preserve course/element association, message order, timestamps, and tutor policy.
5. Old messages lack reliable per-message provider provenance. Mark legacy attribution as unknown or as thread-level metadata; do not invent a model history.
6. Require the new context review before the next request from an imported thread. Importing data is not consent to send it to a newly selected provider.
7. Route legacy tutor commands through the shared runtime while preserving course authorization. Keep the old command contract until callers are migrated.
8. Make migrations transactional and idempotent with explicit schema-version handling. Keep rollback/recovery compatible with the app’s existing database epoch rules; do not promise old binaries can open a newer schema.

## 21. Testing and observability

### Required tests

- Adapter contract tests with recorded/synthetic fixtures: normal response, partial chunks, Unicode boundaries, malformed frames, truncation, timeouts, cancellation, missing usage, and sanitized errors.
- Context tests: explicit attachment only, access revocation, goal/practice scoping, budgets, summary lineage, capability checks, stable citations, snapshot invalidation, and no cloud work during local preparation.
- Profile isolation tests: switch/lock during prepare, secret lookup, request connect, streaming, checkpoint, and final persistence. No stale events or cross-profile writes.
- Persistence tests: idempotent send, concurrent commands, one active run per thread, crash recovery, retry branches, legacy migration, export, and deletion cleanup.
- Frontend/composable tests: draft and active-thread survival across navigation, minimize/resume, model attribution, context disclosure, and state reset on lock.
- UI checks: desktop/mobile, light/dark, keyboard/focus, long names, RTL/localization, empty/error states, sanitized rendering, and interaction with existing overlays.
- Learning checks: hints recorded correctly, independent attempts distinguished, deterministic validators tested, and no practice-to-credential mutation.
- Integrity gate tests: every chat, tutor, practice, and MCP broker entry point returns `assessment_active` while an attempt or interview check is open; starting an attempt cancels active runs; the post-attempt cooldown holds; the `ai_access` deny list rejects every assessment entity kind for every surface; a bank item pasted into chat is refused and flagged.
- Authorizer parity tests: for a fixed fixture, the MCP `read_lesson` result and the chat context excerpt for the same lesson withhold identical material.

Use fake providers for deterministic automated tests. Live provider tests are opt-in, use dedicated test credentials, disclose usage, and never use real learner data.

Run repository-required checks for implementation changes: `vue-tsc -b --noEmit`, relevant frontend tests, `cargo fmt --check`, `cargo clippy -- -D warnings`, and relevant Rust tests. Also run command-generation/policy and localization checks when their surfaces change.

### Operational signals

Record local, content-free metadata: time to first token, completion latency, stream failure/cancel rates, context estimate versus reported usage where available, and persistence failures. Do not log prompts or responses by default.

Evaluate tutoring quality per model/mode using factual correctness, source support, useful hints, and performance on a fresh independent problem. Engagement and chat length alone do not demonstrate learning. Any collection of identifiable learner evaluation data requires an explicit product decision and consent flow.

## 22. Implementation sequence

### Phase 1 — Shared foundation and persistent chat

- Shared connection/secret/provider interfaces and legacy compatibility (Studio and decisions clients).
- Compatible chat-completion adapter only, with streaming and cancellation. Gemini and Ollama are reached through their OpenAI-compatible endpoints; native adapters wait for a capability that needs them.
- Profile-scoped thread/message/run persistence and recovery using the Phase 1 schema subset (section 8), linear threads, retry as message version.
- Shared lesson rules and the `alexandria_studio::gate` attempt gate wired into chat, the lesson tutor, Studio runs, and the MCP broker. Recording the new integrity flags moved to Phase 2: they belong in Sentinel's committed snapshot chain.
- Global panel, full workspace, mobile surface, draft/history, model picker.
- Explicit text/lesson attachments, local preview, immutable request snapshots with content-addressed excerpts.
- Ask mode, citation anchors, error states, and source/style fidelity.

Exit: a user can connect a supported endpoint, chat from multiple pages, stop/retry safely, restart the app, and switch profiles without leakage. No AI surface answers while an assessment attempt is open.

### Phase 2 — Provider breadth and learning behavior

- Native Anthropic adapter; model discovery and manual model handling. Other native adapters only when a required capability is not available through a compatible endpoint.
- Learn and Practice modes with validated practice objects, practice-pool separation, bank-item fuzzy matching, and parameterized items.
- Thread search (FTS5) and, if the branch picker ships, branch columns.
- MCP coexistence changes from section 16A: scope split, grant disclosure, tool-call log, fetch budget.
- Goal/path context and existing course/assessment navigation.
- Reviewed memory proposals and notes/flashcards.
- Lesson-tutor migration and common runtime integration completed.

Exit: a user can switch supported providers without losing canonical history, inspect every context category, practice with recorded hint usage, and save learning memory/artifacts explicitly.

### Phase 3 — Capability-gated extensions

- Private file/image ingestion, extraction, and media-capable adapters.
- Voice and explicit external search/tool integrations.
- Broader source retrieval, optional local indexing, and evaluated summarization.
- Installed-assistant/runtime connectors on supported desktop platforms.
- Cross-device synchronization only after its separate privacy/conflict design is approved.

Each extension must preserve the same authorization, disclosure, profile isolation, and persistence contracts.

## 23. Acceptance criteria

1. The same conversation and draft survive navigation and panel presentation changes.
2. A user can identify the destination and inspect the context for the next send.
3. Opening a page, choosing a model, or reviewing context does not silently send learner content.
4. Unsupported media and oversized requests fail clearly before inference.
5. Provider switches retain history and original attribution while disclosing the new destination.
6. Streaming is cancellable, bounded, recoverable, and isolated to the active profile.
7. Duplicate submission does not create duplicate messages or automatic billable retries.
8. Existing Studio and lesson-tutor access rules survive migration.
9. Chat practice cannot grant, revoke, or impersonate verified credentials.
10. Memory is proposed, approved, editable, and removable; summaries respect its lineage.
11. Secrets and private chats are absent from logs, public P2P traffic, and ordinary connection metadata.
12. Production UI uses the existing Vue components, design tokens, fonts, localization, and accessibility conventions.
13. While an assessment attempt or interview check is open, no chat, tutor, practice, or MCP tool call succeeds, and any that reaches the backend is flagged.
14. No AI surface can retrieve a ratified assessment item, answer key, or attempt row, and the chat and MCP surfaces withhold identical material for the same source.
15. UI copy never names the underlying provider behind "Optional AI checks".

## 24. Decisions to resolve before the relevant phase

These are implementation choices still open; they must not become accidental behavior:

- Which native adapters beyond Anthropic are ever needed, and the capability catalog maintenance process.
- Whether the Phase 1 budget defaults in section 9.3 stand or are replaced before release.
- The age threshold for guardian-approved cloud connections, and whether guardian oversight includes chat history, approved memory, or only connection management.
- The post-attempt cooldown length, the item-exposure retirement threshold, and which banks get transfer items first.
- The integrity tier claim's exact vocabulary in the credential.
- Whether richer file extraction stays entirely on-device and which formats ship first.
- Whether any existing device-backup mechanism includes chat history and how secrets are excluded or re-provisioned.
- Scope and controls for later cross-device sync.
- Exact coexistence rules between general Ask mode, instructor-authored tutor policies, and assessment restrictions, preserving current access rules as the baseline.
- Which learner observations are useful enough to propose as memory and how they expire.
- Whether model-assisted summarization ships initially; it remains off unless its separate disclosure and lineage handling are implemented.

## 25. Repository references and design artifacts

Paths below are relative to the repository root.

- `src/layouts/AppLayout.vue`
- `src/components/layout/AppTopBar.vue`
- `src/components/layout/AppSidebar.vue`
- `src/assets/css/main.css` and `src/assets/fonts.css`
- `src/components/ui/`
- `src/composables/useLocalApi.ts`, `profileSession.ts`, `useProfiles.ts`, `useStudio.ts`
- `src/composables/profile-command-policy.json`
- `src/pages/instructor/AiSettings.vue`
- `src/pages/goals/Index.vue`, `src/components/skills/LearningPathView.vue`
- `src-tauri/src/commands/studio.rs`
- `src-tauri/src/profile/scope.rs`
- `src-tauri/src/db/mod.rs` and `src-tauri/src/db/schema.rs`
- `crates/alexandria-studio/src/provider.rs`, `model.rs`, `store.rs`, `schema.sql`
- `src-tauri/src/assessment/goal_plan.rs`
- `src-tauri/src/commands/assessment.rs`, `src-tauri/src/commands/integrity.rs`, `src-tauri/src/assessment/randomizer.rs`
- `src-tauri/src/sentinel/` (signal sources) and `docs/sentinel.md`
- `src-tauri/src/commands/decisions.rs`, `crates/alexandria-decisions/src/lib.rs`
- `crates/alexandria-mcp/src/{main.rs,broker.rs,stdio.rs}`, `src-tauri/src/commands/studio_mcp.rs`, `docs/instructor-studio-mcp-plan.md`
- `src/utils/markdown.ts`, `src/utils/sanitize.ts`, `src/composables/useKeyboardShortcuts.ts`
- `src-tauri/tauri.conf.json` (CSP)

Interactive design mocks are maintained outside this repository.

This document replaces the earlier context-only specification. Implementation proceeds in the phases of section 22.

## 26. Implementation record

Phase 1 is implemented. Where it differs from earlier revisions of this document:

- **Connections** reuse Studio's connection documents and secrets instead of new tables (section 7.1). `private_network` and `context_tokens` were added to the document.
- **Authorizer**: there is no separate `ai_access` module. `alexandria_studio::learning` is the single set of lesson rules for chat and the broker, and chat is stricter (text lessons only).
- **Gate**: `alexandria_studio::gate`, keyed on a live integrity session started within six hours, because a crashed session is never closed.
- **Quizzes** are withheld from every AI surface, including the external-assistant tools, because the course player monitors them like assessments.
- **Persistence**: excerpts are collected by query when unreferenced rather than reference-counted; checkpoints are written to the streaming message.
- **Phase 1 UI choices** (additive to the existing app): a topbar button with a ⌘J shortcut and no sidebar entry; review as a bottom sheet inside the panel; a floating button and a sheet on narrow windows; Learn and Practice shown as coming soon. A later visual pass made the panel calmer: three header controls (the title opens history; history, saved notes, learning memory, and full width sit in a More menu), the Ask / Learn / Practice mode and the model as pills in the message box, attachments behind a "+" (and as chips above the box on desktop), message actions shown on hover except for the latest answer and on touch screens, suggestions as single compact rows, nothing under 12 px, and connecting a model as one decision per screen (sign in, or set one up with an API key or a model the user runs, with rarely needed fields under Advanced). The expanded view keeps the conversation in a 720 px column; phones get a near full-height sheet with a grab handle.
- **Review before sending is opt-in** (deviation from section 9's review events): learners found a review on every new chat tiring, so chat now sends without the review sheet unless the learner turns on "Review before sending" (setting `chat.review_before_send`, off by default; in the chat's More menu and Settings → Advanced). The destination stays visible in the model menu, attachments in the message box, and a request too large for the model always opens review so the learner chooses what to trim. Every send still goes through prepare, so what is sent is exactly the prepared request, and stale context is still refused.
- **Minors** are refused remote connections unless a linked guardian approved the connection at its current endpoint (section 17). The learner asks from the chat; the request travels sealed over the existing guardian link on the next sync, and the guardian approves or declines it on the child's overview page. The decision is bound to the connection and endpoint, so changing the endpoint needs a new approval. Guardians see the connection's name, endpoint, and model, never chat content.
- **Integrity flags**: `in_app_assistant_during_attempt` (chat or tutor) and `mcp_call_during_attempt` (external assistant) are recorded by the backend when it refuses a request during an attempt, written into the session's committed snapshot chain once per session at the session's current score. Studio run steps are refused but not flagged, since a run may have started before the attempt. `item_text_in_chat`, `impossible_speed`, and `practice_assessment_divergence` are not recorded: outside an attempt there is no live session to attach them to, and the last two belong to grading.
- **Learn and Practice** (Phase 2): per-mode instructions (policy version 3). Practice questions arrive as a fenced JSON block, are validated and stored, and are checked locally (numeric with tolerance, multiple choice, short text); hints and a shown answer mark later attempts assisted. Messages and generated questions that resemble a stored assessment item are refused by a shingle-containment check over item prompts and options.
- **Learning memory** (Phase 2): model suggestions arrive as a fenced block and wait for the learner; saved, active items are sent as data, listed in review, and bound to their revision.
- **Native Anthropic adapter** (Phase 2): connections declare a protocol, `compatible` (the default, and what older documents read as) or `anthropic`. The Messages adapter uses API version `2026-10-21`, so sampling parameters are never sent. History is replayed as text only, without thinking blocks, which the protocol accepts. Because current models think by default and thinking counts against `max_tokens`, the adapter adds a fixed 8,192-token allowance to the requested output; the visible answer stays bounded by the output byte limit. Server-side refusal fallbacks are not requested. The chat connection form offers the choice; Studio's own connection editor keeps whatever a document already declares.
- **Thread search** (Phase 2, migration 23): an FTS5 table over complete messages, kept current by triggers and keyed by message id rather than the implicit rowid, which a VACUUM may renumber. Streaming checkpoints never touch it. Every search term is a quoted prefix phrase, so query syntax is literal text; thread titles are matched too. Results show the best-matching message per thread with a highlighted snippet, and opening one scrolls to the message. Search runs on the device only.
- **Goal context** (Phase 2, migration 24): the Goals page offers each goal; attaching one sends nothing. Each prepare re-reads the goal from the learner's saved goals and recomputes its path with the Goals page's own service, so the text lists proven, available, and locked skills (with suggested courses and missing prerequisites) as they are at that moment; progress comes only from credentials. A removed goal makes the attachment unavailable rather than sending stale text.
- **Notes and flashcards** (Phase 2, migration 25, policy version 4): when asked, a model may end its reply with a fenced `artifact` block (a note, or up to 20 flashcards); it is stored as a proposal under the answer and saved only when the learner saves it. Any finished answer can also be saved as a note. Artifact text is checked against stored assessment items like other model output. Saved artifacts keep the originating answer's sources and outlive the chat; proposals are removed with it. A saved-notes view in the panel lists them and offers a one-card-at-a-time study mode. A saved note that cites a lesson can be added to that course's notes (migration 26) when the learner is enrolled; copying it twice keeps one. Course notes now hold their own text, so a note syncs whole between the learner's devices, and the course player lists them in its sidebar and chapter sheet, the current lesson's first. They are hidden, and refused by the backend, while an assessment is in progress. Course-note sync sends full snapshots without deletions, so a delete keeps the row as a tombstone with its text cleared and a fresh timestamp, which wins on the other devices.
- **Goal proposals** (Phase 2, policy version 5): when the learner asks for help choosing a goal, a model may propose one as an `artifact` block with a name and a short description. The proposal holds no skill ids, and an id the model writes is ignored. The learner finds matching skills with the Goals page's own resolver, picks which to keep, and allows it (see Chat actions below). A goal proposal is never a saved artifact and goes with its chat. With a goal attached, a model may instead propose a change to it (policy version 6): a new name, skills to add, skills to remove. It names skills, never ids; the app keeps only removals that are on the goal and additions that match an existing skill by exact name, drops the rest, and drops a change that would do nothing or leave the goal empty. The learner sees the change against the goal as it is at that moment and applies it with Allow (see Chat actions below). Removing a whole goal stays on the Goals page.
- **Chat actions** (migration 27, policy version 7): everything the chat can write inside Alexandria is a proposal the learner allows or declines, and only the backend writes it. This replaces the separate save, add, and apply flows described above. The model is told it cannot change anything itself and must never say something was saved or changed. Six kinds exist: a note, flashcards, a new goal, a change to an attached goal, a note on the attached lesson (`course_note`), and enrolling in a course (`enroll`). A course note needs a lesson attached to the chat. An enroll proposal is kept only when the course is one the attached goal's path recommends for a skill not yet proven, and only when the learner is not already taking it. Learning paths are computed from goals and credentials and have no stored form, so path changes are goal skill changes plus enrolling; the chat cannot issue or alter credentials. Each proposal is shown as one card headed "The chat is asking to", with Allow and Don't allow. A new goal's card lists the skills the Goals resolver matched and lets the learner search the skill catalog. Because the catalog is only the bundled taxonomy, the instructions (policy version 8) list its subjects by field, and the model says plainly when a goal falls outside them instead of proposing one that would match nothing. Allow is disabled until at least one skill is picked, up to 50. `chat_allow_action` re-checks everything inside one transaction before writing: no assessment in progress, the proposal is still pending, its text does not resemble a stored assessment item, and its target still exists. Goals are written to the synced `learner.targets` setting through the settings store, which notifies the Goals page. Enrolling goes through the course service. A course that needs plugins the learner has not installed is sent to the course page, which asks before installing anything. A declined proposal is recorded as `declined`, writes nothing, can no longer be allowed, and stays in its chat until the chat is deleted.
- **Optional AI checks** (Phase 2): `alexandria-decisions` stays MIT/Apache because `alexandria-cloud` vendors it, so it cannot depend on the AGPL `alexandria-ai` crate. Instead it accepts a host-built HTTP client, and the app builds that client from the shared endpoint policy: the provider must resolve only to public addresses, requests are pinned to them, and redirects are not followed. The pinned client is rebuilt every ten minutes and whenever the profile or settings change. Its key already lives in the shared secret table (`studio_secrets`), so no key migration was needed; its protocol is not a chat protocol and keeps its own request types.
- **Provider terms** (Phase 2): every AI request uses the user's own developer API key, billed to the key's owner, and the key never leaves the device except to that provider. Vendor terms forbid third-party apps from using consumer-plan sign-ins (Anthropic: third parties "may not collect, store, or intermediate Claude.ai credentials or session tokens"), so `alexandria_ai::terms` refuses consumer-app backends as endpoints and refuses Claude subscription tokens and ChatGPT sign-in tokens as keys, both when a connection is saved and before every request. Age rules are shared by chat, the lesson tutor, and Studio runs: learners under 13 use models on their device or own network only, because providers expect zero data retention or COPPA-grade handling for children's data and Alexandria cannot verify that on a user's key; learners aged 13 to 17 need a guardian's approval per remote endpoint and can never use vendors whose terms require users to be 18 or older (Google's Gemini API, OpenRouter, Groq, xAI; Mistral sets no floor but requires parental consent, which guardian approval supplies); adults connecting the Gemini API see that its terms also restrict apps minors are likely to use. Alexandria sends no automatic retries and surfaces provider rate limits as they arrive. Optional AI checks call one fixed service (`api.typesafe.ai`) under its own terms and are not covered by these lists.
- **Provider sign-in** (Phase 2): on desktop, a connection can be made by signing in with a provider account in the system browser instead of pasting a key, for the two providers whose terms offer it to third-party apps. *ChatGPT* follows OpenAI's open-source "Sign in with ChatGPT" plan-usage flow: dynamic client registration with a stable per-device host identifier, authorization code with PKCE and a loopback redirect on `127.0.0.1`, an ID token verified against OpenAI's JWKS (issuer, audience, expiry, nonce), and plan use accepted only with the `chatgpt.tokens.use.direct` grant. Requests then go to `POST /v1/responses` (the only route the plan allows) through a new `responses` adapter, whose event lines may be as long as the largest answer because `response.completed` repeats it; the hour-long access token is refreshed before sending under a process-wide lock, because the refresh token rotates; removing the connection revokes it. The UI follows OpenAI's guidelines: a "Continue with ChatGPT" button, a one-time welcome, "Using ChatGPT plan" with "Manage usage" by the composer, and a usage-limit card. *OpenRouter*'s PKCE sign-in issues an API key on the user's own account, used like any key; it is adults-only. On phones, where a loopback listener cannot stay alive behind the browser, OpenRouter uses its documented headless mode: no callback, OpenRouter shows a code, and the user pastes it into Alexandria, which exchanges it with the PKCE verifier it kept (single use, ten minutes). ChatGPT accepts only a loopback return, so it stays desktop-only. OpenAI's plan model list is large (about 570 KB for nine models), so model discovery on that route reads up to 4 MB, and each failed sign-in step is logged without tokens. Anthropic does not permit third-party Claude sign-in, so Claude connections stay API-key only. Minor profiles follow the key rules: never under 13, guardian approval for 13 to 17, and OpenRouter never. Tokens live in the profile's encrypted database, never in logs, URLs, or the webview. The browser buttons are text-only until OpenAI's approved logo assets are added.
- **External assistants** (Phase 2): `learning:private_skills` and `credentials:verify` scopes, a 20-per-hour peer-fetch budget, and a per-grant activity log shown in Settings. Returned lesson content is not re-wrapped in delimiters: MCP returns it as a structured field, and the existing acceptance test shows forged frames inside it stay inert.

