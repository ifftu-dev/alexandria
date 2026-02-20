<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useAuth } from '@/composables/useAuth'
import { AppButton } from '@/components/ui'

const router = useRouter()
const { unlockVault } = useAuth()

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
    await unlockVault(password.value)
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
  if (e.key === 'Enter') unlock()
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
          <label class="label text-xs text-[rgb(var(--color-muted-foreground))]">Password</label>
          <input
            v-model="password"
            type="password"
            placeholder="Enter your vault password"
            class="input"
            autofocus
            @keydown="handleKeydown"
          >
        </div>

        <p v-if="error" class="text-sm text-[rgb(var(--color-error))] mb-4">{{ error }}</p>

        <AppButton
          class="w-full"
          :loading="unlocking"
          :disabled="!password"
          @click="unlock"
        >
          Unlock
        </AppButton>
      </div>

      <p class="text-center text-xs text-[rgb(var(--color-muted-foreground))] mt-4">
        Your vault is encrypted locally on this device.
      </p>
    </div>
  </div>
</template>
