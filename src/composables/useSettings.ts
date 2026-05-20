// Reactive mirror of the per-profile settings store.
//
// The canonical registry lives on the backend
// (`src-tauri/src/settings/registry.rs`). The frontend never owns a
// list of keys; it pulls the whole registry via `list_settings` on
// mount and listens for `settings-changed` events so multiple
// windows + sync deliveries stay coherent.
//
// Code wanting a single typed setting should call
// `useSetting('ui.theme')` for a `Ref<string>` that auto-updates
// (and writes back via `setSetting`).

import { computed, readonly, ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { useLocalApi } from './useLocalApi'

const { invoke } = useLocalApi()

export type SettingScope = 'sync' | 'device'
export type SettingKind = 'bool' | 'int' | 'float' | 'string' | 'json'

export interface SettingEntry {
  key: string
  scope: SettingScope
  category: string
  label: string
  description: string
  kind: SettingKind
  default_value: string
  current_value: string
  is_default: boolean
}

// Module-level singleton state.
const entries = ref<SettingEntry[]>([])
const ready = ref(false)
const loading = ref(false)
let unlisten: UnlistenFn | null = null

/**
 * Clear the local cache. Called when the active profile is locked —
 * keeps composables that derive from `entries` (theme, shortcuts,
 * recents, sentinel toggles) from showing the previous profile's
 * values while the picker is up.
 */
export function clearSettingsCache(): void {
  entries.value = []
  ready.value = false
}

// `entries` keyed by `key` for O(1) lookup.
const byKey = computed(() => {
  const out = new Map<string, SettingEntry>()
  for (const e of entries.value) out.set(e.key, e)
  return out
})

async function refresh(): Promise<void> {
  entries.value = await invoke<SettingEntry[]>('list_settings')
}

async function initialize(): Promise<void> {
  if (ready.value || loading.value) return
  loading.value = true
  try {
    await refresh()

    // Listen for in-process writes from other windows + inbound sync.
    if (!unlisten) {
      unlisten = await listen<{ key: string | null }>('settings-changed', async () => {
        try {
          await refresh()
        } catch {
          // Refresh may fail if the active profile was locked between
          // emit and handler. Treat that as "no overrides".
          entries.value = []
        }
      })
    }
    ready.value = true
  } finally {
    loading.value = false
  }
}

async function setSetting(key: string, value: string): Promise<void> {
  await invoke('set_setting', { key, value })
  // Optimistic local update so callers see the new value immediately.
  const found = entries.value.find((e) => e.key === key)
  if (found) {
    found.current_value = value
    found.is_default = false
  }
}

async function resetSetting(key: string): Promise<void> {
  await invoke('reset_setting', { key })
  const found = entries.value.find((e) => e.key === key)
  if (found) {
    found.current_value = found.default_value
    found.is_default = true
  }
}

// ── Type coercion helpers ───────────────────────────────────────

function decode(entry: SettingEntry): unknown {
  switch (entry.kind) {
    case 'bool':
      return entry.current_value === 'true' || entry.current_value === '1'
    case 'int':
      return Number.parseInt(entry.current_value, 10)
    case 'float':
      return Number.parseFloat(entry.current_value)
    case 'json':
      try {
        return JSON.parse(entry.current_value)
      } catch {
        return null
      }
    default:
      return entry.current_value
  }
}

function encode(value: unknown, kind: SettingKind): string {
  switch (kind) {
    case 'bool':
      return value ? 'true' : 'false'
    case 'int':
    case 'float':
      return String(value)
    case 'json':
      return JSON.stringify(value)
    default:
      return String(value ?? '')
  }
}

export function useSettings() {
  return {
    entries: readonly(entries),
    byKey,
    ready: readonly(ready),
    loading: readonly(loading),
    initialize,
    refresh,
    setSetting,
    resetSetting,
  }
}

/**
 * Two-way reactive ref for a single setting key.
 *
 * Reads the current value (coerced to its declared type) and writes
 * back to the backend on mutation. The ref tracks `settings-changed`
 * events automatically, so sync deliveries from another device
 * propagate without manual refresh.
 *
 * @example
 *   const theme = useSetting<string>('ui.theme')
 *   theme.value = 'dark'  // persists + emits settings-changed
 */
export function useSetting<T>(key: string): {
  value: T | null
  ref: import('vue').Ref<T | null>
  set: (v: T) => Promise<void>
  reset: () => Promise<void>
} {
  const local = ref<T | null>(null) as import('vue').Ref<T | null>

  const sync = () => {
    const entry = byKey.value.get(key)
    if (!entry) {
      local.value = null
      return
    }
    local.value = decode(entry) as T
  }

  // Auto-update on registry changes.
  watch(entries, sync, { immediate: true, deep: true })

  async function set(v: T) {
    const entry = byKey.value.get(key)
    if (!entry) {
      console.warn(`[useSetting] unknown key: ${key}`)
      return
    }
    await setSetting(key, encode(v, entry.kind))
    local.value = v
  }

  async function reset() {
    await resetSetting(key)
    sync()
  }

  return {
    get value(): T | null {
      return local.value
    },
    set value(v: T | null) {
      if (v === null) {
        void reset()
      } else {
        void set(v)
      }
    },
    ref: local,
    set,
    reset,
  }
}
