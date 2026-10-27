// The notifications inbox: a module singleton mirroring the per-profile
// `notifications` table, kept current by the backend's `notifications:*`
// events. Routing on a tap (an OS banner, a push, a row on the page) is the
// deep-link runtime's job: `openRoute` hands it an in-app path or a full
// `alexandria://` link, and it applies the one allowlist and holds the target
// while no profile is unlocked.

import { computed, readonly, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getProfileSessionToken } from '@/composables/profileSession'
import { useLocalApi } from '@/composables/useLocalApi'
import { isMobilePlatform } from '@/composables/usePlatform'
import { useDeepLinks } from '@/deeplink/useDeepLinks'
import type { AppNotification, PushStatus, NotificationOpen } from '@/types'

const { invoke } = useLocalApi()

const items = ref<AppNotification[]>([])
const unread = ref(0)
const loading = ref(false)
const pushStatus = ref<PushStatus | null>(null)

let opensInitialized = false
let initialized = false
let generation = 0
let unlisteners: UnlistenFn[] = []
let retryTimer: ReturnType<typeof setInterval> | null = null
let onResume: (() => void) | null = null

function currentAction(): () => boolean {
  const gen = generation
  const session = getProfileSessionToken()
  return () => gen === generation && session === getProfileSessionToken()
}

/**
 * Follow a notification's route. A row's `route` is normally an in-app path;
 * a sender may also put a whole deep link there (`alexandria://…` or the
 * https app-link form), which goes through the same parser as a link opened
 * from outside.
 */
async function openRoute(route: string | null | undefined): Promise<void> {
  if (!route) return
  const links = useDeepLinks()
  if (route.startsWith('/')) {
    await links.openPath(route)
  } else {
    await links.openLink(route)
  }
}

async function load(): Promise<void> {
  const current = currentAction()
  loading.value = true
  try {
    const [rows, count] = await Promise.all([
      invoke<AppNotification[]>('notifications_list', { limit: 100, unreadOnly: false }),
      invoke<number>('notifications_unread_count'),
    ])
    if (!current()) return
    items.value = rows
    unread.value = count
  } finally {
    if (current()) loading.value = false
  }
}

async function markRead(ids: string[]): Promise<void> {
  const current = currentAction()
  if (ids.length === 0) return
  await invoke('notifications_mark_read', { ids })
  if (!current()) return
  const now = new Date().toISOString()
  const set = new Set(ids)
  let cleared = 0
  for (const n of items.value) {
    if (set.has(n.id) && !n.readAt) {
      n.readAt = now
      cleared++
    }
  }
  unread.value = Math.max(0, unread.value - cleared)
}

async function markAllRead(): Promise<void> {
  const current = currentAction()
  await invoke('notifications_mark_all_read')
  if (!current()) return
  const now = new Date().toISOString()
  for (const n of items.value) if (!n.readAt) n.readAt = now
  unread.value = 0
}

async function clear(): Promise<void> {
  const current = currentAction()
  await invoke('notifications_clear')
  if (!current()) return
  items.value = []
  unread.value = 0
}

/** Tap a row: mark it read and follow its route. */
async function open(n: AppNotification): Promise<void> {
  const current = currentAction()
  if (!n.readAt) await markRead([n.id]).catch(() => {})
  if (current()) await openRoute(n.route)
}

async function refreshPushStatus(): Promise<PushStatus | null> {
  const current = currentAction()
  try {
    const status = await invoke<PushStatus>('push_status')
    if (current()) pushStatus.value = status
  } catch {
    if (current()) pushStatus.value = null
  }
  return pushStatus.value
}

async function registerPush(): Promise<PushStatus> {
  const current = currentAction()
  const status = await invoke<PushStatus>('push_register')
  if (current()) pushStatus.value = status
  return status
}

async function unregisterPush(): Promise<PushStatus> {
  const current = currentAction()
  const status = await invoke<PushStatus>('push_unregister')
  if (current()) pushStatus.value = status
  return status
}

