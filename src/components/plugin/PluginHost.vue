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
  if (!pluginCid.value) {
    loadError.value = 'This element references a plugin, but no plugin CID is set.'
    loading.value = false
    return
  }
  try {
    const [m, perms] = await Promise.all([
      invoke<PluginManifest>('plugin_get_manifest', { pluginCid: pluginCid.value }),
      invoke<PluginPermissionRecord[]>('plugin_list_permissions', { pluginCid: pluginCid.value }),
    ])
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
  // If a prompt is already open, auto-deny the new request — concurrent
  // capability prompts are not supported in Phase 1 and would produce a
  // confusing UX.
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

function onEmitEvent(type: string, payload: unknown) {
  console.debug('[alex] plugin event', type, payload)
}

async function onSubmit(submission: unknown, _metadata: unknown) {
  const m = manifest.value
  if (!m) return
  if (!m.grader) {
    console.warn('[alex] plugin submitted but manifest has no grader — ignoring')
    return
  }
  if (!props.enrollmentId) {
    console.warn('[alex] graded plugin submission requires an enrollment — refusing')
    iframeRef.value?.sendSubmitAck('', null)
    return
  }

  // Build content_json from element.content_inline. If content_cid is the
  // primary source (Phase 3+), the host will resolve it to bytes here.
  // For Phase 2 session 1, content_inline is sufficient.
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
    iframeRef.value?.sendSubmitAck(blake3HexHint(), score.score)
    emit('scored-complete', score.score)
  } catch (e) {
    console.error('[alex] grade failed', e)
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
  <div class="plugin-host space-y-3">
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
      <div class="flex items-center justify-between rounded-lg border border-border/50 bg-card/40 px-3 py-2 text-xs">
        <div class="flex items-center gap-2">
          <span class="inline-flex items-center gap-1 rounded-full bg-primary/10 px-2 py-0.5 font-medium text-primary">
            Plugin
          </span>
          <span class="font-medium text-foreground">{{ manifest.name }}</span>
          <span class="text-muted-foreground">v{{ manifest.version }}</span>
        </div>
        <span class="font-mono text-[10px] text-muted-foreground">
          {{ pluginCid.slice(0, 12) }}…
        </span>
      </div>

      <PluginIframe
        ref="iframeRef"
        :plugin-cid="pluginCid"
        :entry="manifest.entry"
        :granted-capabilities="grantedCapabilities"
        :content="null"
        :state="null"
        :mode="props.mode ?? 'learn'"
        :element-id="props.element.id"
        @ready="onReady"
        @request-capability="onRequestCapability"
        @persist-state="onPersistState"
        @emit-event="onEmitEvent"
        @submit="onSubmit"
        @complete="onComplete"
        @error="onIframeError"
      />
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
