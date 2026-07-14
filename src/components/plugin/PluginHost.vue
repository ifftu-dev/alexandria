<script setup lang="ts">
/**
 * Top-level host for an element that dispatches to a community plugin.
 *
 * Responsibilities:
 *  - Load the manifest and permission grants for the plugin CID
 *    referenced by the element.
 *  - Mount `PluginIframe` with the current granted-capability set.
 *  - Present `PermissionPrompt` when the plugin requests a capability.
 *  - Persist state and proxy completion events back to the parent
 *    element registry.
 *
 * Phase 1 scope: interactive plugins. Phase 2 adds graded plugins —
 * when a plugin emits `submit` with a non-null `grader` in its manifest,
 * the host calls `plugin_submit_and_grade` to run the grader inside the
 * deterministic Wasmtime sandbox, persists the reproducibility bundle,
 * and emits `scored-complete` with the resulting score.
 */

import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import { useI18n } from 'vue-i18n'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { readFile } from '@tauri-apps/plugin-fs'
import { useLocalApi } from '@/composables/useLocalApi'
import PluginIframe from './PluginIframe.vue'
import PermissionPrompt from './PermissionPrompt.vue'
import { AppSpinner, AppAlert } from '@/components/ui'
import type {
  Element,
  InstalledPlugin,
  IrlSubmission,
  PluginManifest,
  PluginCapability,
  PluginPermissionRecord,
  PluginPermissionScope,
} from '@/types'

interface ScoreRecord {
  version: string
  score: number
  details: unknown
}

const props = defineProps<{
  element: Element
  mode?: 'learn' | 'author' | 'review'
  /** Enrollment the learner is taking this element under. Required for
   *  graded plugins to persist a submission row. */
  enrollmentId?: string | null
  /** Active Sentinel integrity session id. Forwarded to the grader so a
   *  passing graded submission mints an integrity-bound credential. */
  integritySessionId?: string | null
  /** In `review` mode, the id of the submission being reviewed. The plugin's
   *  review UI submits its verdict and the host posts it against this row. */
  reviewSubmissionId?: string | null
}>()

const emit = defineEmits<{
  (e: 'complete'): void
  (e: 'scored-complete', score: number): void
}>()

const { t } = useI18n()
const { invoke } = useLocalApi()

const manifest = ref<PluginManifest | null>(null)
const permissions = ref<PluginPermissionRecord[]>([])
const loading = ref(true)
const loadError = ref<string | null>(null)
const refusalReason = ref<string | null>(null)
// Opaque state the plugin last persisted for this element (e.g. an editor's
// unsubmitted source). Loaded before the iframe mounts so it reaches the
// plugin's `init`; the iframe is keyed on it so a late load still re-inits.
const elementState = ref<unknown>(null)

// Session-scoped capability grants live in-memory only. Database rows
// are for 'always' grants (persistent across vault sessions).
const sessionGrants = ref<Set<PluginCapability>>(new Set())
const onceGrants = ref<Set<PluginCapability>>(new Set())

interface PendingCapability {
  requestId: number
  name: PluginCapability
  reason: string
}
const pendingCapability = ref<PendingCapability | null>(null)
// Requests that arrived while a prompt was already open. Instead of
// auto-denying them (which breaks plugins that fire a request per user action,
// e.g. clicking a "Start" button that needs the mic), they queue and are
// resolved after the current decision — auto-granted if the user just granted
// the same capability, otherwise prompted in turn.
const pendingQueue = ref<PendingCapability[]>([])

const iframeRef = ref<InstanceType<typeof PluginIframe> | null>(null)

const pluginCid = computed(() => props.element.plugin_cid ?? '')

/** Parsed `content_inline` JSON passed to the iframe in `init`. Plugins
 *  see this as `msg.payload.content` and use it to configure their UI
 *  (e.g. Music Reviews reads its target-notes sequence here).
 *
 *  Security: the top-level `grader_private` key is stripped before the content
 *  reaches the sandboxed iframe. Graded plugins (e.g. the code editors) put
 *  hidden test expectations there; the learner can read anything the iframe
 *  receives, but the deterministic grader still gets the full content because
 *  `submitElement` passes the raw `content_inline` to `plugin_submit_and_grade`. */
const elementContent = computed<unknown>(() => {
  const raw = props.element.content_inline
  if (!raw) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return raw
  }
  if (parsed && typeof parsed === 'object' && !Array.isArray(parsed) && 'grader_private' in parsed) {
    const clone = { ...(parsed as Record<string, unknown>) }
    delete clone.grader_private
    return clone
  }
  return parsed
})

