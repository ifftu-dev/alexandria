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

const layout = computed(() => {
  const meta = route.meta?.layout as string | undefined
  if (meta === 'blank') return BlankLayout
  return AppLayout
})

onMounted(async () => {
  try {
    // Check if a Stronghold vault exists
    const vaultExists = await invoke<boolean>('check_vault_exists')

    if (!vaultExists) {
      // No vault — first-time setup
      if (route.name !== 'onboarding') {
        router.replace('/onboarding')
      }
    } else {
      // Vault exists — check if we have an identity in DB (meaning vault was
      // already unlocked this session, e.g., navigating back)
      const wallet = await invoke<{ stake_address: string; payment_address: string; has_mnemonic_backup: boolean } | null>('get_wallet_info')

      if (wallet && route.name !== 'unlock') {
        // Wallet row exists, but we might not be unlocked yet.
        // The unlock page will handle the actual decryption.
        // If we're on a protected route, redirect to unlock.
        if (route.meta?.layout === 'app') {
          router.replace('/unlock')
        }
      } else if (!wallet) {
        // Vault exists but no DB identity — edge case (maybe DB was reset).
        // Send to unlock which will re-create the identity row.
        if (route.name !== 'unlock') {
          router.replace('/unlock')
        }
      }
    }
  } catch {
    // On error, default to onboarding
    if (route.name !== 'onboarding') {
      router.replace('/onboarding')
    }
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
