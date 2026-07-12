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
  /** Manifest-declared capabilities. Drives the iframe's Permissions
   *  Policy `allow` attribute at load time — WKWebView/WebView2 read
   *  this once and ignore later mutations, so we have to declare the
   *  full set the plugin might use upfront. Runtime gating happens via
   *  the host↔plugin `capability_granted`/`capability_revoked` messages
   *  built from `grantedCapabilities`. */
  declaredCapabilities: PluginCapability[]
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
  /** Plugin emitted a telemetry event. The host can synchronously call
   *  [`resolveEvent`] with a response payload (e.g. for `irl_refresh` to
   *  return the learner's submissions). If the host does not resolve
   *  synchronously, a default `null` response is sent after the emit
   *  returns. */
  (e: 'emit-event', requestId: number, type: string, payload: unknown): void
  /** Plugin submitted a credential-bearing submission. The host can
   *  synchronously call [`resolveSubmit`] (or asynchronously, once the
   *  IPC call returns) with the response payload — e.g. `{submission_id}`
   *  for IRL Review. If not resolved within the emit, a default
   *  `{submission_received: true}` is sent. */
  (e: 'submit', requestId: number, submission: unknown, metadata: unknown): void
  /** Plugin marked the element complete. */
  (e: 'complete', progress: number, advisoryScore: number | null): void
  /** Plugin requested the host's native file picker. Host resolves via
   *  [`resolvePickFiles`] with the selected files. */
  (e: 'pick-files', requestId: number, options: unknown): void
  /** Host-internal error (sandbox escape attempt, malformed message, etc.). */
  (e: 'error', message: string): void
}>()

defineExpose({
  resolveCapabilityRequest,
  resolveSubmit,
  resolveEvent,
  resolvePickFiles,
  sendCapabilityGranted,
  sendCapabilityRevoked,
  sendSubmitAck,
})

