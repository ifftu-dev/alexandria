<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'

const { invoke } = useLocalApi()
const router = useRouter()

const password = ref('')
const error = ref('')
const unlocking = ref(false)

async function unlock() {
  if (!password.value) {
    error.value = 'Please enter your password.'
    return
  }

  unlocking.value = true
  error.value = ''

  try {
    await invoke<{
      stake_address: string
      payment_address: string
      has_mnemonic_backup: boolean
    }>('unlock_vault', { password: password.value })

    router.replace('/home')
  } catch (e) {
    const msg = String(e)
    if (msg.includes('incorrect password') || msg.includes('IncorrectPassword')) {
      error.value = 'Incorrect password. Please try again.'
    } else {
      error.value = `Failed to unlock: ${msg}`
    }
    password.value = ''
  } finally {
    unlocking.value = false
  }
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    unlock()
  }
}
</script>

<template>
  <div class="min-h-screen flex items-center justify-center bg-[rgb(var(--color-background))] p-8">
    <div class="w-full max-w-md">
      <div class="text-center mb-8">
        <div class="w-14 h-14 rounded-full bg-[rgb(var(--color-primary)/0.1)] flex items-center justify-center mx-auto mb-4">
          <svg class="w-7 h-7 text-[rgb(var(--color-primary))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
          </svg>
        </div>
        <h1 class="text-2xl font-bold text-[rgb(var(--color-foreground))]">Welcome Back</h1>
        <p class="text-sm text-[rgb(var(--color-muted-foreground))] mt-1">
          Enter your password to unlock Alexandria.
        </p>
      </div>

      <div class="card p-6">
        <div class="mb-4">
          <label class="block text-xs font-medium text-[rgb(var(--color-muted-foreground))] mb-1.5">
            Password
          </label>
          <input
            v-model="password"
            type="password"
            placeholder="Enter your vault password"
            class="w-full px-3 py-2.5 text-sm rounded-md border border-[rgb(var(--color-border))] bg-[rgb(var(--color-background))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-ring))]"
            autofocus
            @keydown="handleKeydown"
          >
        </div>

        <p v-if="error" class="text-sm text-[rgb(var(--color-error))] mb-4">{{ error }}</p>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-[rgb(var(--color-primary))] text-[rgb(var(--color-primary-foreground))] hover:bg-[rgb(var(--color-primary-hover))] transition-colors disabled:opacity-50"
          :disabled="unlocking || !password"
          @click="unlock"
        >
          <span v-if="unlocking" class="flex items-center justify-center gap-2">
            <span class="w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin" />
            Unlocking...
          </span>
          <span v-else>Unlock</span>
        </button>
      </div>

      <p class="text-center text-xs text-[rgb(var(--color-muted-foreground))] mt-4">
        Your vault is encrypted locally on this device.
      </p>
    </div>
  </div>
</template>
