<script setup lang="ts">
/**
 * Sandboxed plugin iframe + host↔plugin postMessage bridge (protocol v1).
 *
 * Phase 1 of the community plugin system. See
 * `/Users/hack/.claude/plans/prancy-bubbling-grove.md`.
 *
 * Security contract:
 *  - sandbox="allow-scripts" only — never `allow-same-origin`, never
 *    `allow-top-navigation`, never `allow-popups`.
 *  - The `allow` attribute is built exclusively from `grantedCapabilities`.
 *    Revoked capabilities are absent from the attribute entirely.
 *  - Each plugin loads from its own `plugin://<cid>/` origin, so the browser's
 *    same-origin policy gives cross-plugin isolation for free.
 *  - The host sends the MessagePort to the iframe via a one-shot
 *    `window.postMessage` with `{ __alex_init__: true }`. The bootstrap
 *    script injected by the asset protocol handler picks this up and
 *    exposes `window.alex` to plugin authors.
 */

import { ref, shallowRef, onMounted, onBeforeUnmount, watch, computed } from 'vue'
import type { PluginCapability } from '@/types'

const props = defineProps<{
  pluginCid: string
  entry: string
  grantedCapabilities: PluginCapability[]
  /** Arbitrary content config the plugin's `init` sees. */
  content: unknown
  /** Opaque persisted state (JSON-serializable). */
  state: unknown
  /** `learn` | `author` | `review`. Passed through in init. */
  mode: 'learn' | 'author' | 'review'
  locale?: string
  /** Caller-controlled element id — used for scoped state and telemetry. */
  elementId: string
}>()

const emit = defineEmits<{
  /** Plugin has handshaken. `declared` is the subset of manifest-declared
   *  capabilities the plugin will actually use this session. */
  (e: 'ready', declared: string[]): void
  /** Plugin requests a capability. Host must call `resolveCapabilityRequest`
   *  with the same `requestId`. */
  (e: 'request-capability', requestId: number, name: PluginCapability, reason: string): void
  /** Plugin wants to persist opaque state. */
  (e: 'persist-state', blob: unknown): void
  /** Plugin emitted a telemetry event. */
  (e: 'emit-event', type: string, payload: unknown): void
  /** Plugin submitted a credential-bearing submission (Phase 2+). */
  (e: 'submit', submission: unknown, metadata: unknown): void
  /** Plugin marked the element complete. */
  (e: 'complete', progress: number, advisoryScore: number | null): void
  /** Host-internal error (sandbox escape attempt, malformed message, etc.). */
  (e: 'error', message: string): void
}>()

defineExpose({
  resolveCapabilityRequest,
  sendCapabilityGranted,
  sendCapabilityRevoked,
  sendSubmitAck,
})

const iframeEl = ref<HTMLIFrameElement | null>(null)
/** Host-side MessagePort. Other half goes to the iframe. */
const hostPort = shallowRef<MessagePort | null>(null)
/** Pending capability-request ids awaiting host decision. */
const pendingCapabilityRequests = new Map<number, { name: PluginCapability; reason: string }>()

const API_VERSION = '1'

const allowAttribute = computed(() => {
  // Browsers expect a space-separated list of feature names, not the
  // CSP directives. We map our capability names directly to Permissions
  // Policy feature names.
  const map: Record<PluginCapability, string | null> = {
    microphone: 'microphone',
    camera: 'camera',
    midi: 'midi',
    fullscreen: 'fullscreen',
    clipboard: 'clipboard-read; clipboard-write',
    storage: null, // handled host-side via persist_state
    ml_inference: null, // no browser feature
  }
  const features: string[] = []
  for (const cap of props.grantedCapabilities) {
    const f = map[cap]
    if (f) features.push(f)
  }
  return features.join('; ')
})

const srcUrl = computed(() => `plugin://${props.pluginCid}/${props.entry.replace(/^\/+/, '')}`)

onMounted(() => {
  window.addEventListener('message', onWindowMessage)
})

onBeforeUnmount(() => {
  window.removeEventListener('message', onWindowMessage)
  teardown()
})

// Re-initialize the channel every time the plugin CID or entry changes.
watch(
  () => `${props.pluginCid}|${props.entry}`,
  () => {
    teardown()
  },
)

function onIframeLoad() {
  // Create a fresh channel each load so stale ports never bridge two
  // different plugin sessions.
  teardown()
  const channel = new MessageChannel()
  hostPort.value = channel.port1
  channel.port1.onmessage = onPluginMessage

  const iframe = iframeEl.value
  if (!iframe || !iframe.contentWindow) {
    emit('error', 'iframe contentWindow not available')
    return
  }

  // targetOrigin "*" is correct here — the recipient is a different
  // origin (plugin://<cid>/) that we cannot name in advance. The
  // port itself is the authenticated channel.
  iframe.contentWindow.postMessage({ __alex_init__: true }, '*', [channel.port2])
}

/** Listen for plain window messages from the iframe. In Phase 1 we don't
 *  expect any (everything goes over the port), but a hostile plugin could
 *  try to reach us this way. Ignore + log. */