const grantedCapabilities = computed<PluginCapability[]>(() => {
  const granted = new Set<PluginCapability>()
  for (const p of permissions.value) {
    if (p.scope === 'always') granted.add(p.capability)
  }
  for (const c of sessionGrants.value) granted.add(c)
  for (const c of onceGrants.value) granted.add(c)
  return Array.from(granted)
})

// When granting a `once` capability we also need to actually emit it
// to the iframe for this single render. We clear it on teardown.

onMounted(async () => {
  void invoke('frontend_log', { message: `[PluginHost] onMounted pluginCid=${pluginCid.value || '<empty>'} elementId=${props.element.id}` })
  if (!pluginCid.value) {
    loadError.value = t('plugins.host.noCid')
    loading.value = false
    return
  }
  try {
    const [list, m, perms, savedState] = await Promise.all([
      invoke<InstalledPlugin[]>('plugin_list'),
      invoke<PluginManifest>('plugin_get_manifest', { pluginCid: pluginCid.value }),
      invoke<PluginPermissionRecord[]>('plugin_list_permissions', { pluginCid: pluginCid.value }),
      invoke<string | null>('plugin_load_element_state', { elementId: props.element.id }).catch(
        () => null,
      ),
    ])
    if (savedState) {
      try {
        elementState.value = JSON.parse(savedState)
      } catch {
        // Corrupt/legacy blob — ignore and start fresh.
      }
    }
    const installed = list.find((p) => p.plugin_cid === pluginCid.value)
    if (installed && !installed.enabled) {
      refusalReason.value = t('plugins.host.disabled')
      manifest.value = m
      permissions.value = perms
      return
    }
    manifest.value = m
    permissions.value = perms

    // Phase 2 supports both interactive and graded plugins. Refuse only
    // if the plugin declares no kinds at all (manifest validation should
    // have already caught that, but defense in depth).
    if (m.kinds.length === 0) {
      refusalReason.value = t('plugins.host.noKinds')
    } else if (m.kinds.includes('graded') && !m.kinds.includes('interactive') && !m.grader) {
      refusalReason.value = t('plugins.host.noGrader')
    }
  } catch (e) {
    loadError.value = t('plugins.host.loadFailed', { error: String(e) })
  } finally {
    loading.value = false
  }
})

onBeforeUnmount(() => {
  // Once-grants do not persist beyond a single plugin mount.
  onceGrants.value.clear()
  pendingCapability.value = null
  pendingQueue.value = []
})

// Proxy plugin events back to the element registry.
function onReady(declared: string[]) {
  // The plugin has shown us which capabilities it actually intends to
  // use. Not persisted — just logged for debugging.
  console.debug('[alex] plugin ready', pluginCid.value, declared)
}

function onRequestCapability(requestId: number, name: PluginCapability, reason: string) {
  // Already granted (always / session / once for this mount)? Auto-resolve
  // without re-prompting the user — the consent has been recorded.
  if (grantedCapabilities.value.includes(name)) {
    iframeRef.value?.resolveCapabilityRequest(requestId, true)
    iframeRef.value?.sendCapabilityGranted(name, 'session')
    return
  }
  // A prompt is already open — queue this request instead of denying it, so a
  // plugin that requests the capability again (e.g. a second Start click) isn't
  // spuriously rejected. It resolves when the current decision is made.
  if (pendingCapability.value) {
    pendingQueue.value.push({ requestId, name, reason })
    return
  }
  pendingCapability.value = { requestId, name, reason }
}

/** After a decision, resolve any queued requests: auto-grant ones the user just
 *  allowed, and promote the next still-ungranted request to the active prompt. */
function drainCapabilityQueue() {
  const still: PendingCapability[] = []
  for (const q of pendingQueue.value) {
    if (grantedCapabilities.value.includes(q.name)) {
      iframeRef.value?.resolveCapabilityRequest(q.requestId, true)
      iframeRef.value?.sendCapabilityGranted(q.name, 'session')
    } else {
      still.push(q)
    }
  }
  pendingQueue.value = still
  if (!pendingCapability.value && still.length > 0) {
    pendingCapability.value = still.shift() ?? null
  }
}

