<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppLayout from '@/layouts/AppLayout.vue'
import BlankLayout from '@/layouts/BlankLayout.vue'
import { useAuth } from '@/composables/useAuth'

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
})
</script>

<template>
  <div v-if="!ready" class="flex items-center justify-center h-screen bg-[rgb(var(--color-background))]">
    <div class="text-center">
      <div class="w-8 h-8 border-2 border-[rgb(var(--color-primary))] border-t-transparent rounded-full animate-spin mx-auto mb-3" />
      <p class="text-sm text-[rgb(var(--color-muted-foreground))]">Initializing...</p>
    </div>
  </div>
  <component v-else :is="layout">
    <router-view />
  </component>
</template>
