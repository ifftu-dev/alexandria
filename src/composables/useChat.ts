import { computed, readonly, ref, shallowRef } from 'vue'
import { Channel } from '@tauri-apps/api/core'
import { useLocalApi } from './useLocalApi'
import { getProfileSessionToken } from './profileSession'
import { onProfileLocked } from './useProfiles'
import type {
  ChatBinding,
  ChatEvent,
  ChatOffer,
  ChatPracticeView,
  ChatMemoryItem,
  ChatArtifact,
  ChatGuardianApproval,
  ChatPreparedMessage,
  ChatStartedRun,
  ChatStatus,
  ChatThread,
  ChatSearchHit,
  ChatThreadDetail,
  StudioConnection,
  StudioDocument,
} from '@/types'

/**
 * Global chat state. One instance for the whole app, so the conversation,
 * the draft, and a response that is still writing all survive navigation and
 * closing the panel. Everything here belongs to the active profile and is
 * cleared when it locks or switches; nothing is kept in localStorage.
 */

const NEW_SCOPE = 'new'

const panelOpen = ref(false)
const expanded = ref(false)
const showHistory = ref(false)
const threads = ref<ChatThread[]>([])
const detail = shallowRef<ChatThreadDetail | null>(null)
const draft = ref('')
const connections = ref<StudioDocument<StudioConnection>[]>([])
const status = ref<ChatStatus>({ assessment_active: false })
const prepared = shallowRef<ChatPreparedMessage | null>(null)
const preparing = ref(false)
const error = ref<string | null>(null)
/** Live text of answers being written, by assistant message id. */
const liveText = ref<Record<string, string>>({})
const activeRunId = ref<string | null>(null)
/** Connection and model chosen before the first message creates a thread. */
const pendingConnectionId = ref<string | null>(null)
const pendingModelId = ref<string | null>(null)
/** Mode chosen before the first message creates a thread. */
const pendingMode = ref<'ask' | 'learn' | 'practice'>('ask')
const excludedBindings = ref<string[]>([])
const excludedMemory = ref<string[]>([])
const showMemory = ref(false)
const memoryItems = ref<ChatMemoryItem[]>([])
const showSaved = ref(false)
const savedArtifacts = ref<ChatArtifact[]>([])
const approvals = ref<ChatGuardianApproval[]>([])
const includeHistory = ref(true)
const loaded = ref(false)

/** Pending review: what to send once the learner confirms. */
let pendingText: string | null = null
let pendingRegenerate: string | null = null
/** The pending message came from the composer, so sending clears the draft. */
let pendingFromDraft = false
/** Bumped on lock/switch so late results from the old profile are dropped. */
let generation = 0
let draftTimer: ReturnType<typeof setTimeout> | null = null
/** Assistant message each live run is writing, so deltas land on the right
 * message even while another chat is open. */
const runMessages = new Map<string, string>()

function reset() {
  generation++
  panelOpen.value = false
  expanded.value = false
  showHistory.value = false
  threads.value = []
  detail.value = null
  draft.value = ''
  connections.value = []
  status.value = { assessment_active: false }
  prepared.value = null
  preparing.value = false
  error.value = null
  liveText.value = {}
  activeRunId.value = null
  pendingConnectionId.value = null
  pendingModelId.value = null
  pendingMode.value = 'ask'
  excludedBindings.value = []
  excludedMemory.value = []
  showMemory.value = false
  memoryItems.value = []
  showSaved.value = false
  savedArtifacts.value = []
  approvals.value = []
  includeHistory.value = true
  loaded.value = false
  pendingText = null
  pendingRegenerate = null
  runMessages.clear()
  if (draftTimer) clearTimeout(draftTimer)
  draftTimer = null
}

onProfileLocked(reset)

/** The error code before the first colon of a backend message. */
export function chatErrorCode(value: unknown): string {
  const text = String(value instanceof Error ? value.message : value)
  const code = text.split(':')[0]?.trim() ?? ''
  return /^[a-z_]+$/.test(code) ? code : 'generic'
}

