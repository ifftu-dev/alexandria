// Deep-link runtime: subscribes to opened `alexandria://` / https app-link
// URLs, parses them, and routes — but only once a profile is unlocked. A link
// received on the picker / onboarding / unlock screens is queued and replayed
// the instant a profile becomes ready (unlock, create, or restore). Singleton,
// mirroring the lazy-listener pattern in `useSettings`.

import type { UnlistenFn } from '@tauri-apps/api/event'
import router from '@/router'
import { useProfiles, onProfileReady } from '@/composables/useProfiles'
import { parseDeepLink, type DeepLinkTarget } from './parse'

let initialized = false
let unlisten: UnlistenFn | null = null
let pending: DeepLinkTarget | null = null

/** Does this path resolve to a real registered route? (open-redirect guard) */
function isKnownRoute(path: string): boolean {
  try {
    return router.resolve(path).matched.length > 0
  } catch {
    return false
  }
}

async function navigate(target: DeepLinkTarget): Promise<void> {
  if (target.kind === 'guardian-accept') {
    // Hand the code to the guardian dashboard, which prompts the parent to
    // confirm and runs `guardian_accept_invite` (surfaces its own errors).
    await router.push({ path: '/guardian', query: { accept: target.code } })
    return
  }
  if (!isKnownRoute(target.path)) return
  await router.push(target.path)
}

async function dispatch(raw: string): Promise<void> {
  const target = parseDeepLink(raw)
  if (!target) return
  if (useProfiles().isUnlocked.value) {
    await navigate(target)
  } else {
    // Navigating now would be clobbered by the App.vue boot flow routing to
    // the picker/onboarding. Hold it; onProfileReady replays it.
    pending = target
  }
}

async function init(): Promise<void> {
  if (initialized) return
  initialized = true

  // Replay a queued link on the next unlock/create/restore (warm or cold).
  onProfileReady(() => {
    if (!pending) return
    const target = pending
    pending = null
    void navigate(target)
  })

  // Plugin JS API is the single source of truth for opened URLs.
  let plugin: typeof import('@tauri-apps/plugin-deep-link') | null = null
  try {
    plugin = await import('@tauri-apps/plugin-deep-link')
  } catch {
    // Non-Tauri (browser dev) — nothing to subscribe to.
    return
  }

  // Cold start: the launching URL may already be queued by the OS.
  try {
    const urls = await plugin.getCurrent()
    for (const u of urls ?? []) void dispatch(u)
  } catch {
    /* getCurrent throws outside a Tauri webview — ignore */
  }

  // Warm: fired on every subsequent open (incl. Windows/Linux second-launch
  // forwarded by single-instance, and macOS/mobile native delivery).
  try {
    unlisten = await plugin.onOpenUrl((urls) => {
      for (const u of urls) void dispatch(u)
    })
  } catch {
    /* ignore in web dev */
  }
}

/** Tear down the URL listener (used only in teardown/tests). */
function dispose(): void {
  unlisten?.()
  unlisten = null
  initialized = false
  pending = null
}

export function useDeepLinks() {
  return { init, dispose }
}
