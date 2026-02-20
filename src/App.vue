<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppLayout from '@/layouts/AppLayout.vue'
import BlankLayout from '@/layouts/BlankLayout.vue'
import { useLocalApi } from '@/composables/useLocalApi'

const route = useRoute()
const router = useRouter()
const { invoke } = useLocalApi()

const initialized = ref(false)
const hasWallet = ref(false)

const layout = computed(() => {
  const meta = route.meta?.layout as string | undefined
  if (meta === 'blank') return BlankLayout
  return AppLayout
})

onMounted(async () => {
  try {
    const wallet = await invoke<{ stake_address: string; payment_address: string; has_mnemonic_backup: boolean } | null>('get_wallet_info')
    hasWallet.value = wallet !== null
  } catch {
    hasWallet.value = false
  }

  // Redirect to onboarding if no wallet exists
  if (!hasWallet.value && route.name !== 'onboarding') {
    router.replace('/onboarding')
  }

  initialized.value = true
})
</script>

<template>
  <div v-if="!initialized" class="flex items-center justify-center h-screen bg-[rgb(var(--color-background))]">
    <div class="text-center">
      <div class="w-8 h-8 border-2 border-[rgb(var(--color-primary))] border-t-transparent rounded-full animate-spin mx-auto mb-3" />
      <p class="text-sm text-[rgb(var(--color-muted-foreground))]">Initializing...</p>
    </div>
  </div>
  <component v-else :is="layout">
    <router-view />
  </component>
</template>
