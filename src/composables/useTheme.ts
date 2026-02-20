import { ref, watch, onMounted } from 'vue'

type Theme = 'light' | 'dark' | 'system'

const theme = ref<Theme>('system')

function applyTheme(t: Theme) {
  const isDark =
    t === 'dark' || (t === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)

  document.documentElement.classList.toggle('dark', isDark)
}

export function useTheme() {
  onMounted(() => {
    const stored = localStorage.getItem('alexandria-theme') as Theme | null
    if (stored) theme.value = stored
    applyTheme(theme.value)

    // Listen for OS theme changes when using "system"
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
      if (theme.value === 'system') applyTheme('system')
    })
  })

  watch(theme, (val) => {
    localStorage.setItem('alexandria-theme', val)
    applyTheme(val)
  })

  function toggleTheme() {
    const order: Theme[] = ['light', 'dark', 'system']
    const idx = order.indexOf(theme.value)
    theme.value = order[(idx + 1) % order.length] as Theme
  }

  return { theme, toggleTheme }
}
