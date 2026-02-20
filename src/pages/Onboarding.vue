<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'

const { invoke } = useLocalApi()
const router = useRouter()

const step = ref<'welcome' | 'generating' | 'backup' | 'done'>('welcome')
const mnemonic = ref('')
const error = ref('')

async function createWallet() {
  step.value = 'generating'
  error.value = ''

  try {
    const result = await invoke<{
      mnemonic: string
      stake_address: string
      payment_address: string
    }>('generate_wallet')

    mnemonic.value = result.mnemonic
    step.value = 'backup'
  } catch (e) {
    error.value = String(e)
    step.value = 'welcome'
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
              We generate a unique wallet — your identity on the network.
            </li>
            <li class="flex items-start gap-2">
              <span class="text-[rgb(var(--color-primary))] mt-0.5">2.</span>
              You receive a 24-word recovery phrase. Write it down and keep it safe.
            </li>
            <li class="flex items-start gap-2">
              <span class="text-[rgb(var(--color-primary))] mt-0.5">3.</span>
              Start learning, earn credentials, own your education.
            </li>
          </ul>
        </div>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-[rgb(var(--color-primary))] text-[rgb(var(--color-primary-foreground))] hover:bg-[rgb(var(--color-primary-hover))] transition-colors"
          @click="createWallet"
        >
          Create My Identity
        </button>

        <p v-if="error" class="mt-3 text-sm text-[rgb(var(--color-error))]">{{ error }}</p>
      </div>

      <!-- Generating -->
      <div v-else-if="step === 'generating'" class="text-center">
        <div class="w-10 h-10 border-2 border-[rgb(var(--color-primary))] border-t-transparent rounded-full animate-spin mx-auto mb-4" />
        <p class="text-[rgb(var(--color-muted-foreground))]">Generating your wallet...</p>
      </div>

      <!-- Backup -->
      <div v-else-if="step === 'backup'" class="text-center">
        <h1 class="text-2xl font-bold mb-2">Your Recovery Phrase</h1>
        <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
          Write these 24 words down on paper and store them somewhere safe.
          This is the ONLY way to recover your identity and credentials.
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
          <span class="text-3xl">&#10003;</span>
        </div>
        <h1 class="text-2xl font-bold mb-2">You're Ready</h1>
        <p class="text-[rgb(var(--color-muted-foreground))] mb-6">
          Your identity has been created. All your data stays on this device.
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