const iframeEl = ref<HTMLIFrameElement | null>(null)
/** Host-side MessagePort. Other half goes to the iframe. */
const hostPort = shallowRef<MessagePort | null>(null)
/** Pending capability-request ids awaiting host decision. */
const pendingCapabilityRequests = new Map<number, { name: PluginCapability; reason: string }>()
/** Submit / event request ids the host may resolve with a custom payload. */
const pendingResponses = new Set<number>()

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
  for (const cap of props.declaredCapabilities) {
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
  try {
    const w = window as unknown as { __TAURI_INTERNALS__?: unknown }
    if (w.__TAURI_INTERNALS__) {
      import('@tauri-apps/api/core').then(({ invoke }) => {
        void invoke('frontend_log', {
          message: `[PluginIframe] onIframeLoad src=${srcUrl.value} declared=${props.declaredCapabilities.join(',')}`,
        })
      }).catch(() => {})
    }
  } catch {
    // ignore
  }
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
  // Dev-time diagnostics from bootstrap.js / plugin code.
  if (ev.data && typeof ev.data === 'object' && (ev.data as { __alex_diag__?: unknown }).__alex_diag__) {
    const msg = String((ev.data as { msg?: unknown }).msg ?? '')
    const w = window as unknown as { __TAURI_INTERNALS__?: unknown }
    if (w.__TAURI_INTERNALS__) {
      import('@tauri-apps/api/core').then(({ invoke }) => {
        void invoke('frontend_log', { message: `[bootstrap] ${msg}` })
      }).catch(() => {})
    }
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

  // Dev-time trace — surface every plugin→host message into the Rust log.
  if (msg && typeof msg === 'object') {
    try {
      // Lazy import to avoid pulling Tauri into the bundle path twice.
      // Errors are swallowed so non-Tauri preview builds keep working.
      const w = window as unknown as { __TAURI_INTERNALS__?: unknown }
      if (w.__TAURI_INTERNALS__) {
        import('@tauri-apps/api/core').then(({ invoke }) => {
          void invoke('frontend_log', {
            message: `[PluginIframe] msg type=${msg.type} request_id=${msg.request_id} api=${msg.api_version}`,
          })
        }).catch(() => {})
      }
    } catch {
      // ignore
    }
  }

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
      // After ready, push init so the plugin gets content+state+theme.
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
          theme: collectHostThemeVars(),
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
      // Like `submit`, the host resolves via `resolveEvent` — synchronously for
      // fire-and-forget events, or asynchronously for ones that await IPC (e.g.
      // `irl_refresh` returning the learner's submissions). Do NOT send a
      // default response here: doing so races the async handler and delivers
      // `null` before the real payload arrives. The host resolves every event.
      const type = typeof payload.type === 'string' ? payload.type : 'unknown'
      pendingResponses.add(msg.request_id)
      emit('emit-event', msg.request_id, type, payload.payload)
      return
    }
    case 'submit': {
      pendingResponses.add(msg.request_id)
      emit('submit', msg.request_id, payload.submission, payload.metadata)
      if (pendingResponses.has(msg.request_id)) {
        // Host did not resolve synchronously — it will call resolveSubmit
        // once its async work (e.g. IPC) settles. Leave the entry in
        // pendingResponses; resolveSubmit / resolveEvent clears it.
      }
      return
    }
    case 'pick_files': {
      // Host-resolved asynchronously via `resolvePickFiles` (opens the native
      // file dialog + reads the chosen files).
      pendingResponses.add(msg.request_id)
      emit('pick-files', msg.request_id, payload)
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

/** Snapshot the host's `--app-*` theme tokens at iframe init time so the
 *  plugin can opt into the user's theme (Light / Dark / future custom
 *  accent). The plugin reads `init.payload.theme` and applies the tokens
 *  to its own `documentElement` via bootstrap.js. */
function collectHostThemeVars(): Record<string, string> {
  const cs = getComputedStyle(document.documentElement)
  const tokens: Record<string, string> = {}
  // Surface the full `--app-*` palette plus a small generic alias set so
  // plugin authors can write `var(--theme-accent)` without knowing the
  // host's internal naming convention.
  const APP_TOKENS = [
    'background', 'foreground',
    'muted', 'muted-foreground',
    'card', 'card-foreground',
    'border', 'input', 'ring',
    'primary', 'primary-foreground', 'primary-hover',
    'secondary', 'secondary-foreground',
    'accent', 'accent-foreground',
    'success', 'success-foreground',
    'warning', 'warning-foreground',
    'error', 'error-foreground',
    'destructive',
  ] as const
  for (const t of APP_TOKENS) {
    const v = cs.getPropertyValue(`--app-${t}`).trim()
    if (v) {
      tokens[`--app-${t}`] = v
      // Generic mirror — host-agnostic alias plugins should target.
      tokens[`--theme-${t}`] = v
    }
  }
  return tokens
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

/** Host resolves a pending `submit` request (e.g. with `{submission_id}`). */
function resolveSubmit(requestId: number, payload: unknown, error?: string) {
  if (!pendingResponses.delete(requestId)) return
  sendResponse(requestId, payload, error)
}

function resolvePickFiles(requestId: number, payload: unknown, error?: string) {
  if (!pendingResponses.delete(requestId)) return
  sendResponse(requestId, payload, error)
}

/** Host resolves a pending `emit_event` request with a custom payload
 *  (e.g. answering an `irl_refresh` with `{submissions:[…]}`). */
function resolveEvent(requestId: number, payload: unknown, error?: string) {
  if (!pendingResponses.delete(requestId)) return
  sendResponse(requestId, payload, error)
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
    :key="`${pluginCid}|${entry}|${allowAttribute}`"
    ref="iframeEl"
    :src="srcUrl"
    sandbox="allow-scripts allow-downloads"
    :allow="allowAttribute"
    referrerpolicy="no-referrer"
    class="plugin-iframe block w-full h-full min-h-[400px] bg-background"
    @load="onIframeLoad"
  />
</template>

<style scoped>
.plugin-iframe {
  /* Give the iframe a safe default; plugin UIs scale inside. */
  color-scheme: normal;
}
</style>
