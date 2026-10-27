// The notifications inbox: a module singleton mirroring the per-profile
// `notifications` table, kept current by the backend's `notifications:*`
// events. Routing on a tap (an OS banner, a push, a row on the page) is the
// deep-link runtime's job: `openRoute` hands it an in-app path or a full
// `alexandria://` link, and it applies the one allowlist and holds the target
// while no profile is unlocked.

import { computed, readonly, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useLocalApi } from '@/composables/useLocalApi'
import { isMobilePlatform } from '@/composables/usePlatform'
import { useDeepLinks } from '@/deeplink/useDeepLinks'
import type { AppNotification, PushStatus } from '@/types'

const { invoke } = useLocalApi()

const items = ref<AppNotification[]>([])
const unread = ref(0)
const loading = ref(false)
const pushStatus = ref<PushStatus | null>(null)

let initialized = false
let generation = 0
let unlisteners: UnlistenFn[] = []

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
  const gen = generation
  loading.value = true
  try {
    const [rows, count] = await Promise.all([
      invoke<AppNotification[]>('notifications_list', { limit: 100, unreadOnly: false }),
      invoke<number>('notifications_unread_count'),
    ])
    if (gen !== generation) return
    items.value = rows
    unread.value = count
  } finally {
    if (gen === generation) loading.value = false
  }
}

async function markRead(ids: string[]): Promise<void> {
  if (ids.length === 0) return
  await invoke('notifications_mark_read', { ids })
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
  await invoke('notifications_mark_all_read')
  const now = new Date().toISOString()
  for (const n of items.value) if (!n.readAt) n.readAt = now
  unread.value = 0
}

async function clear(): Promise<void> {
  await invoke('notifications_clear')
  items.value = []
  unread.value = 0
}

/** Tap a row: mark it read and follow its route. */
async function open(n: AppNotification): Promise<void> {
  if (!n.readAt) await markRead([n.id]).catch(() => {})
  await openRoute(n.route)
}

async function refreshPushStatus(): Promise<PushStatus | null> {
  try {
    pushStatus.value = await invoke<PushStatus>('push_status')
  } catch {
    pushStatus.value = null
  }
  return pushStatus.value
}

async function registerPush(): Promise<PushStatus> {
  const status = await invoke<PushStatus>('push_register')
  pushStatus.value = status
  return status
}

async function unregisterPush(): Promise<PushStatus> {
  const status = await invoke<PushStatus>('push_unregister')
  pushStatus.value = status
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

  await load().catch((e) => console.warn('[notifications] load failed:', e))
  if (gen !== generation) return

  try {
    const releases = await Promise.all([
      listen<AppNotification>('notifications:new', (event) => {
        if (gen !== generation) return
        const row = event.payload
        if (!items.value.some((n) => n.id === row.id)) {
          items.value.unshift(row)
          if (!row.readAt) unread.value++
        }
      }),
      listen<{ unread: number }>('notifications:changed', (event) => {
        if (gen !== generation) return
        unread.value = event.payload.unread
        // Another window or the backend changed rows; resync the list.
        void load().catch(() => {})
      }),
      listen<{ route: string }>('notifications:open', (event) => {
        void openRoute(event.payload.route)
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

  // A tap that launched the app fired before any listener existed; the
  // backend holds that route until somebody asks. Taken once.
  try {
    const held = await invoke<string | null>('notifications_pending_open')
    if (gen === generation) await openRoute(held)
  } catch (e) {
    console.warn('[notifications] pending open failed:', e)
  }
}

/** Drop listeners and cached rows so the next profile starts clean. */
function teardown(): void {
  generation++
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
async function registerPushIfEnabled(pushEnabled: boolean): Promise<void> {
  if (!isMobilePlatform || !pushEnabled) return
  try {
    await registerPush()
  } catch (e) {
    console.warn('[notifications] push registration failed:', e)
    await refreshPushStatus()
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
    registerPushIfEnabled,
  }
}