async function setPushEnabled(enabled: boolean): Promise<PushStatus> {
  const current = currentAction()
  const status = await invoke<PushStatus>('push_set_enabled', { enabled })
  if (current()) pushStatus.value = status
  return status
}

/** Ask the OS once for permission to show banners. Desktop answers granted. */
async function requestPermission(): Promise<string> {
  try {
    return await invoke<string>('notifications_permission_request')
  } catch {
    return 'unavailable'
  }
}

/**
 * Subscribe to the backend's events and load the inbox. Called once a profile
 * is unlocked; `teardown` on lock. Idempotent.
 */
async function init(): Promise<void> {
  if (initialized) return
  initialized = true
  const gen = ++generation
  const session = getProfileSessionToken()
  if (isMobilePlatform) {
    const retry = () => {
      if (gen === generation && session === getProfileSessionToken()) void reconcilePush()
    }
    retryTimer = setInterval(retry, 60_000)
    onResume = () => { if (document.visibilityState === 'visible') retry() }
    document.addEventListener('visibilitychange', onResume)
  }

  await load().catch((e) => console.warn('[notifications] load failed:', e))
  if (gen !== generation) return

  try {
    const releases = await Promise.all([
      listen<{ session: string; row: AppNotification }>('notifications:new', (event) => {
        if (gen !== generation || session !== getProfileSessionToken() || event.payload.session !== session) return
        const row = event.payload.row
        if (!items.value.some((n) => n.id === row.id)) {
          items.value.unshift(row)
          if (!row.readAt) unread.value++
        }
      }),
      listen<{ session: string; unread: number }>('notifications:changed', (event) => {
        if (gen !== generation || session !== getProfileSessionToken() || event.payload.session !== session) return
        unread.value = event.payload.unread
        // Another window or the backend changed rows; resync the list.
        void load().catch(() => {})
      }),

    ])
    if (gen !== generation) {
      for (const release of releases) release()
      return
    }
    unlisteners = releases
  } catch {
    // Outside a Tauri webview (browser dev) — the inbox is static.
    return
  }

}

/** Process-lifetime listener: taps must reach the picker while the vault is locked. */
async function initOpens(): Promise<void> {
  if (opensInitialized) return
  opensInitialized = true
  try {
    await listen<NotificationOpen>('notifications:open', (event) => {
      void useDeepLinks().openNotification(event.payload)
    })
    const held = await invoke<NotificationOpen | null>('notifications_pending_open')
    if (held) await useDeepLinks().openNotification(held)
  } catch (e) {
    opensInitialized = false
    console.warn('[notifications] pending open failed:', e)
  }
}

/** Drop listeners and cached rows so the next profile starts clean. */
function teardown(): void {
  generation++
  if (retryTimer) clearInterval(retryTimer)
  retryTimer = null
  if (onResume) document.removeEventListener('visibilitychange', onResume)
  onResume = null
  initialized = false
  for (const release of unlisteners) release()
  unlisteners = []
  items.value = []
  unread.value = 0
  loading.value = false
  pushStatus.value = null
}

/**
 * Register this device for push after unlock, when the platform supports it
 * and the person has not switched it off. Failures are logged, never shown:
 * the Settings panel reports them on demand.
 */
async function reconcilePush(): Promise<void> {
  const current = currentAction()
  if (!isMobilePlatform) return
  try {
    await registerPush()
  } catch (e) {
    console.warn('[notifications] push registration failed:', e)
    if (current()) await refreshPushStatus()
  }
}

export function useNotifications() {
  return {
    items: readonly(items),
    unread: readonly(unread),
    loading: readonly(loading),
    pushStatus: readonly(pushStatus),
    hasUnread: computed(() => unread.value > 0),
    init,
    initOpens,
    teardown,
    load,
    markRead,
    markAllRead,
    clear,
    open,
    openRoute,
    requestPermission,
    refreshPushStatus,
    registerPush,
    unregisterPush,
    setPushEnabled,
    reconcilePush,
  }
}
