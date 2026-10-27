// Deep-link runtime: subscribes to opened `alexandria://` / https app-link
// URLs, parses them, and routes — but only once a profile is unlocked. A link
// received on the picker / onboarding / unlock screens is queued and replayed
// the instant a profile becomes ready (unlock, create, or restore). Singleton,
// mirroring the lazy-listener pattern in `useSettings`.
//
// It is also the one door every other "take me there" goes through: a
// notification tap (banner, push, or a row in the inbox) hands its route or
// link to `openPath` / `openLink`, so one allowlist, one hold-until-unlocked
// queue and one set of special cases serve them all.

import type { UnlistenFn } from '@tauri-apps/api/event'
import router from '@/router'
import { useProfiles, onProfileReady } from '@/composables/useProfiles'
import { parseDeepLink, type DeepLinkTarget } from './parse'

let initialized = false
let replayRegistered = false
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
  if (target.kind === 'interview-panel') {
    // The backend checks the link and fetches the interview; a bad link just
    // lands the panelist on the organisations page with nothing opened.
    const { usePanel } = await import('@/composables/usePanel')
    try {
      const seat = await usePanel().openLink(target.link)
      await router.push(`/panel/${seat.sessionId}`)
    } catch (cause) {
      console.warn('could not open interview link', cause)
    }
    return
  }
  if (target.kind === 'governance-genesis-locator') {
    await router.push({ path: '/community/import', query: { locator: target.locator } })
    return
  }
  if (!isKnownRoute(target.path)) return
  await router.push(target.path)
}

/**
 * Replay a queued target on the next unlock/create/restore (warm or cold).
 * Registered once, by whichever caller comes first: `init` on boot, or a
 * notification that arrived before it.
 */
function ensureReplay(): void {
  if (replayRegistered) return
  replayRegistered = true
  onProfileReady(() => {
    if (!pending) return
    const target = pending
    pending = null
    void navigate(target)
  })
}

/**
 * Go to a parsed target — now if a profile is unlocked, else once one is. A
 * second target arriving while one is held replaces it: the most recent
 * thing the person asked for is the one they meant.
 */
async function openTarget(target: DeepLinkTarget): Promise<void> {
  ensureReplay()
  if (useProfiles().isUnlocked.value) {
    await navigate(target)
  } else {
    // Navigating now would be clobbered by the App.vue boot flow routing to
    // the picker/onboarding. Hold it; onProfileReady replays it.
    pending = target
  }
}

/**
 * Go to an in-app path. The same shape `alexandria://open?route=` accepts:
 * absolute, same-document, and known to the router (checked at navigation).
 */
async function openPath(path: string | null | undefined): Promise<void> {
  if (!path || !path.startsWith('/') || path.startsWith('//')) return
  await openTarget({ kind: 'route', path })
}

/** Go wherever a raw deep-link URL points, if it is one of ours. */
async function openLink(raw: string): Promise<void> {
  const target = parseDeepLink(raw)
  if (!target) return
  await openTarget(target)
}

/** The target currently held for replay, if any (exposed for tests). */
function heldTarget(): DeepLinkTarget | null {
  return pending
}

async function init(): Promise<void> {
  if (initialized) return
  initialized = true
  ensureReplay()

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
    for (const u of urls ?? []) void openLink(u)
  } catch {
    /* getCurrent throws outside a Tauri webview — ignore */
  }

  // Warm: fired on every subsequent open (incl. Windows/Linux second-launch
  // forwarded by single-instance, and macOS/mobile native delivery).
  try {
    unlisten = await plugin.onOpenUrl((urls) => {
      for (const u of urls) void openLink(u)
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
  return { init, dispose, openTarget, openPath, openLink, heldTarget }
}