async function onPermissionDecision(decision: 'once' | 'session' | 'always' | 'deny') {
  const pending = pendingCapability.value
  if (!pending) return
  pendingCapability.value = null

  const iframe = iframeRef.value
  if (!iframe) return

  if (decision === 'deny') {
    iframe.resolveCapabilityRequest(pending.requestId, false)
    drainCapabilityQueue()
    return
  }

  if (decision === 'always') {
    try {
      await invoke('plugin_grant_capability', {
        pluginCid: pluginCid.value,
        capability: pending.name,
        scope: 'always' as PluginPermissionScope,
      })
      // Reload permissions so the granted-capability computed updates.
      permissions.value = await invoke<PluginPermissionRecord[]>('plugin_list_permissions', {
        pluginCid: pluginCid.value,
      })
    } catch (e) {
      console.error('[alex] failed to persist capability grant', e)
    }
  } else if (decision === 'session') {
    sessionGrants.value.add(pending.name)
  } else if (decision === 'once') {
    onceGrants.value.add(pending.name)
  }

  iframe.resolveCapabilityRequest(pending.requestId, true)
  iframe.sendCapabilityGranted(pending.name, decision)
  drainCapabilityQueue()
}

/**
 * Open the host's native file picker on the plugin's behalf. The sandboxed
 * iframe can't present one itself; the user's file selection is its own consent
 * (no capability grant needed). Reads the chosen files and returns their bytes
 * to the plugin over the port.
 */
async function onPickFiles(requestId: number, options: unknown) {
  const iframe = iframeRef.value
  if (!iframe) return
  const opts = (options ?? {}) as { multiple?: boolean; accept?: string[] }
  try {
    const selection = await openFileDialog({
      multiple: opts.multiple ?? true,
      directory: false,
    })
    const paths = selection == null ? [] : Array.isArray(selection) ? selection : [selection]
    const files = await Promise.all(
      paths.map(async (p) => {
        const data = await readFile(p)
        const name = p.split(/[\\/]/).pop() ?? p
        return { name, size: data.byteLength, data }
      }),
    )
    iframe.resolvePickFiles(requestId, { files })
  } catch (e) {
    iframe.resolvePickFiles(requestId, { files: [] }, String(e))
  }
}

function onPersistState(blob: unknown) {
  // Durably persist the plugin's opaque per-element state so unsubmitted work
  // (e.g. a code editor's source) survives navigating away and restarting
  // the app. Best-effort + fire-and-forget: a failed save must not disrupt the
  // plugin. Reloaded into the plugin's `init` payload on next mount.
  elementState.value = blob
  void invoke('plugin_save_element_state', {
    elementId: props.element.id,
    pluginCid: pluginCid.value,
    stateJson: JSON.stringify(blob),
  }).catch((e) => console.debug('[alex] persist_state save failed', e))
}

async function onEmitEvent(requestId: number, type: string, payload: unknown) {
  if (type === 'debug_log') {
    const msg = (payload as { msg?: string } | null)?.msg ?? ''
    void invoke('frontend_log', { message: `[plugin:${manifest.value?.name}] ${msg}` })
    iframeRef.value?.resolveEvent(requestId, null)
    return
  }
  // Special-cased events the plugin awaits a real response from.
  if (type === 'irl_refresh') {
    try {
      const submissions = await invoke<IrlSubmission[]>('irl_list_my_submissions', {
        pluginCid: pluginCid.value,
      })
      iframeRef.value?.resolveEvent(requestId, { submissions })
    } catch (e) {
      iframeRef.value?.resolveEvent(requestId, null, String(e))
    }
    return
  }
  // Fire-and-forget event (e.g. note_correct/note_wrong telemetry). Resolve it
  // so the plugin's emitEvent promise settles — the iframe no longer sends a
  // default response of its own.
  console.debug('[alex] plugin event', type, payload)
  iframeRef.value?.resolveEvent(requestId, null)
}

