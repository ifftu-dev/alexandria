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
}>()

const emit = defineEmits<{
  (e: 'complete'): void
  (e: 'scored-complete', score: number): void
}>()

const { invoke } = useLocalApi()

const manifest = ref<PluginManifest | null>(null)
const permissions = ref<PluginPermissionRecord[]>([])
const loading = ref(true)
const loadError = ref<string | null>(null)
const refusalReason = ref<string | null>(null)

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

const iframeRef = ref<InstanceType<typeof PluginIframe> | null>(null)

const pluginCid = computed(() => props.element.plugin_cid ?? '')

/** Parsed `content_inline` JSON passed to the iframe in `init`. Plugins
 *  see this as `msg.payload.content` and use it to configure their UI
 *  (e.g. Music Reviews reads its target-notes sequence here). */
const elementContent = computed<unknown>(() => {
  const raw = props.element.content_inline
  if (!raw) return null
  try {
    return JSON.parse(raw)
  } catch {
    return raw
  }
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
    loadError.value = 'This element references a plugin, but no plugin CID is set.'
    loading.value = false
    return
  }
  try {
    const [list, m, perms] = await Promise.all([
      invoke<InstalledPlugin[]>('plugin_list'),
      invoke<PluginManifest>('plugin_get_manifest', { pluginCid: pluginCid.value }),
      invoke<PluginPermissionRecord[]>('plugin_list_permissions', { pluginCid: pluginCid.value }),
    ])
    const installed = list.find((p) => p.plugin_cid === pluginCid.value)
    if (installed && !installed.enabled) {
      refusalReason.value =
        'This plugin is disabled. Re-enable it from Settings → Plugins to use it.'
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
      refusalReason.value = 'This plugin declares no element kinds and cannot be mounted.'
    } else if (m.kinds.includes('graded') && !m.kinds.includes('interactive') && !m.grader) {
      refusalReason.value =
        'This plugin declares "graded" but no grader is attached. The author needs to publish a corrected manifest.'
    }
  } catch (e) {
    loadError.value = `Failed to load plugin: ${e}`
  } finally {
    loading.value = false
  }
})

onBeforeUnmount(() => {
  // Once-grants do not persist beyond a single plugin mount.
  onceGrants.value.clear()
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
  // A second concurrent prompt is auto-denied — Phase 1 only supports
  // one active consent dialog at a time.
  if (pendingCapability.value) {
    iframeRef.value?.resolveCapabilityRequest(requestId, false)
    return
  }
  pendingCapability.value = { requestId, name, reason }
}

async function onPermissionDecision(decision: 'once' | 'session' | 'always' | 'deny') {
  const pending = pendingCapability.value
  if (!pending) return
  pendingCapability.value = null

  const iframe = iframeRef.value
  if (!iframe) return

  if (decision === 'deny') {
    iframe.resolveCapabilityRequest(pending.requestId, false)
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
}

function onPersistState(blob: unknown) {
  // Phase 1: state persistence is in-process only (the iframe is
  // teardowned on element change, so state is effectively per-session).
  // The full `persist_state` SQLite path ships with the built-in plugins
  // migration in Phase 2, when element_submissions lands.
  console.debug('[alex] plugin persist_state (not yet durable)', blob)
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
  console.debug('[alex] plugin event', type, payload)
}

async function onSubmit(requestId: number, submission: unknown, metadata: unknown) {
  const m = manifest.value
  if (!m) {
    iframeRef.value?.resolveSubmit(requestId, null, 'manifest unavailable')
    return
  }

  // IRL Review path — route to the local instructor inbox IPC.
  const meta = (metadata as { type?: string } | null) ?? null
  if (meta?.type === 'irl_review') {
    const skills = Array.isArray((meta as { skills?: unknown }).skills)
      ? ((meta as { skills?: unknown[] }).skills as unknown[])
      : []
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
          :state="null"
          :mode="props.mode ?? 'learn'"
          :element-id="props.element.id"
          class="flex-1 min-h-0"
          @ready="onReady"
          @request-capability="onRequestCapability"
          @persist-state="onPersistState"
          @emit-event="onEmitEvent"
          @submit="onSubmit"
          @complete="onComplete"
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
