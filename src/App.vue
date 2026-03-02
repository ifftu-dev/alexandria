<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import AppLayout from '@/layouts/AppLayout.vue'
import BlankLayout from '@/layouts/BlankLayout.vue'
import { useAuth } from '@/composables/useAuth'
import { initTheme } from '@/composables/useTheme'

// Apply stored theme immediately (before first render)
initTheme()

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
</script>

<template>
  <div v-if="!ready" class="flex items-center justify-center h-dvh bg-background safe-area-top">
    <div class="text-center">
      <div class="w-8 h-8 border-2 border-primary border-t-transparent rounded-full animate-spin mx-auto mb-3" />
      <p class="text-sm text-muted-foreground">Initializing...</p>
    </div>
  </div>
  <component v-else :is="layout">
    <router-view />
  </component>
</template>