async function onSubmit(requestId: number, submission: unknown, metadata: unknown) {
  const m = manifest.value
  if (!m) {
    iframeRef.value?.resolveSubmit(requestId, null, 'manifest unavailable')
    return
  }

  const meta = (metadata as Record<string, unknown> | null) ?? null

  // Review mode — the instructor's review UI is posting its verdict for the
  // submission being reviewed. Route to the generalized review-post IPC.
  if (props.mode === 'review') {
    if (!props.reviewSubmissionId) {
      iframeRef.value?.resolveSubmit(requestId, null, 'no submission to review')
      return
    }
    const sub = (submission as Record<string, unknown> | null) ?? {}
    const pick = <T,>(key: string): T | undefined =>
      (meta?.[key] as T | undefined) ?? (sub[key] as T | undefined)
    const score = pick<number>('score')
    if (typeof score !== 'number' || !isFinite(score)) {
      iframeRef.value?.resolveSubmit(requestId, null, 'review is missing a numeric score')
      return
    }
    const feedback = pick<string>('feedback') ?? ''
    const skillRatings = pick<Record<string, number>>('skill_ratings') ?? {}
    try {
      await invoke('irl_post_review', {
        submissionId: props.reviewSubmissionId,
        score,
        feedback,
        skillRatingsJson: JSON.stringify(skillRatings),
      })
      iframeRef.value?.resolveSubmit(requestId, { reviewed: true })
      emit('complete')
    } catch (e) {
      iframeRef.value?.resolveSubmit(requestId, null, String(e))
    }
    return
  }

  // Learner submit → instructor inbox, for any plugin that declares the
  // `instructor_review` capability (the IRL Review plugin's `type:'irl_review'`
  // metadata is still honoured for backward compatibility).
  const isReviewSubmit =
    meta?.type === 'irl_review' || (m.capabilities?.includes('instructor_review') ?? false)
  if (isReviewSubmit) {
    const skills = Array.isArray(meta?.skills) ? (meta.skills as unknown[]) : []
    try {
      const submissionId = await invoke<string>('irl_submit_for_review', {
        pluginCid: pluginCid.value,
        elementId: props.element.id,
        enrollmentId: props.enrollmentId ?? null,
        submissionJson: JSON.stringify(submission ?? {}),
        skillsJson: JSON.stringify(skills),
      })
      iframeRef.value?.resolveSubmit(requestId, { submission_id: submissionId })
    } catch (e) {
      iframeRef.value?.resolveSubmit(requestId, null, String(e))
    }
    return
  }

  // Graded plugin path — run the WASM grader.
  if (!m.grader) {
    iframeRef.value?.resolveSubmit(requestId, { submission_received: true })
    console.warn('[alex] plugin submitted but manifest has no grader — ignoring')
    return
  }
  if (!props.enrollmentId) {
    iframeRef.value?.resolveSubmit(requestId, null, 'no enrollment')
    iframeRef.value?.sendSubmitAck('', null)
    return
  }

  const contentJson = props.element.content_inline ?? '{}'
  const submissionJson = JSON.stringify(submission ?? {})

  try {
    const score = await invoke<ScoreRecord>('plugin_submit_and_grade', {
      pluginCid: pluginCid.value,
      elementId: props.element.id,
      enrollmentId: props.enrollmentId,
      contentJson,
      submissionJson,
      integritySessionId: props.integritySessionId ?? null,
    })
    iframeRef.value?.resolveSubmit(requestId, { score: score.score })
    iframeRef.value?.sendSubmitAck(blake3HexHint(), score.score)
    emit('scored-complete', score.score)
  } catch (e) {
    console.error('[alex] grade failed', e)
    iframeRef.value?.resolveSubmit(requestId, null, String(e))
    iframeRef.value?.sendSubmitAck('', null)
  }
}

/** The host computes BLAKE3 of the submission bytes for the persisted row;
 *  the iframe doesn't need the actual CID right now (it's a hint for the
 *  plugin's own UI, e.g. "submission saved as <short cid>"). Phase 3
 *  surfaces the real CID once submissions are pinned in the iroh store. */
function blake3HexHint(): string {
  return ''
}

function onComplete(progress: number, advisoryScore: number | null) {
  if (progress >= 1) {
    if (advisoryScore !== null) {
      emit('scored-complete', advisoryScore)
    } else {
      emit('complete')
    }
  }
}

function onIframeError(msg: string) {
  console.error('[alex] plugin iframe error:', msg)
}
</script>

<template>
  <div class="plugin-host flex flex-col h-full min-h-0">
    <div v-if="loading" class="flex items-center justify-center p-10">
      <AppSpinner />
    </div>

    <AppAlert v-else-if="loadError" variant="error">
      {{ loadError }}
    </AppAlert>

    <AppAlert v-else-if="refusalReason" variant="warning">
      {{ refusalReason }}
    </AppAlert>

    <template v-else-if="manifest">
      <div class="flex-1 min-h-0 flex flex-col">
        <PluginIframe
          ref="iframeRef"
          :plugin-cid="pluginCid"
          :entry="manifest.entry"
          :declared-capabilities="manifest.capabilities"
          :granted-capabilities="grantedCapabilities"
          :content="elementContent"
          :state="elementState"
          :mode="props.mode ?? 'learn'"
          :element-id="props.element.id"
          class="flex-1 min-h-0"
          @ready="onReady"
          @request-capability="onRequestCapability"
          @persist-state="onPersistState"
          @emit-event="onEmitEvent"
          @submit="onSubmit"
          @complete="onComplete"
          @pick-files="onPickFiles"
          @error="onIframeError"
        />
      </div>
    </template>

    <PermissionPrompt
      v-if="pendingCapability && manifest"
      :plugin-name="manifest.name"
      :author-did="manifest.author_did"
      :capability="pendingCapability.name"
      :reason="pendingCapability.reason"
      @decide="onPermissionDecision"
    />
  </div>
</template>
