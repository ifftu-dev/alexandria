import { createApp } from 'vue'
import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import App from './App.vue'
import router from './router'
import { i18n } from './i18n'
import { initMotion } from './composables/useMotion'
import { initLocale } from './composables/useLocale'
import { showOnFirstPaint } from './composables/launchWindow'
import './assets/fonts.css'
// Non-Latin script fonts (self-hosted via @fontsource; each @font-face is
// unicode-range-scoped so Latin users don't download them). Covers the six
// launch locales whose scripts Inter can't render.
import '@fontsource/noto-sans-sc/400.css'
import '@fontsource/noto-sans-sc/500.css'
import '@fontsource/noto-sans-devanagari/400.css'
import '@fontsource/noto-sans-devanagari/500.css'
import '@fontsource/noto-sans-bengali/400.css'
import '@fontsource/noto-sans-bengali/500.css'
import '@fontsource/noto-sans-telugu/400.css'
import '@fontsource/noto-sans-telugu/500.css'
import '@fontsource/noto-nastaliq-urdu/400.css'
import '@fontsource/noto-nastaliq-urdu/600.css'
import './assets/css/main.css'

// Apply the last-known language + text direction before first render, awaiting
// the cached locale's messages so setup-time t() calls resolve correctly.
initMotion()
await initLocale()

const app = createApp(App)
app.use(router)
app.use(i18n)
app.mount('#app')

// The window was created hidden; show it with the themed first frame.
if (isTauri()) showOnFirstPaint(() => { void getCurrentWebviewWindow().show().catch(() => {}) })
