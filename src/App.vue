<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { invoke } from '@tauri-apps/api/core'
import AppLayout from '@/layouts/AppLayout.vue'
import BlankLayout from '@/layouts/BlankLayout.vue'
import { useProfiles, onProfileLocked, onProfileReady } from '@/composables/useProfiles'
import { initTheme, initThemeFromSettings } from '@/composables/useTheme'
import { initShortcutsFromSettings } from '@/composables/useKeyboardShortcuts'
import { initOmniRecentsFromSettings } from '@/composables/useOmniSearch'
import { initSentinelFlagsFromSettings } from '@/composables/useSentinel'
import { clearSettingsCache, useSettings } from '@/composables/useSettings'
import { isMac } from '@/composables/usePlatform'

// Apply stored theme immediately (before first render)
initTheme()

// Cmd/Ctrl + vertical scroll → horizontal scroll in overflow-x containers.
function onWheel(e: WheelEvent) {
  const mod = isMac ? e.metaKey : e.ctrlKey
  if (!mod) return
  // Only act on vertical wheel deltas.
  if (e.deltaY === 0) return
  const target = e.target as HTMLElement | null
  if (!target) return
  const scroller = target.closest('.overflow-x-auto, .scrollbar-thin') as HTMLElement | null
  if (!scroller) return
  // If the container can scroll horizontally, redirect.
  if (scroller.scrollWidth <= scroller.clientWidth) return
  e.preventDefault()
  scroller.scrollLeft += e.deltaY
}

const route = useRoute()
const router = useRouter()
const { initialize } = useProfiles()

const ready = ref(false)

const layout = computed(() => {
  const meta = route.meta?.layout as string | undefined
  if (meta === 'blank') return BlankLayout
  return AppLayout
})

/**
 * Hydrate all per-profile settings consumers from the backend.
 * Called once a profile is unlocked. Safe to call repeatedly —
 * each `initXFromSettings` is idempotent.
 */
onProfileReady(() => {
  hydrateProfileScopedState()
  // Unlock just blurred/destroyed the password field; clear any leaked
  // Secure Event Input now (no focus event fires since the window is
  // already key).
  void invoke('release_secure_input').catch(() => {})
})
onProfileLocked(() => {
  // Drop the in-memory settings cache so the picker (and the next
  // profile that unlocks) does not flash the previously-active
  // profile's preferences.
  clearSettingsCache()
})

async function hydrateProfileScopedState() {
  try {
    await useSettings().initialize()
    await Promise.all([
      initThemeFromSettings(),
      initShortcutsFromSettings(),
      initOmniRecentsFromSettings(),
      initSentinelFlagsFromSettings(),
    ])
  } catch (e) {
    console.warn('[App] settings hydration failed:', e)
  }
}

// macOS WKWebView leaks Secure Event Input after a password field is
// focused then navigated away from, which suppresses global hotkey tools
// (CGEventTaps) while Alexandria is foreground. WebKit re-asserts the
// leaked state each time the window becomes key, so clear it on every
// focus — but never while a password field is genuinely focused, so real
// password entry stays protected.
function onWindowFocus() {
  const el = document.activeElement as HTMLInputElement | null
  if (el && el.tagName === 'INPUT' && el.type === 'password') return
  void invoke('release_secure_input').catch(() => {})
}

onMounted(async () => {
  document.addEventListener('wheel', onWheel, { passive: false })
  window.addEventListener('focus', onWindowFocus)

  try {
    const state = await initialize()

    if (state === 'onboarding' && route.name !== 'onboarding') {
      router.replace('/onboarding')
    } else if (state === 'picker' && route.name !== 'profiles' && route.name !== 'onboarding') {
      router.replace('/profiles')
    } else if (state === 'ready') {
      await hydrateProfileScopedState()
    }
  } catch {
    if (route.name !== 'onboarding' && route.name !== 'profiles') {
      router.replace('/profiles')
    }
  }

  ready.value = true

  // Show the window now that the frontend is rendered and themed
  getCurrentWebviewWindow().show()
})

onUnmounted(() => {
  document.removeEventListener('wheel', onWheel)
  window.removeEventListener('focus', onWindowFocus)
})
</script>

<template>
  <div v-if="!ready" class="flex items-center justify-center h-full bg-background safe-area-top">
    <div class="text-center">
      <div class="w-8 h-8 border-2 border-primary border-t-transparent rounded-full animate-spin mx-auto mb-3" />
      <p class="text-sm text-muted-foreground">Initializing...</p>
    </div>
  </div>
  <component v-else :is="layout">
    <router-view />
  </component>
</template>
