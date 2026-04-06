import { ref, watch, onMounted, onUnmounted } from 'vue'

type Theme = 'light' | 'dark' | 'system'

const STORAGE_KEY = 'alexandria-theme'
const theme = ref<Theme>('system')
let initialized = false
let mediaQuery: MediaQueryList | null = null
let mediaHandler: (() => void) | null = null

function applyTheme(t: Theme) {
  const isDark =
    t === 'dark' || (t === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
  document.documentElement.classList.toggle('dark', isDark)
  // Clear any inline background-color set by the startup flash-prevention script
  document.documentElement.style.removeProperty('background-color')
}

/** Call once at app startup (from App.vue) to eagerly apply the stored theme. */
export function initTheme() {
  if (initialized) return
  initialized = true

  const stored = localStorage.getItem(STORAGE_KEY) as Theme | null
  if (stored) theme.value = stored
  applyTheme(theme.value)
}

export function useTheme() {
  // Start listening for OS theme changes while this component is mounted.
  onMounted(() => {
    // Ensure theme is applied even if initTheme() wasn't called
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
    localStorage.setItem(STORAGE_KEY, val)
    applyTheme(val)
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
