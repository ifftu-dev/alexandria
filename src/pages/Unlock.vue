<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import { useAuth } from '@/composables/useAuth'
import {
  biometricCredentialExists,
  biometricSupported,
  getVaultPasswordViaBiometric,
  storeVaultPasswordForBiometric,
} from '@/composables/useBiometricVault'
import { AppButton } from '@/components/ui'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import Starfield from '@/components/auth/Starfield.vue'

const router = useRouter()
const { unlockVault, resetLocalWallet } = useAuth()

const password = ref('')
const error = ref('')
const unlocking = ref(false)
const recovering = ref(false)
const showRecoverConfirm = ref(false)
const biometricAvailable = ref(false)
const hasBiometricCredential = ref(false)
const biometricLoading = ref(false)
const autoBiometricTried = ref(false)

// Progress tracking from Rust events
const progressLines = ref<string[]>([])
let unlisten: UnlistenFn | null = null

onMounted(async () => {
  unlisten = await listen<{ step: string; detail: string }>('vault-progress', (event) => {
    progressLines.value.push(event.payload.detail)
  })

  try {
    const [supported, hasCredential] = await Promise.all([
      biometricSupported(),
      biometricCredentialExists(),
    ])
    biometricAvailable.value = supported
    hasBiometricCredential.value = hasCredential

    if (supported && hasCredential && !autoBiometricTried.value) {
      autoBiometricTried.value = true
      await unlockWithBiometric(true)
    }
  } catch {
    biometricAvailable.value = false
    hasBiometricCredential.value = false
  }
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
    try {
      const mode = await storeVaultPasswordForBiometric(password.value)
      hasBiometricCredential.value = true
      if (mode === 'session') {
        console.info('Biometric unlock running in session-only mode (macOS entitlement missing).')
      }
    } catch (setupError) {
      hasBiometricCredential.value = false
      console.warn('Biometric credential setup failed after password unlock:', setupError)
    }
    router.replace('/home')
  } catch (e) {
    const msg = String(e)
    if (msg.includes('incorrect password') || msg.includes('IncorrectPassword')) {
      error.value = 'That password does not match the local vault on this device. Please try again.'
    } else if (msg.includes('salt file corrupted') || msg.includes('integrity check failed')) {
      error.value = 'This desktop vault looks out of sync with its local unlock data. If you recently reset this device, clear the local desktop vault and create or restore it again.'
    } else {
      error.value = `Failed to unlock: ${msg}`
    }
    password.value = ''
    progressLines.value = []
  } finally {
    unlocking.value = false
  }
}

async function unlockWithBiometric(auto = false) {
  biometricLoading.value = true
  if (!auto) error.value = ''
  progressLines.value = []
  try {
    let biometricPassword: string
    try {
      biometricPassword = await getVaultPasswordViaBiometric('Authenticate to unlock Alexandria vault')
    } catch (e) {
      hasBiometricCredential.value = await biometricCredentialExists()
      const msg = String(e)
      if (msg.includes('itemNotFound') || msg.includes('not found')) {
        error.value = 'Biometric unlock is not enabled on this device yet. Unlock once with password to enable it.'
      } else if (msg.includes('-34018')) {
        error.value = 'Biometric credential could not be stored due to macOS keychain entitlement (-34018). Run a bundled/signed app build and enable biometrics in Settings.'
      } else if (!auto || (!msg.includes('userCancel') && !msg.includes('cancel'))) {
        error.value = `Biometric unlock failed: ${msg}`
      }
      return
    }

    try {
      await unlockVault(biometricPassword)
    } catch (e) {
      const msg = String(e)
      if (msg.includes('incorrect password') || msg.includes('IncorrectPassword')) {
        error.value = 'Biometric unlock credential is out of date. Unlock with password once to refresh it.'
      } else if (msg.includes('salt file corrupted') || msg.includes('integrity check failed')) {
        error.value = 'The local desktop vault files look out of sync. Unlock with password after resetting the local desktop vault.'
      } else {
        error.value = `Biometric unlock failed: ${msg}`
      }
      return
    }

    router.replace('/home')
  } finally {
    biometricLoading.value = false
  }
}

function promptRecoverWallet() {
  showRecoverConfirm.value = true
}

async function confirmRecoverWallet() {
  showRecoverConfirm.value = false
  recovering.value = true
  error.value = ''
  progressLines.value = []

  try {
    await resetLocalWallet()
    password.value = ''
    router.replace({ name: 'onboarding', query: { mode: 'import' } })
  } catch (e) {
    error.value = `Couldn't prepare wallet recovery: ${String(e)}`
  } finally {
    recovering.value = false
  }
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter') unlock()
}
</script>

