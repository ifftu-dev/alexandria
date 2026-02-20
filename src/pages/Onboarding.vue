<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'

const { invoke } = useLocalApi()
const router = useRouter()

type Step = 'welcome' | 'password' | 'generating' | 'backup' | 'done'
type Mode = 'create' | 'import'

const mode = ref<Mode>('create')
const step = ref<Step>('welcome')
const mnemonic = ref('')
const importMnemonic = ref('')
const password = ref('')
const confirmPassword = ref('')
const error = ref('')

const passwordsMatch = computed(() => password.value === confirmPassword.value)
const passwordValid = computed(() => password.value.length >= 8)

function startCreate() {
  mode.value = 'create'
  step.value = 'password'
  error.value = ''
}

function startImport() {
  mode.value = 'import'
  step.value = 'password'
  error.value = ''
}

function goBack() {
  if (step.value === 'password') {
    step.value = 'welcome'
    password.value = ''
    confirmPassword.value = ''
    importMnemonic.value = ''
    error.value = ''
  }
}

async function proceedFromPassword() {
  error.value = ''

  if (!passwordValid.value) {
    error.value = 'Password must be at least 8 characters.'
    return
  }
  if (!passwordsMatch.value) {
    error.value = 'Passwords do not match.'
    return
  }

  if (mode.value === 'create') {
    await createWallet()
  } else {
    await restoreWallet()
  }
}

async function createWallet() {
  step.value = 'generating'

  try {
    const result = await invoke<{
      mnemonic: string
      stake_address: string
      payment_address: string
    }>('generate_wallet', { password: password.value })

    mnemonic.value = result.mnemonic
    step.value = 'backup'
  } catch (e) {
    error.value = String(e)
    step.value = 'password'
  }
}

async function restoreWallet() {
  const phrase = importMnemonic.value.trim()
  if (!phrase) {
    error.value = 'Please enter your recovery phrase.'
    return
  }

  const words = phrase.split(/\s+/)
  if (words.length !== 12 && words.length !== 15 && words.length !== 24) {
    error.value = 'Recovery phrase must be 12, 15, or 24 words.'
    return
  }

  step.value = 'generating'

  try {
    await invoke<{
      stake_address: string
      payment_address: string
      has_mnemonic_backup: boolean
    }>('restore_wallet', { mnemonic: phrase, password: password.value })

    step.value = 'done'
  } catch (e) {
    error.value = String(e)
    step.value = 'password'
  }
}

function confirmBackup() {
  step.value = 'done'
}

function enterApp() {
  router.replace('/home')
}
</script>