function onWindowMessage(ev: MessageEvent) {
  if (!iframeEl.value || ev.source !== iframeEl.value.contentWindow) return
  if (ev.data && typeof ev.data === 'object' && (ev.data as { __alex_init__?: unknown }).__alex_init__) {
    // Our own init echoing back via a buggy user handler — ignore.
    return
  }
  // Anything else over window.postMessage is a protocol violation.
  emit('error', 'plugin sent an out-of-band window message')
}

function onPluginMessage(ev: MessageEvent) {
  const msg = ev.data as
    | {
        api_version?: string
        request_id?: number
        type?: string
        payload?: Record<string, unknown>
      }
    | null

  if (!msg || typeof msg !== 'object') return
  if (msg.api_version !== API_VERSION) {
    emit('error', `plugin sent unsupported api_version: ${msg.api_version}`)
    return
  }
  if (typeof msg.request_id !== 'number' || !msg.type) {
    emit('error', 'plugin message missing request_id or type')
    return
  }

  const payload = (msg.payload ?? {}) as Record<string, unknown>

  switch (msg.type) {
    case 'ready': {
      const declared = Array.isArray(payload.declared_capabilities)
        ? (payload.declared_capabilities as string[])
        : []
      emit('ready', declared)
      sendResponse(msg.request_id, null)
      // After ready, push init so the plugin gets content+state.
      sendHostMessage({
        api_version: API_VERSION,
        type: 'init',
        payload: {
          content: props.content,
          state: props.state,
          mode: props.mode,
          granted_capabilities: props.grantedCapabilities,
          locale: props.locale ?? 'en',
          element_id: props.elementId,
        },
      })
      return
    }
    case 'request_capability': {
      const name = payload.name as string
      if (!isKnownCapability(name)) {
        sendResponse(msg.request_id, null, `unknown capability '${name}'`)
        return
      }
      const reason = typeof payload.reason === 'string' ? payload.reason : ''
      pendingCapabilityRequests.set(msg.request_id, { name: name as PluginCapability, reason })
      emit('request-capability', msg.request_id, name as PluginCapability, reason)
      return
    }
    case 'persist_state': {
      emit('persist-state', payload.blob)
      sendResponse(msg.request_id, null)
      return
    }
    case 'emit_event': {
      const type = typeof payload.type === 'string' ? payload.type : 'unknown'
      emit('emit-event', type, payload.payload)
      sendResponse(msg.request_id, null)
      return
    }
    case 'submit': {
      emit('submit', payload.submission, payload.metadata)
      sendResponse(msg.request_id, { submission_received: true })
      return
    }
    case 'complete': {
      const progress =
        typeof payload.progress_fraction === 'number' ? payload.progress_fraction : 1
      const advisory =
        typeof payload.optional_advisory_score === 'number'
          ? (payload.optional_advisory_score as number)
          : null
      emit('complete', progress, advisory)
      sendResponse(msg.request_id, null)
      return
    }
    default:
      sendResponse(msg.request_id, null, `unknown message type '${msg.type}'`)
  }
}

function isKnownCapability(name: string): boolean {
  return (
    name === 'microphone' ||
    name === 'camera' ||
    name === 'midi' ||
    name === 'fullscreen' ||
    name === 'clipboard' ||
    name === 'storage' ||
    name === 'ml_inference'
  )
}

function sendResponse(requestId: number, payload: unknown, error?: string) {
  const port = hostPort.value
  if (!port) return
  port.postMessage({
    api_version: API_VERSION,
    response_id: requestId,
    payload,
    error: error ?? null,
  })
}

function sendHostMessage(msg: Record<string, unknown>) {
  const port = hostPort.value
  if (!port) return
  port.postMessage(msg)
}

function teardown() {
  if (hostPort.value) {
    hostPort.value.onmessage = null
    hostPort.value.close()
    hostPort.value = null
  }
  pendingCapabilityRequests.clear()
}

/** Host resolves a pending capability request (the user clicked grant/deny). */
function resolveCapabilityRequest(requestId: number, granted: boolean) {
  pendingCapabilityRequests.delete(requestId)
  sendResponse(requestId, { granted })
}

/** Unsolicited notification that a previously-granted capability is now active. */
function sendCapabilityGranted(name: PluginCapability, scope: 'once' | 'session' | 'always') {
  sendHostMessage({
    api_version: API_VERSION,
    type: 'capability_granted',
    payload: { name, scope },
  })
}

function sendCapabilityRevoked(name: PluginCapability) {
  sendHostMessage({
    api_version: API_VERSION,
    type: 'capability_revoked',
    payload: { name },
  })
}

function sendSubmitAck(submissionCid: string, score: number | null) {
  sendHostMessage({
    api_version: API_VERSION,
    type: 'submit_ack',
    payload: { submission_cid: submissionCid, score },
  })
}
</script>

<template>
  <iframe
    ref="iframeEl"
    :src="srcUrl"
    sandbox="allow-scripts"
    :allow="allowAttribute"
    referrerpolicy="no-referrer"
    class="plugin-iframe block w-full h-full min-h-[400px] rounded-lg border border-border bg-background"
    @load="onIframeLoad"
  />
</template>

<style scoped>
.plugin-iframe {
  /* Give the iframe a safe default; plugin UIs scale inside. */
  color-scheme: normal;
}
</style>
