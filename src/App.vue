<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import AppLayout from '@/layouts/AppLayout.vue'
import BlankLayout from '@/layouts/BlankLayout.vue'
import { useAuth } from '@/composables/useAuth'
import { initTheme } from '@/composables/useTheme'
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
const { initialize } = useAuth()

const ready = ref(false)

const layout = computed(() => {
  const meta = route.meta?.layout as string | undefined
  if (meta === 'blank') return BlankLayout
  return AppLayout
})

onMounted(async () => {
  document.addEventListener('wheel', onWheel, { passive: false })

  try {
    const state = await initialize()

    if (state === 'onboarding' && route.name !== 'onboarding') {
      router.replace('/onboarding')
    } else if (state === 'unlock' && route.name !== 'unlock') {
      router.replace('/unlock')
    }
  } catch {
    if (route.name !== 'onboarding') {
      router.replace('/onboarding')
    }
  }

  ready.value = true

  // Show the window now that the frontend is rendered and themed
  getCurrentWebviewWindow().show()
})

onUnmounted(() => {
  document.removeEventListener('wheel', onWheel)
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
