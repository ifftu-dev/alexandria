import { computed, onMounted, onUnmounted, watch } from 'vue'

import { useSetting, useSettings } from './useSettings'

type Theme = 'light' | 'dark' | 'system'

const SETTING_KEY = 'ui.theme'
// Pre-unlock cache — only used by `initTheme()` to avoid a startup
// theme flash before any profile is unlocked. The per-profile
// `app_settings` store (via `useSetting`) is the source of truth.
const PRE_UNLOCK_CACHE_KEY = 'alexandria-theme-pre-unlock'

let initialized = false
let mediaQuery: MediaQueryList | null = null
let mediaHandler: (() => void) | null = null

// Reactive ref bound to the per-profile setting. When the active
// profile changes (lock + unlock) or a sync delivery updates the
// theme, this ref tracks automatically via the `useSettings` event
// bridge.
const setting = useSetting<string>(SETTING_KEY)

function readTheme(): Theme {
  const v = setting.ref.value
  if (v === 'light' || v === 'dark' || v === 'system') return v
  return 'system'
}

function applyTheme(t: Theme) {
  const isDark =
    t === 'dark' || (t === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
  document.documentElement.classList.toggle('dark', isDark)
  // Clear any inline background-color set by the startup flash-prevention script.
  document.documentElement.style.removeProperty('background-color')
}

/**
 * Apply the last-known theme synchronously before the first render.
 *
 * Uses a localStorage hint (no async IPC is available at this point
 * and no profile has been unlocked yet). The cache is intentionally
 * NOT keyed per profile because the picker has no concept of an
 * active user. Once a profile unlocks, `useSetting('ui.theme')`
 * (above) takes over as the reactive source of truth.
 */
export function initTheme() {
  if (initialized) return
  initialized = true

  const stored = localStorage.getItem(PRE_UNLOCK_CACHE_KEY) as Theme | null
  if (stored === 'light' || stored === 'dark' || stored === 'system') {
    applyTheme(stored)
  } else {
    applyTheme('system')
  }
}

/**
 * Reconcile with the active profile's theme. Idempotent — call
 * after profile unlock from `App.vue::hydrateProfileScopedState`.
 *
 * Crucially, this does NOT copy any pre-unlock localStorage value
 * into the profile's settings store. Each profile owns its theme
 * independently; the cache is only ever read, never written by this
 * function. Writes only happen when the user explicitly changes the
 * theme via `setTheme`/`toggleTheme`.
 */
export async function initThemeFromSettings(): Promise<void> {
  await useSettings().initialize()
  applyTheme(readTheme())
}

export function useTheme() {
  const theme = computed<Theme>({
    get: () => readTheme(),
    set: (t: Theme) => {
      void setting.set(t)
    },
  })

  onMounted(() => {
    if (!initialized) initTheme()

    mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
    mediaHandler = () => {
      if (theme.value === 'system') applyTheme('system')
    }
    mediaQuery.addEventListener('change', mediaHandler)
  })

  onUnmounted(() => {
    if (mediaQuery && mediaHandler) {
      mediaQuery.removeEventListener('change', mediaHandler)
      mediaQuery = null
      mediaHandler = null
    }
  })

  // React to any change in the per-profile setting — explicit user
  // toggle, profile switch, or inbound sync from another device.
  watch(
    setting.ref,
    (val) => {
      const t: Theme =
        val === 'light' || val === 'dark' || val === 'system' ? val : 'system'
      // Refresh the pre-unlock cache so the *next* launch paints
      // the same theme this profile is currently using.
      localStorage.setItem(PRE_UNLOCK_CACHE_KEY, t)
      applyTheme(t)
    },
    { immediate: true },
  )

  function toggleTheme() {
    const order: Theme[] = ['light', 'dark', 'system']
    const idx = order.indexOf(theme.value)
    theme.value = order[(idx + 1) % order.length] as Theme
  }

  function setTheme(t: Theme) {
    theme.value = t
  }

  return { theme, toggleTheme, setTheme }
}
