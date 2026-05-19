import { onMounted, onUnmounted, ref, watch } from 'vue'

import { useSettings } from './useSettings'

type Theme = 'light' | 'dark' | 'system'

const SETTING_KEY = 'ui.theme'
const LEGACY_LOCALSTORAGE_KEY = 'alexandria-theme'

const theme = ref<Theme>('system')
let initialized = false
let mediaQuery: MediaQueryList | null = null
let mediaHandler: (() => void) | null = null

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
 * Reads from localStorage as a hint (no async IPC available pre-mount,
 * and the profile may not even be unlocked yet). Once a profile is
 * active, `initThemeFromSettings()` resyncs with the per-profile
 * settings store.
 */
export function initTheme() {
  if (initialized) return
  initialized = true

  const stored = localStorage.getItem(LEGACY_LOCALSTORAGE_KEY) as Theme | null
  if (stored) theme.value = stored
  applyTheme(theme.value)
}

/** Hydrate from the per-profile settings store. Call once after profile unlock. */
export async function initThemeFromSettings(): Promise<void> {
  const { entries, initialize } = useSettings()
  await initialize()
  const found = entries.value.find((e) => e.key === SETTING_KEY)
  if (found) {
    theme.value = found.current_value as Theme
    applyTheme(theme.value)
  }
  // Migrate any pre-multi-user localStorage value into the settings
  // store the first time we see a fresh profile.
  const legacy = localStorage.getItem(LEGACY_LOCALSTORAGE_KEY) as Theme | null
  if (legacy && found?.is_default) {
    const { setSetting } = useSettings()
    await setSetting(SETTING_KEY, legacy)
    theme.value = legacy
    applyTheme(legacy)
  }
}

export function useTheme() {
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

  watch(theme, (val) => {
    // Keep localStorage in sync so initTheme() (synchronous, runs
    // before profile unlock) shows the same theme on next launch.
    localStorage.setItem(LEGACY_LOCALSTORAGE_KEY, val)
    applyTheme(val)
    // Persist to the per-profile settings store; awaiting would
    // turn this into an async callback and break the watch contract,
    // so fire-and-forget.
    void useSettings().setSetting(SETTING_KEY, val)
  })

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
