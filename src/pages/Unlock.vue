<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import { useAuth } from '@/composables/useAuth'
import { AppButton } from '@/components/ui'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import Starfield from '@/components/auth/Starfield.vue'

const router = useRouter()
const { unlockVault } = useAuth()

const password = ref('')
const error = ref('')
const unlocking = ref(false)

// Progress tracking from Rust events
const progressLines = ref<string[]>([])
let unlisten: UnlistenFn | null = null

onMounted(async () => {
  unlisten = await listen<{ step: string; detail: string }>('vault-progress', (event) => {
    progressLines.value.push(event.payload.detail)
  })
})

onUnmounted(() => {
  if (unlisten) unlisten()
})

async function unlock() {
  if (!password.value) {
    error.value = 'Please enter your password.'
    return
  }

  unlocking.value = true
  error.value = ''
  progressLines.value = []

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
    progressLines.value = []
  } finally {
    unlocking.value = false
  }
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter') unlock()
}
</script>

<template>
  <div class="min-h-screen flex items-center justify-center bg-[rgb(var(--color-background))] p-4 sm:p-8 relative overflow-hidden">
    <Starfield />

    <div class="w-full max-w-md relative z-10">

      <!-- ============================================ -->
      <!-- IDLE STATE — password entry                  -->
      <!-- ============================================ -->
      <div v-if="!unlocking">
        <div class="text-center mb-8">
          <!-- Alexandria logo -->
          <div class="relative w-14 h-14 mx-auto mb-4">
            <div class="absolute inset-0 rounded-full bg-[rgb(var(--color-primary)/0.05)] animate-ping" style="animation-duration: 4s;" />
            <div class="relative w-14 h-14 flex items-center justify-center">
              <svg class="w-10 h-10 text-[rgb(var(--color-primary))]" viewBox="0 0 32 32" fill="none">
                <path d="M16 2L4 8v16l12 6 12-6V8L16 2z" stroke="currentColor" stroke-width="2" fill="none" />
                <path d="M16 8v16M8 12l8 4 8-4" stroke="currentColor" stroke-width="2" />
              </svg>
            </div>
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
            :disabled="!password"
            @click="unlock"
          >
            Unlock
          </AppButton>
        </div>

        <p class="text-center text-xs text-[rgb(var(--color-muted-foreground))] mt-4 italic tracking-wide">
          I am, because we all are
        </p>
      </div>

      <!-- ============================================ -->
      <!-- UNLOCKING STATE — animated progress          -->
      <!-- ============================================ -->
      <div v-else class="text-center">
        <!-- Orbital animation (same as onboarding) -->
        <div class="relative w-24 h-24 mx-auto mb-6">
          <div class="absolute inset-0 rounded-full border border-[rgb(var(--color-border)/0.4)]" />
          <div class="absolute inset-0 animate-spin" style="animation-duration: 3s;">
            <div class="absolute top-0 left-1/2 -translate-x-1/2 -translate-y-1/2 w-2.5 h-2.5 rounded-full bg-[rgb(var(--color-primary))]" />
          </div>
          <div class="absolute inset-3 rounded-full border border-[rgb(var(--color-border)/0.3)]" />
          <div class="absolute inset-3 animate-spin" style="animation-duration: 2s; animation-direction: reverse;">
            <div class="absolute top-0 left-1/2 -translate-x-1/2 -translate-y-1/2 w-2 h-2 rounded-full bg-[rgb(var(--color-primary)/0.7)]" />
          </div>
          <div class="absolute inset-6 rounded-full bg-[rgb(var(--color-primary)/0.1)] flex items-center justify-center">
            <svg class="w-6 h-6 text-[rgb(var(--color-primary))] animate-pulse" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M13.5 10.5V6.75a4.5 4.5 0 119 0v3.75M3.75 21.75h10.5a2.25 2.25 0 002.25-2.25v-6.75a2.25 2.25 0 00-2.25-2.25H3.75a2.25 2.25 0 00-2.25 2.25v6.75a2.25 2.25 0 002.25 2.25z" />
            </svg>
          </div>
        </div>

        <h2 class="text-xl font-bold mb-1 text-[rgb(var(--color-foreground))]">
          Unlocking Vault
        </h2>
        <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
          Decrypting your identity and deriving keys...
        </p>

        <!-- Live log output -->
        <div class="card p-4 text-left mb-4">
          <div class="font-mono text-xs space-y-1.5 min-h-[80px]">
            <div
              v-for="(line, i) in progressLines"
              :key="i"
              class="flex items-start gap-2 text-[rgb(var(--color-muted-foreground))]"
              :class="{ 'text-[rgb(var(--color-primary))]': i === progressLines.length - 1 }"
            >
              <svg v-if="i < progressLines.length - 1" class="w-3 h-3 mt-0.5 shrink-0 text-[rgb(var(--color-success))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="3">
                <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
              </svg>
              <div v-else class="w-3 h-3 mt-0.5 shrink-0 border-2 border-[rgb(var(--color-primary))] border-t-transparent rounded-full animate-spin" />
              <span>{{ line }}</span>
            </div>
            <div v-if="progressLines.length === 0" class="flex items-start gap-2 text-[rgb(var(--color-primary))]">
              <div class="w-3 h-3 mt-0.5 shrink-0 border-2 border-[rgb(var(--color-primary))] border-t-transparent rounded-full animate-spin" />
              <span>Initializing...</span>
            </div>
          </div>
        </div>

        <p class="text-xs text-[rgb(var(--color-muted-foreground))] italic tracking-wide">
          I am, because we all are
        </p>
      </div>

    </div>
  </div>
</template>