<template>
  <div class="min-h-screen flex items-center justify-center bg-[rgb(var(--color-background))] p-8">
    <div class="w-full max-w-lg">
      <!-- Welcome -->
      <div v-if="step === 'welcome'" class="text-center">
        <h1 class="text-3xl font-bold mb-2 text-[rgb(var(--color-foreground))]">Welcome to Alexandria</h1>
        <p class="text-[rgb(var(--color-muted-foreground))] mb-8">
          Free, decentralized learning. Your credentials. Your identity. Your control.
        </p>

        <div class="card p-6 mb-6 text-left">
          <h2 class="text-base font-semibold mb-3">What happens next</h2>
          <ul class="space-y-2 text-sm text-[rgb(var(--color-muted-foreground))]">
            <li class="flex items-start gap-2">
              <span class="text-[rgb(var(--color-primary))] mt-0.5">1.</span>
              You set a password to protect your vault on this device.
            </li>
            <li class="flex items-start gap-2">
              <span class="text-[rgb(var(--color-primary))] mt-0.5">2.</span>
              We generate a unique wallet — your identity on the network.
            </li>
            <li class="flex items-start gap-2">
              <span class="text-[rgb(var(--color-primary))] mt-0.5">3.</span>
              You receive a 24-word recovery phrase. Write it down and keep it safe.
            </li>
            <li class="flex items-start gap-2">
              <span class="text-[rgb(var(--color-primary))] mt-0.5">4.</span>
              Start learning, earn credentials, own your education.
            </li>
          </ul>
        </div>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-[rgb(var(--color-primary))] text-[rgb(var(--color-primary-foreground))] hover:bg-[rgb(var(--color-primary-hover))] transition-colors"
          @click="startCreate"
        >
          Create My Identity
        </button>

        <button
          class="w-full mt-3 py-2.5 px-4 rounded-md text-sm font-medium border border-[rgb(var(--color-border))] text-[rgb(var(--color-foreground))] hover:bg-[rgb(var(--color-muted)/0.5)] transition-colors"
          @click="startImport"
        >
          Import Existing Wallet
        </button>
      </div>

      <!-- Password Setup -->
      <div v-else-if="step === 'password'">
        <button
          class="mb-4 text-sm text-[rgb(var(--color-muted-foreground))] hover:text-[rgb(var(--color-foreground))] transition-colors"
          @click="goBack"
        >
          &larr; Back
        </button>

        <h1 class="text-2xl font-bold mb-2 text-center">
          {{ mode === 'create' ? 'Set Your Password' : 'Import Wallet' }}
        </h1>
        <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6 text-center">
          {{ mode === 'create'
            ? 'This password protects your encrypted vault on this device.'
            : 'Enter your recovery phrase and set a password for this device.'
          }}
        </p>

        <!-- Import: Mnemonic input -->
        <div v-if="mode === 'import'" class="card p-5 mb-4">
          <label class="block text-xs font-medium text-[rgb(var(--color-muted-foreground))] mb-1.5">
            Recovery Phrase
          </label>
          <textarea
            v-model="importMnemonic"
            placeholder="Enter your 24-word recovery phrase, separated by spaces"
            rows="3"
            class="w-full px-3 py-2 text-sm font-mono rounded-md border border-[rgb(var(--color-border))] bg-[rgb(var(--color-background))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-ring))] resize-none"
          />
        </div>

        <!-- Password fields -->
        <div class="card p-5 mb-4">
          <div class="space-y-4">
            <div>
              <label class="block text-xs font-medium text-[rgb(var(--color-muted-foreground))] mb-1.5">
                Password
              </label>
              <input
                v-model="password"
                type="password"
                placeholder="At least 8 characters"
                class="w-full px-3 py-2 text-sm rounded-md border border-[rgb(var(--color-border))] bg-[rgb(var(--color-background))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-ring))]"
              >
            </div>
            <div>
              <label class="block text-xs font-medium text-[rgb(var(--color-muted-foreground))] mb-1.5">
                Confirm Password
              </label>
              <input
                v-model="confirmPassword"
                type="password"
                placeholder="Enter password again"
                class="w-full px-3 py-2 text-sm rounded-md border border-[rgb(var(--color-border))] bg-[rgb(var(--color-background))] focus:outline-none focus:ring-2 focus:ring-[rgb(var(--color-ring))]"
              >
              <p
                v-if="confirmPassword && !passwordsMatch"
                class="text-xs text-[rgb(var(--color-error))] mt-1"
              >
                Passwords do not match.
              </p>
            </div>
          </div>
        </div>

        <div class="card p-4 mb-4 border-[rgb(var(--color-warning))] bg-[rgb(var(--color-warning)/0.05)]">
          <p class="text-sm text-[rgb(var(--color-warning))] font-medium">
            There is no password recovery. If you forget this password, you'll need your recovery phrase to restore access.
          </p>
        </div>

        <p v-if="error" class="text-sm text-[rgb(var(--color-error))] mb-3">{{ error }}</p>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-[rgb(var(--color-primary))] text-[rgb(var(--color-primary-foreground))] hover:bg-[rgb(var(--color-primary-hover))] transition-colors disabled:opacity-50"
          :disabled="!passwordValid || !passwordsMatch"
          @click="proceedFromPassword"
        >
          {{ mode === 'create' ? 'Create Wallet' : 'Restore Wallet' }}
        </button>
      </div>

      <!-- Generating -->
      <div v-else-if="step === 'generating'" class="text-center">
        <div class="w-10 h-10 border-2 border-[rgb(var(--color-primary))] border-t-transparent rounded-full animate-spin mx-auto mb-4" />
        <p class="text-[rgb(var(--color-muted-foreground))]">
          {{ mode === 'create' ? 'Generating your wallet...' : 'Restoring your wallet...' }}
        </p>
        <p class="text-xs text-[rgb(var(--color-muted-foreground))] mt-2">
          Encrypting your vault. This may take a moment.
        </p>
      </div>

      <!-- Backup -->
      <div v-else-if="step === 'backup'" class="text-center">
        <h1 class="text-2xl font-bold mb-2">Your Recovery Phrase</h1>
        <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
          Write these 24 words down on paper and store them somewhere safe.
          This is the ONLY way to recover your identity if you forget your password.
        </p>

        <div class="card p-5 mb-6">
          <div class="grid grid-cols-3 gap-2">
            <div
              v-for="(word, i) in mnemonic.split(' ')"
              :key="i"
              class="flex items-center gap-2 text-sm py-1.5 px-2.5 rounded bg-[rgb(var(--color-muted)/0.3)]"
            >
              <span class="text-xs text-[rgb(var(--color-muted-foreground))] w-5 text-right">{{ i + 1 }}.</span>
              <span class="font-mono font-medium">{{ word }}</span>
            </div>
          </div>
        </div>

        <div class="card p-4 mb-6 border-[rgb(var(--color-warning))] bg-[rgb(var(--color-warning)/0.05)]">
          <p class="text-sm text-[rgb(var(--color-warning))] font-medium">
            Never share your recovery phrase. Anyone with these words can access your credentials.
          </p>
        </div>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-[rgb(var(--color-primary))] text-[rgb(var(--color-primary-foreground))] hover:bg-[rgb(var(--color-primary-hover))] transition-colors"
          @click="confirmBackup"
        >
          I've Written It Down
        </button>
      </div>

      <!-- Done -->
      <div v-else-if="step === 'done'" class="text-center">
        <div class="w-16 h-16 rounded-full bg-[rgb(var(--color-success)/0.1)] flex items-center justify-center mx-auto mb-4">
          <svg class="w-8 h-8 text-[rgb(var(--color-success))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
          </svg>
        </div>
        <h1 class="text-2xl font-bold mb-2">You're Ready</h1>
        <p class="text-[rgb(var(--color-muted-foreground))] mb-2">
          {{ mode === 'create'
            ? 'Your identity has been created and encrypted.'
            : 'Your wallet has been restored and encrypted.'
          }}
        </p>
        <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
          All your data stays on this device, protected by your password.
        </p>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-[rgb(var(--color-primary))] text-[rgb(var(--color-primary-foreground))] hover:bg-[rgb(var(--color-primary-hover))] transition-colors"
          @click="enterApp"
        >
          Start Learning
        </button>
      </div>
    </div>
  </div>
</template>
