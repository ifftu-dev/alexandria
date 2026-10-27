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

import { getProfileSessionToken } from '@/composables/profileSession'
import { useLocalApi } from '@/composables/useLocalApi'
import type { NotificationOpen } from '@/types'
import { isNavigationFailure, NavigationFailureType, type RouteLocationRaw } from 'vue-router'
import type { UnlistenFn } from '@tauri-apps/api/event'
import router from '@/router'
import { useProfiles, onProfileReady } from '@/composables/useProfiles'
import { parseDeepLink, type DeepLinkTarget } from './parse'

let initialized = false
let replayRegistered = false
let unlisten: UnlistenFn | null = null
let pending: DeepLinkTarget | null = null
let notification: NotificationOpen | null = null
let pendingProfile: string | null = null
const consumed = new Set<string>()
let navigating: Promise<void> | null = null
let destinationSession: string | null | undefined

function hasUnlockDestination(): boolean {
  return destinationSession !== undefined && destinationSession === getProfileSessionToken()
}

async function replay(): Promise<void> {
  if (navigating) return navigating
  if (!useProfiles().isUnlocked.value || !pending) return
  if (pendingProfile && pendingProfile !== useProfiles().activeProfileId.value) return
  const target = pending
  const intent = notification
  const session = getProfileSessionToken()
  navigating = (async () => {
    let destination = target
    if (intent && !intent.route) {
      const resolved = await useLocalApi().invoke<NotificationOpen | null>('notifications_pending_open')
      if (session !== getProfileSessionToken() || pending !== target || !resolved
        || resolved.id !== intent.id || resolved.profileId !== intent.profileId
        || !resolved.route.startsWith('/') || resolved.route.startsWith('//')) return
      destination = { kind: 'route', path: resolved.route }
    }
    if (!await navigate(destination)) return
    if (session !== getProfileSessionToken()) return
    destinationSession = session
    if (intent) {
      consumed.add(intent.id)
      await useLocalApi().invoke('notifications_consume_open', { id: intent.id }).catch(() => {})
    }
    if (pending === target) { pending = null; notification = null }
  })()
  try { await navigating } finally {
    navigating = null
    if (pending && pending !== target) await replay()
  }
}

async function openNotification(intent: NotificationOpen): Promise<void> {
  if (!intent.id || !intent.profileId || (intent.route !== '' && (!intent.route?.startsWith('/') || intent.route.startsWith('//')))) return
  if (consumed.has(intent.id) || notification?.id === intent.id) return
  ensureReplay()
  pending = { kind: 'route', path: intent.route }
  notification = intent
  pendingProfile = intent.profileId
  await replay()
}

/** Does this path resolve to a real registered route? (open-redirect guard) */
function isKnownRoute(path: string): boolean {
  try {
    const route = router.resolve(path)
    return route.matched.length > 0 && route.name !== 'not-found'
  } catch {
    return false
  }
}

async function pushForSession(to: RouteLocationRaw, current: () => boolean): Promise<boolean> {
  const path = router.resolve(to).fullPath
  // Router guards and lazy components can await after openTarget has checked
  // the session. Recheck at the final navigation boundary, before commitment.
  const removeGuard = router.beforeResolve(next => next.fullPath !== path || current())
  try {
    const failure = await router.push(to)
    return current()
      && (!failure || isNavigationFailure(failure, NavigationFailureType.duplicated))
      && router.currentRoute.value.fullPath === path
  } finally {
    removeGuard()
  }
}

async function navigate(target: DeepLinkTarget): Promise<boolean> {
  const session = getProfileSessionToken()
  const current = () => session === getProfileSessionToken() && useProfiles().isUnlocked.value
  if (!current()) return false
  if (target.kind === 'guardian-accept') {
    // Hand the code to the guardian dashboard, which prompts the parent to
    // confirm and runs `guardian_accept_invite` (surfaces its own errors).
    return pushForSession({ path: '/guardian', query: { accept: target.code } }, current)
  }
  if (target.kind === 'interview-panel') {
    // The backend checks the link and fetches the interview; a bad link just
    // lands the panelist on the organisations page with nothing opened.
    const { usePanel } = await import('@/composables/usePanel')
    if (!current()) return false
    try {
      const seat = await usePanel().openLink(target.link)
      if (!current()) return false
      return await pushForSession(`/panel/${seat.sessionId}`, current)
    } catch (cause) {
      console.warn('could not open interview link', cause)
      return false
    }
  }
  if (target.kind === 'governance-genesis-locator') {
    return pushForSession({ path: '/community/import', query: { locator: target.locator } }, current)
  }
  if (!isKnownRoute(target.path)) return false
  return pushForSession(target.path, current)
}

/**
 * Replay a queued target on the next unlock/create/restore (warm or cold).
 * Registered once, by whichever caller comes first: `init` on boot, or a
 * notification that arrived before it.
 */
function ensureReplay(): void {
  if (replayRegistered) return
  replayRegistered = true
  onProfileReady(replay)
}

/**
 * Go to a parsed target — now if a profile is unlocked, else once one is. A
 * second target arriving while one is held replaces it: the most recent
 * thing the person asked for is the one they meant.
 */
async function openTarget(target: DeepLinkTarget): Promise<void> {
  ensureReplay()
  pending = target
  notification = null
  pendingProfile = useProfiles().isUnlocked.value ? useProfiles().activeProfileId.value : null
  await replay()
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
  notification = null
  pendingProfile = null
  consumed.clear()
  destinationSession = undefined
}

export function useDeepLinks() {
  return { init, dispose, openTarget, openPath, openLink, heldTarget, openNotification, hasUnlockDestination }
}