export function useChat() {
  const { invoke } = useLocalApi()

  const usableConnections = computed(() =>
    connections.value.filter(
      (c) => c.value.enabled && c.value.capability === 'text' && c.value.location !== 'external',
    ),
  )
  const thread = computed(() => detail.value?.thread ?? null)
  const selectedConnectionId = computed(
    () => thread.value?.connection_id ?? pendingConnectionId.value ?? usableConnections.value[0]?.id ?? null,
  )
  const selectedConnection = computed(
    () => usableConnections.value.find((c) => c.id === selectedConnectionId.value) ?? null,
  )
  const selectedModel = computed(
    () => thread.value?.model_id ?? pendingModelId.value ?? selectedConnection.value?.value.model ?? null,
  )
  const draftScope = computed(() => thread.value?.id ?? NEW_SCOPE)
  const mode = computed(() => thread.value?.mode ?? pendingMode.value)
  const streaming = computed(() => activeRunId.value !== null)

  function guard<T>(run: () => Promise<T>): Promise<T | undefined> {
    const started = generation
    return run().then(
      (value) => (started === generation ? value : undefined),
      (e) => {
        if (started === generation) throw e
        return undefined
      },
    )
  }

  async function refreshStatus() {
    const next = await guard(() => invoke<ChatStatus>('chat_status'))
    if (next) status.value = next
    return status.value
  }

  async function refreshConnections() {
    const next = await guard(() => invoke<StudioDocument<StudioConnection>[]>('studio_list_connections'))
    if (next) connections.value = next
  }

  async function refreshThreads() {
    const next = await guard(() => invoke<ChatThread[]>('chat_list_threads'))
    if (next) threads.value = next
  }

  async function ensureLoaded() {
    if (loaded.value) return
    loaded.value = true
    try {
      await Promise.all([refreshStatus(), refreshConnections(), refreshThreads()])
      if (!detail.value) await loadDraft()
    } catch (e) {
      loaded.value = false
      error.value = chatErrorCode(e)
    }
  }

  async function loadDraft() {
    const scope = draftScope.value
    const text = await guard(() => invoke<string>('chat_get_draft', { scope }))
    if (text !== undefined && scope === draftScope.value) draft.value = text
  }

  function setDraft(text: string) {
    draft.value = text
    if (draftTimer) clearTimeout(draftTimer)
    const scope = draftScope.value
    const started = generation
    draftTimer = setTimeout(() => {
      if (started !== generation) return
      void invoke('chat_save_draft', { scope, text }).catch(() => {})
    }, 400)
  }

  async function reloadThread(id: string) {
    const next = await guard(() => invoke<ChatThreadDetail>('chat_get_thread', { threadId: id }))
    if (!next) return
    detail.value = next
    activeRunId.value = next.active_run_id
    const live = { ...liveText.value }
    for (const m of next.messages) if (m.status !== 'streaming') delete live[m.id]
    liveText.value = live
  }

  async function openThread(id: string) {
    error.value = null
    prepared.value = null
    excludedBindings.value = []
    showHistory.value = false
    await reloadThread(id)
    await loadDraft()
  }

  /** Searches this device's chats. Nothing leaves the device. */
  async function searchThreads(query: string): Promise<ChatSearchHit[]> {
    if (!query.trim()) return []
    return (await guard(() => invoke<ChatSearchHit[]>('chat_search_threads', { query }))) ?? []
  }

  async function newChat() {
    error.value = null
    prepared.value = null
    detail.value = null
    activeRunId.value = null
    excludedBindings.value = []
    showHistory.value = false
    await loadDraft()
  }

  async function ensureThread(): Promise<ChatThread | null> {
    if (detail.value) return detail.value.thread
    const created = await guard(() =>
      invoke<ChatThread>('chat_create_thread', {
        connectionId: selectedConnectionId.value,
        modelId: pendingModelId.value,
      }),
    )
    if (!created) return null
    // Move the "new chat" draft to the thread it now belongs to.
    const text = draft.value
    void invoke('chat_save_draft', { scope: NEW_SCOPE, text: '' }).catch(() => {})
    if (text) void invoke('chat_save_draft', { scope: created.id, text }).catch(() => {})
    detail.value = { thread: created, messages: [], bindings: [], practice: [], memory_proposals: [], artifacts: [], active_run_id: null }
    threads.value = [created, ...threads.value]
    if (pendingMode.value !== 'ask') {
      const withMode = await guard(() =>
        invoke<ChatThread>('chat_set_thread_mode', { threadId: created.id, mode: pendingMode.value }),
      )
      if (withMode && detail.value) detail.value = { ...detail.value, thread: withMode }
      return withMode ?? created
    }
    return created
  }

  async function selectModel(connectionId: string, modelId: string | null) {
    pendingConnectionId.value = connectionId
    pendingModelId.value = modelId
    if (!detail.value) return
    const updated = await guard(() =>
      invoke<ChatThread>('chat_set_thread_model', {
        threadId: detail.value!.thread.id,
        connectionId,
        modelId,
      }),
    )
    if (updated && detail.value) detail.value = { ...detail.value, thread: updated }
  }

  async function listModels(connectionId: string): Promise<string[]> {
    return invoke<string[]>('chat_list_models', { connectionId })
  }

  async function attach(offer: ChatOffer) {
    error.value = null
    try {
      const t = await ensureThread()
      if (!t) return
      await guard(() =>
        offer.kind === 'goal'
          ? invoke<ChatBinding>('chat_attach_goal', { threadId: t.id, goalId: offer.element_id })
          : invoke<ChatBinding>('chat_attach_lesson', {
              threadId: t.id,
              courseId: offer.course_id,
              elementId: offer.element_id,
            }),
      )
      await reloadThread(t.id)
    } catch (e) {
      error.value = chatErrorCode(e)
    }
  }

  async function detach(bindingId: string) {
    if (!detail.value) return
    const id = detail.value.thread.id
    try {
      await guard(() => invoke('chat_detach', { threadId: id, bindingId }))
      excludedBindings.value = excludedBindings.value.filter((b) => b !== bindingId)
      await reloadThread(id)
    } catch (e) {
      error.value = chatErrorCode(e)
    }
  }

  async function prepare(): Promise<ChatPreparedMessage | null> {
    const t = await ensureThread()
    if (!t) return null
    preparing.value = true
    try {
      const next = await guard(() =>
        invoke<ChatPreparedMessage>('chat_prepare_message', {
          threadId: t.id,
          text: pendingRegenerate ? null : pendingText,
          regenerateOf: pendingRegenerate,
          excludedBindings: excludedBindings.value,
          excludedMemory: excludedMemory.value,
          includeHistory: includeHistory.value,
        }),
      )
      return next ?? null
    } finally {
      preparing.value = false
    }
  }

  /** Send the draft. Shows the review sheet when the scope needs review. */
  async function send() {
    const text = draft.value.trim()
    if (!text || streaming.value || preparing.value) return
    pendingText = text
    pendingRegenerate = null
    pendingFromDraft = true
    await prepareAndMaybeStart()
  }

  /** Send a fixed request (such as "another practice question") without
   * touching the draft. */
  async function ask(text: string) {
    if (!text.trim() || streaming.value || preparing.value) return
    pendingText = text.trim()
    pendingRegenerate = null
    pendingFromDraft = false
    await prepareAndMaybeStart()
  }

  async function setMode(next: 'ask' | 'learn' | 'practice') {
    pendingMode.value = next
    if (!detail.value) return
    const updated = await guard(() =>
      invoke<ChatThread>('chat_set_thread_mode', { threadId: detail.value!.thread.id, mode: next }),
    )
    if (updated && detail.value) detail.value = { ...detail.value, thread: updated }
  }

  function replacePractice(view: ChatPracticeView | undefined) {
    if (!view || !detail.value) return
    detail.value = {
      ...detail.value,
      practice: detail.value.practice.map((p) => (p.id === view.id ? view : p)),
    }
  }

  /** Checks an answer locally; no model is asked. Throws on an unreadable answer. */
  async function practiceAttempt(itemId: string, answer: string) {
    replacePractice(await guard(() => invoke<ChatPracticeView>('chat_practice_attempt', { itemId, answer })))
  }

  async function practiceHint(itemId: string) {
    replacePractice(await guard(() => invoke<ChatPracticeView>('chat_practice_hint', { itemId })))
  }

  async function practiceReveal(itemId: string) {
    replacePractice(await guard(() => invoke<ChatPracticeView>('chat_practice_reveal', { itemId })))
  }

  /** Ask the same question again; the existing answer is kept. */
  async function retry(userMessageId: string) {
    if (streaming.value || preparing.value) return
    pendingText = null
    pendingRegenerate = userMessageId
    await prepareAndMaybeStart()
  }

  async function prepareAndMaybeStart() {
    error.value = null
    try {
      const next = await prepare()
      if (!next) return
      if (!next.fits || next.requires_review) {
        prepared.value = next
        return
      }
      await start(next)
    } catch (e) {
      error.value = chatErrorCode(e)
    }
  }

  /** Re-prepare after the learner switched an optional item in review. */
  async function revise(change: { binding?: string; memory?: string; history?: boolean }) {
    if (change.memory) {
      const m = change.memory
      excludedMemory.value = excludedMemory.value.includes(m)
        ? excludedMemory.value.filter((x) => x !== m)
        : [...excludedMemory.value, m]
    }
    if (change.binding) {
      const b = change.binding
      excludedBindings.value = excludedBindings.value.includes(b)
        ? excludedBindings.value.filter((x) => x !== b)
        : [...excludedBindings.value, b]
    }
    if (change.history !== undefined) includeHistory.value = change.history
    try {
      const next = await prepare()
      if (next) prepared.value = next
    } catch (e) {
      prepared.value = null
      error.value = chatErrorCode(e)
    }
  }

  function cancelReview() {
    prepared.value = null
    pendingText = null
    pendingRegenerate = null
  }

  async function confirmReview() {
    const next = prepared.value
    if (!next || !next.fits) return
    prepared.value = null
    try {
      await start(next)
    } catch (e) {
      error.value = chatErrorCode(e)
    }
  }

  async function start(next: ChatPreparedMessage) {
    const session = getProfileSessionToken()
    const started = generation
    let lastSeq = 0
    let runId: string | null = null
    const early: ChatEvent[] = []
    const channel = new Channel<ChatEvent>()
    const apply = (event: ChatEvent) => {
      if (started !== generation || session !== getProfileSessionToken()) return
      if (event.run_id !== runId) return
      if (event.seq <= lastSeq) return
      lastSeq = event.seq
      onEvent(event)
    }
    channel.onmessage = (event) => {
      // Events can arrive before the start command returns the run id.
      if (runId === null) early.push(event)
      else apply(event)
    }
    const result = await guard(() =>
      invoke<ChatStartedRun>('chat_start_run', {
        prepareId: next.prepare_id,
        digest: next.digest,
        clientRequestId: crypto.randomUUID(),
        onEvent: channel,
      }),
    )
    if (!result) return
    runId = result.run_id
    runMessages.set(result.run_id, result.assistant_message.id)
    activeRunId.value = result.existing ? null : result.run_id
    if (pendingText !== null && pendingFromDraft) {
      draft.value = ''
      void invoke('chat_save_draft', { scope: result.thread_id, text: '' }).catch(() => {})
    }
    pendingText = null
    pendingRegenerate = null
    excludedBindings.value = []
    excludedMemory.value = []
    liveText.value = { ...liveText.value, [result.assistant_message.id]: '' }
    await reloadThread(result.thread_id)
    for (const event of early.splice(0)) apply(event)
  }

  function onEvent(event: ChatEvent) {
    const messageId = runMessages.get(event.run_id)
    switch (event.kind) {
      case 'text_delta':
        if (messageId) {
          liveText.value = {
            ...liveText.value,
            [messageId]: (liveText.value[messageId] ?? '') + event.text,
          }
        }
        break
      case 'run_completed':
      case 'run_failed':
      case 'run_cancelled':
        runMessages.delete(event.run_id)
        if (activeRunId.value === event.run_id) activeRunId.value = null
        if (detail.value?.thread.id === event.thread_id) void reloadThread(event.thread_id)
        void refreshThreads()
        break
      default:
        break
    }
  }

  async function stop() {
    const id = activeRunId.value
    if (!id) return
    await invoke<boolean>('chat_cancel_run', { runId: id }).catch(() => false)
  }

  async function refreshMemory() {
    const next = await guard(() => invoke<ChatMemoryItem[]>('chat_list_memory'))
    if (next) memoryItems.value = next
  }

  function syncMemory(item: ChatMemoryItem | undefined) {
    if (!item) return
    const exists = memoryItems.value.some((m) => m.id === item.id)
    memoryItems.value = exists
      ? memoryItems.value.map((m) => (m.id === item.id ? item : m))
      : [...memoryItems.value, item]
    if (detail.value) {
      detail.value = {
        ...detail.value,
        memory_proposals: detail.value.memory_proposals.filter((m) => m.id !== item.id || item.status === 'proposed'),
      }
    }
  }

  async function addMemory(kind: 'preference' | 'observation', content: string) {
    syncMemory(await guard(() => invoke<ChatMemoryItem>('chat_add_memory', { kind, content })))
  }

  async function approveMemory(memoryId: string, content?: string) {
    syncMemory(await guard(() => invoke<ChatMemoryItem>('chat_approve_memory', { memoryId, content: content ?? null })))
  }

  async function updateMemory(memoryId: string, change: { content?: string; active?: boolean }) {
    syncMemory(
      await guard(() =>
        invoke<ChatMemoryItem>('chat_update_memory', {
          memoryId,
          content: change.content ?? null,
          active: change.active ?? null,
        }),
      ),
    )
  }

  /** Replaces or removes an artifact in the open thread and the saved list. */
  function syncArtifact(item: ChatArtifact | null | undefined, removedId?: string) {
    if (item === undefined) return
    const id = item?.id ?? removedId
    if (detail.value) {
      const rest = detail.value.artifacts.filter((a) => a.id !== id)
      const inThread = item && item.thread_id === detail.value.thread.id
      detail.value = { ...detail.value, artifacts: inThread ? [...rest, item].sort((a, b) => a.created_at.localeCompare(b.created_at)) : rest }
    }
    const others = savedArtifacts.value.filter((a) => a.id !== id)
    savedArtifacts.value = item && item.status === 'saved' ? [item, ...others] : others
  }

  async function refreshSaved() {
    const next = await guard(() => invoke<ChatArtifact[]>('chat_list_artifacts'))
    if (next) savedArtifacts.value = next
  }

  async function saveArtifact(artifactId: string) {
    syncArtifact(await guard(() => invoke<ChatArtifact>('chat_save_artifact', { artifactId })))
  }

  /** Records that the learner added a proposed goal as `goalId`. */
  async function goalAdded(artifactId: string, goalId: string) {
    syncArtifact(await guard(() => invoke<ChatArtifact>('chat_goal_added', { artifactId, goalId })))
  }

  /** Records that the learner applied a proposed goal change. */
  async function goalChanged(artifactId: string) {
    syncArtifact(await guard(() => invoke<ChatArtifact>('chat_goal_changed', { artifactId })))
  }

  async function deleteArtifact(artifactId: string) {
    await guard(() => invoke('chat_delete_artifact', { artifactId }))
    syncArtifact(null, artifactId)
  }

  /** Saves a finished answer as a note. */
  async function saveNote(messageId: string, title: string) {
    syncArtifact(await guard(() => invoke<ChatArtifact>('chat_save_note', { messageId, title })))
  }

  async function forgetMemory(memoryId: string) {
    await guard(() => invoke('chat_forget_memory', { memoryId }))
    memoryItems.value = memoryItems.value.filter((m) => m.id !== memoryId)
    if (detail.value) {
      detail.value = {
        ...detail.value,
        memory_proposals: detail.value.memory_proposals.filter((m) => m.id !== memoryId),
      }
    }
  }

  async function refreshApprovals() {
    const next = await guard(() => invoke<ChatGuardianApproval[]>('chat_list_guardian_approvals'))
    if (next) approvals.value = next
  }

  /** The latest guardian decision for a connection at its current endpoint. */
  function approvalFor(connectionId: string, endpoint: string) {
    return approvals.value.find((a) => a.connection_id === connectionId && a.endpoint === endpoint) ?? null
  }

  /** Ask the learner's guardians to approve the selected remote model. The
   * request is sent now if a guardian is reachable, else on the next sync. */
  async function requestGuardianApproval() {
    const connectionId = selectedConnectionId.value
    if (!connectionId) return
    try {
      await invoke('chat_request_guardian_approval', { connectionId })
      error.value = 'guardian_pending'
      void refreshApprovals().catch(() => {})
      void invoke('guardian_sync_now').catch(() => {})
    } catch (e) {
      error.value = chatErrorCode(e)
    }
  }

  async function renameThread(id: string, title: string) {
    const updated = await guard(() => invoke<ChatThread>('chat_rename_thread', { threadId: id, title }))
    if (!updated) return
    threads.value = threads.value.map((t) => (t.id === id ? updated : t))
    if (detail.value?.thread.id === id) detail.value = { ...detail.value, thread: updated }
  }

  async function deleteThread(id: string) {
    await guard(() => invoke('chat_delete_thread', { threadId: id }))
    threads.value = threads.value.filter((t) => t.id !== id)
    if (detail.value?.thread.id === id) await newChat()
  }

  function setOpen(open: boolean) {
    panelOpen.value = open
    if (open) {
      void ensureLoaded()
      void refreshStatus().catch(() => {})
    }
  }

  return {
    panelOpen: readonly(panelOpen),
    expanded,
    showHistory,
    threads: readonly(threads),
    detail,
    draft: readonly(draft),
    connections: usableConnections,
    status: readonly(status),
    prepared,
    preparing: readonly(preparing),
    error,
    liveText: readonly(liveText),
    activeRunId: readonly(activeRunId),
    includeHistory: readonly(includeHistory),
    selectedConnection,
    selectedConnectionId,
    selectedModel,
    streaming,
    thread,
    setOpen,
    toggle: () => setOpen(!panelOpen.value),
    ensureLoaded,
    refreshStatus,
    refreshConnections,
    openThread,
    searchThreads,
    newChat,
    setDraft,
    selectModel,
    listModels,
    attach,
    detach,
    send,
    retry,
    ask,
    requestGuardianApproval,
    refreshApprovals,
    approvalFor,
    showMemory,
    showSaved,
    savedArtifacts: readonly(savedArtifacts),
    refreshSaved,
    saveArtifact,
    deleteArtifact,
    goalAdded,
    goalChanged,
    saveNote,
    memoryItems: readonly(memoryItems),
    refreshMemory,
    addMemory,
    approveMemory,
    updateMemory,
    forgetMemory,
    mode,
    setMode,
    practiceAttempt,
    practiceHint,
    practiceReveal,
    revise,
    cancelReview,
    confirmReview,
    stop,
    renameThread,
    deleteThread,
  }
}