<template>
  <div class="min-h-screen flex items-center justify-center bg-background p-4 sm:p-8 relative overflow-hidden">
    <Starfield />

    <div class="w-full max-w-md relative z-10">

      <!-- ============================================ -->
      <!-- IDLE STATE — password entry                  -->
      <!-- ============================================ -->
      <div v-if="!unlocking">
        <div class="text-center mb-8">
          <!-- Alexandria logo -->
          <div class="relative w-14 h-14 mx-auto mb-4">
            <div class="absolute inset-0 rounded-full bg-primary/5 animate-ping" style="animation-duration: 4s;" />
            <div class="relative w-14 h-14 flex items-center justify-center">
              <svg class="w-10 h-10 text-primary" viewBox="0 0 32 32" fill="none">
                <path d="M16 2L4 8v16l12 6 12-6V8L16 2z" stroke="currentColor" stroke-width="2" fill="none" />
                <path d="M16 8v16M8 12l8 4 8-4" stroke="currentColor" stroke-width="2" />
              </svg>
            </div>
          </div>
          <h1 class="text-2xl font-bold text-foreground">Welcome Back</h1>
          <p class="text-sm text-muted-foreground mt-1">
            Enter your password to unlock Alexandria.
          </p>
        </div>

        <div class="card p-6">
          <div class="mb-4">
            <label class="label text-xs text-muted-foreground">Password</label>
            <input
              v-model="password"
              type="password"
              placeholder="Enter your vault password"
              class="input"
              autofocus
              @keydown="handleKeydown"
            >
          </div>

          <p v-if="error" class="text-sm text-error mb-4">{{ error }}</p>

          <AppButton
            class="w-full"
            :disabled="!password || recovering"
            @click="unlock"
          >
            Unlock
          </AppButton>

          <AppButton
            v-if="biometricAvailable"
            class="w-full mt-2"
            variant="secondary"
            :loading="biometricLoading"
            :disabled="unlocking"
            @click="unlockWithBiometric"
          >
            Unlock with Biometrics
          </AppButton>

          <AppButton
            class="w-full mt-2"
            variant="outline"
            :loading="recovering"
            :disabled="unlocking || biometricLoading"
            @click="promptRecoverWallet"
          >
            Recover Wallet with Recovery Phrase
          </AppButton>

          <p v-if="biometricAvailable && !hasBiometricCredential" class="mt-2 text-xs text-muted-foreground">
            Touch ID is available. Unlock once with password to enable biometric unlock.
          </p>

          <p class="mt-3 text-xs text-muted-foreground">
            Forgot your password? Recovery removes the local wallet on this device and lets you restore it from your recovery phrase.
          </p>
        </div>

        <p class="text-center text-xs text-muted-foreground mt-4 italic tracking-wide">
          I am, because we all are
        </p>
      </div>

      <!-- ============================================ -->
      <!-- UNLOCKING STATE — animated progress          -->
      <!-- ============================================ -->
      <div v-else class="text-center">
        <!-- Orbital animation (same as onboarding) -->
        <div class="relative w-24 h-24 mx-auto mb-6">
          <div class="absolute inset-0 rounded-full border border-border/40" />
          <div class="absolute inset-0 animate-spin" style="animation-duration: 3s;">
            <div class="absolute top-0 left-1/2 -translate-x-1/2 -translate-y-1/2 w-2.5 h-2.5 rounded-full bg-primary" />
          </div>
          <div class="absolute inset-3 rounded-full border border-border/30" />
          <div class="absolute inset-3 animate-spin" style="animation-duration: 2s; animation-direction: reverse;">
            <div class="absolute top-0 left-1/2 -translate-x-1/2 -translate-y-1/2 w-2 h-2 rounded-full bg-primary/70" />
          </div>
          <div class="absolute inset-6 rounded-full bg-primary/10 flex items-center justify-center">
            <svg class="w-6 h-6 text-primary animate-pulse" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M13.5 10.5V6.75a4.5 4.5 0 119 0v3.75M3.75 21.75h10.5a2.25 2.25 0 002.25-2.25v-6.75a2.25 2.25 0 00-2.25-2.25H3.75a2.25 2.25 0 00-2.25 2.25v6.75a2.25 2.25 0 002.25 2.25z" />
            </svg>
          </div>
        </div>

        <h2 class="text-xl font-bold mb-1 text-foreground">
          Unlocking Vault
        </h2>
        <p class="text-sm text-muted-foreground mb-6">
          Decrypting your identity and deriving keys...
        </p>

        <!-- Live log output -->
        <div class="card p-4 text-left mb-4">
          <div class="font-mono text-xs space-y-1.5 min-h-[80px]">
            <div
              v-for="(line, i) in progressLines"
              :key="i"
              class="flex items-start gap-2 text-muted-foreground"
              :class="{ 'text-primary': i === progressLines.length - 1 }"
            >
              <svg v-if="i < progressLines.length - 1" class="w-3 h-3 mt-0.5 shrink-0 text-success" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="3">
                <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
              </svg>
              <div v-else class="w-3 h-3 mt-0.5 shrink-0 border-2 border-primary border-t-transparent rounded-full animate-spin" />
              <span>{{ line }}</span>
            </div>
            <div v-if="progressLines.length === 0" class="flex items-start gap-2 text-primary">
              <div class="w-3 h-3 mt-0.5 shrink-0 border-2 border-primary border-t-transparent rounded-full animate-spin" />
              <span>Initializing...</span>
            </div>
          </div>
        </div>

        <p class="text-xs text-muted-foreground italic tracking-wide">
          I am, because we all are
        </p>
      </div>

    </div>

    <!-- Recovery confirmation dialog -->
    <Teleport to="body">
      <div v-if="showRecoverConfirm" class="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 backdrop-blur-sm p-4">
        <div class="w-full max-w-sm rounded-2xl bg-card border border-border p-6 shadow-2xl">
          <h3 class="text-base font-semibold text-foreground">Reset wallet?</h3>
          <p class="mt-2 text-sm text-muted-foreground">
            This will remove the local Alexandria wallet on this device and send you to wallet recovery. Continue only if you still have your recovery phrase.
          </p>
          <div class="mt-5 flex gap-3">
            <AppButton variant="outline" class="flex-1" @click="showRecoverConfirm = false">
              Cancel
            </AppButton>
            <AppButton variant="danger" class="flex-1" @click="confirmRecoverWallet">
              Reset &amp; Recover
            </AppButton>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>
