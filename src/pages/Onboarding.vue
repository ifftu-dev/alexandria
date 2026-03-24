<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import { useAuth } from '@/composables/useAuth'
import { biometricSupported, storeVaultPasswordForBiometric } from '@/composables/useBiometricVault'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import Starfield from '@/components/auth/Starfield.vue'

const router = useRouter()
const { generateWallet: authGenerate, restoreWallet: authRestore, checkVaultExists } = useAuth()

const vaultExists = ref(false)

type Step = 'welcome' | 'password' | 'generating' | 'backup' | 'done'
type Mode = 'create' | 'import'

const mode = ref<Mode>('create')
const step = ref<Step>('welcome')
const mnemonic = ref('')
const importMnemonic = ref('')
const password = ref('')
const confirmPassword = ref('')
const error = ref('')
const biometricHint = ref('')
const biometricAvailable = ref(false)
const enableBiometricOnSetup = ref(false)

const copied = ref(false)

// Progress tracking from Rust events
const progressLines = ref<string[]>([])
const currentStep = ref('')
let unlisten: UnlistenFn | null = null

onMounted(async () => {
  unlisten = await listen<{ step: string; detail: string }>('vault-progress', (event) => {
    currentStep.value = event.payload.step
    progressLines.value.push(event.payload.detail)
  })

  // Check if a vault already exists (user may want to sign in instead)
  try {
    vaultExists.value = await checkVaultExists()
    biometricAvailable.value = await biometricSupported()
  } catch {
    // ignore
    biometricAvailable.value = false
  }
})

onUnmounted(() => {
  if (unlisten) unlisten()
  // Clear sensitive data from memory (JS strings are GC'd, not truly zeroed,
  // but dropping references allows collection sooner)
  mnemonic.value = ''
  importMnemonic.value = ''
  password.value = ''
  confirmPassword.value = ''
})

const passwordsMatch = computed(() => password.value === confirmPassword.value)
const passwordValid = computed(() => password.value.length >= 12)
const mnemonicWords = computed(() => mnemonic.value.trim().split(/\s+/).filter(Boolean))

const createWizardSteps: { id: Step; label: string }[] = [
  { id: 'welcome', label: 'Welcome' },
  { id: 'password', label: 'Secure Vault' },
  { id: 'generating', label: 'Generate Keys' },
  { id: 'backup', label: 'Backup Phrase' },
  { id: 'done', label: 'Complete' },
]

const importWizardSteps: { id: Step; label: string }[] = [
  { id: 'welcome', label: 'Welcome' },
  { id: 'password', label: 'Secure Vault' },
  { id: 'generating', label: 'Restore Keys' },
  { id: 'done', label: 'Complete' },
]

const wizardSteps = computed(() => (mode.value === 'create' ? createWizardSteps : importWizardSteps))
const activeStepIndex = computed(() => {
  const idx = wizardSteps.value.findIndex((s) => s.id === step.value)
  return idx >= 0 ? idx : 0
})
const progressPercent = computed(() => {
  const maxIndex = wizardSteps.value.length - 1
  if (maxIndex <= 0) return 0
  return Math.round((activeStepIndex.value / maxIndex) * 100)
})

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
    error.value = 'Password must be at least 12 characters.'
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
  progressLines.value = []
  currentStep.value = ''

  try {
    const result = await authGenerate(password.value)
    mnemonic.value = result.mnemonic
    try {
      if (enableBiometricOnSetup.value && biometricAvailable.value) {
        const mode = await storeVaultPasswordForBiometric(password.value)
        biometricHint.value = mode === 'secure'
          ? 'Biometric unlock enabled on this device.'
          : 'Biometric unlock enabled for this app session (dev runtime keychain limitation).'
      }
    } catch {
      biometricHint.value = 'Biometric unlock setup skipped. You can still unlock with password.'
    }
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
  progressLines.value = []
  currentStep.value = ''

  try {
    await authRestore(phrase, password.value)
    try {
      if (enableBiometricOnSetup.value && biometricAvailable.value) {
        const mode = await storeVaultPasswordForBiometric(password.value)
        biometricHint.value = mode === 'secure'
          ? 'Biometric unlock enabled on this device.'
          : 'Biometric unlock enabled for this app session (dev runtime keychain limitation).'
      }
    } catch {
      biometricHint.value = 'Biometric unlock setup skipped. You can still unlock with password.'
    }
    step.value = 'done'
  } catch (e) {
    error.value = String(e)
    step.value = 'password'
  }
}

async function copyMnemonic() {
  await navigator.clipboard.writeText(mnemonic.value)
  copied.value = true
  setTimeout(() => { copied.value = false }, 2000)
}

function confirmBackup() {
  step.value = 'done'
}

function enterApp() {
  router.replace('/home')
}
</script>

<template>
  <div class="min-h-full bg-background relative overflow-y-auto flex items-center justify-center p-4 sm:p-6 lg:p-8">
    <Starfield />

    <div class="w-full max-w-6xl relative z-10">
      <div class="grid gap-4 lg:grid-cols-[280px_minmax(0,1fr)] lg:gap-6 xl:gap-8">
        <aside class="hidden lg:flex lg:flex-col rounded-2xl border border-border/70 bg-card/70 backdrop-blur p-6">
          <div class="mb-6">
            <div class="relative w-12 h-12 mb-4">
              <div class="absolute inset-0 rounded-full bg-primary/10 animate-ping" style="animation-duration: 3s;" />
              <div class="relative w-12 h-12 flex items-center justify-center">
                <svg class="w-9 h-9 text-primary" viewBox="0 0 32 32" fill="none">
                  <path d="M16 2L4 8v16l12 6 12-6V8L16 2z" stroke="currentColor" stroke-width="2" fill="none" />
                  <path d="M16 8v16M8 12l8 4 8-4" stroke="currentColor" stroke-width="2" />
                </svg>
              </div>
            </div>
            <h2 class="text-xl font-semibold text-foreground">Welcome to Alexandria</h2>
            <p class="mt-1 text-sm text-muted-foreground">Set up your sovereign learning identity in a few guided steps.</p>
          </div>

          <div class="space-y-2.5">
            <div
              v-for="(wizardStep, index) in wizardSteps"
              :key="wizardStep.id"
              class="flex items-center gap-3 rounded-lg px-2.5 py-2"
              :class="index === activeStepIndex ? 'bg-primary/10 text-primary' : index < activeStepIndex ? 'text-foreground' : 'text-muted-foreground'"
            >
              <span
                class="flex h-6 w-6 items-center justify-center rounded-full border text-xs font-semibold"
                :class="index <= activeStepIndex ? 'border-primary/50 bg-primary/10' : 'border-border/70 bg-background/70'"
              >
                {{ index + 1 }}
              </span>
              <span class="text-sm font-medium">{{ wizardStep.label }}</span>
            </div>
          </div>

          <div class="mt-auto pt-6 text-xs text-muted-foreground italic tracking-wide">
            I am, because we all are
          </div>
        </aside>

        <div class="rounded-2xl border border-border/70 bg-card/80 backdrop-blur px-4 py-5 sm:px-6 sm:py-6 lg:px-8 lg:py-8">
          <div class="mb-5">
            <div class="flex items-center justify-between text-xs text-muted-foreground mb-2">
              <span>{{ wizardSteps[activeStepIndex]?.label }}</span>
              <span>{{ progressPercent }}%</span>
            </div>
            <div class="h-1.5 rounded-full bg-muted/50 overflow-hidden">
              <div class="h-full bg-primary transition-all duration-500" :style="{ width: `${progressPercent}%` }" />
            </div>
          </div>

      <!-- ============================================ -->
      <!-- WELCOME                                      -->
      <!-- ============================================ -->
      <div v-if="step === 'welcome'" class="text-center">
        <!-- Alexandria logo -->
        <div class="relative w-16 h-16 mx-auto mb-6">
          <div class="absolute inset-0 rounded-full bg-primary/8 animate-ping" style="animation-duration: 3s;" />
          <div class="relative w-16 h-16 flex items-center justify-center">
            <svg class="w-12 h-12 text-primary" viewBox="0 0 32 32" fill="none">
              <path d="M16 2L4 8v16l12 6 12-6V8L16 2z" stroke="currentColor" stroke-width="2" fill="none" />
              <path d="M16 8v16M8 12l8 4 8-4" stroke="currentColor" stroke-width="2" />
            </svg>
          </div>
        </div>

        <h1 class="text-3xl font-bold mb-1 text-foreground">Alexandria</h1>
        <p class="text-sm text-muted-foreground mb-1 italic tracking-wide">
          I am, because we all are
        </p>
        <p class="text-muted-foreground mb-8 text-sm">
          Free, decentralized learning. Your credentials. Your identity. Your control.
        </p>

        <div class="card p-6 mb-6 text-left">
          <h2 class="text-base font-semibold mb-3">What happens next</h2>
          <ul class="space-y-2 text-sm text-muted-foreground">
            <li class="flex items-start gap-2">
              <span class="text-primary mt-0.5 font-mono text-xs w-4 text-right shrink-0">01</span>
              You set a password to protect your vault on this device.
            </li>
            <li class="flex items-start gap-2">
              <span class="text-primary mt-0.5 font-mono text-xs w-4 text-right shrink-0">02</span>
              We generate a unique wallet &mdash; your identity on the network.
            </li>
            <li class="flex items-start gap-2">
              <span class="text-primary mt-0.5 font-mono text-xs w-4 text-right shrink-0">03</span>
              You receive a 24-word recovery phrase. Write it down and keep it safe.
            </li>
            <li class="flex items-start gap-2">
              <span class="text-primary mt-0.5 font-mono text-xs w-4 text-right shrink-0">04</span>
              Start learning, earn credentials, own your education.
            </li>
          </ul>
        </div>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors"
          @click="startCreate"
        >
          Create My Identity
        </button>

        <button
          class="w-full mt-3 py-2.5 px-4 rounded-md text-sm font-medium border border-border text-foreground hover:bg-muted/50 transition-colors"
          @click="startImport"
        >
          Import Existing Wallet
        </button>

        <button
          v-if="vaultExists"
          class="w-full mt-3 py-2 text-sm text-primary hover:underline transition-colors"
          @click="router.replace('/unlock')"
        >
          Sign in to existing vault
        </button>
      </div>

      <!-- ============================================ -->
      <!-- PASSWORD SETUP                               -->
      <!-- ============================================ -->
      <div v-else-if="step === 'password'">
        <button
          class="mb-4 text-sm text-muted-foreground hover:text-foreground transition-colors flex items-center gap-1"
          @click="goBack"
        >
          <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
          </svg>
          Back
        </button>

        <h1 class="text-2xl font-bold mb-2 text-center">
          {{ mode === 'create' ? 'Set Your Password' : 'Import Wallet' }}
        </h1>
        <p class="text-sm text-muted-foreground mb-6 text-center">
          {{ mode === 'create'
            ? 'This password protects your encrypted vault on this device.'
            : 'Enter your recovery phrase and set a password for this device.'
          }}
        </p>

        <!-- Import: Mnemonic input -->
        <div v-if="mode === 'import'" class="card p-5 mb-4">
          <label class="block text-xs font-medium text-muted-foreground mb-1.5">
            Recovery Phrase
          </label>
          <textarea
            v-model="importMnemonic"
            placeholder="Enter your 24-word recovery phrase, separated by spaces"
            rows="3"
            class="w-full px-3 py-2 text-sm font-mono rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring resize-none"
          />
        </div>

        <!-- Password fields -->
        <div class="card p-5 mb-4">
          <div class="space-y-4">
            <div>
              <label class="block text-xs font-medium text-muted-foreground mb-1.5">
                Password
              </label>
              <input
                v-model="password"
                type="password"
                placeholder="At least 12 characters"
                class="w-full px-3 py-2 text-sm rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring"
              >
            </div>
            <div>
              <label class="block text-xs font-medium text-muted-foreground mb-1.5">
                Confirm Password
              </label>
              <input
                v-model="confirmPassword"
                type="password"
                placeholder="Enter password again"
                class="w-full px-3 py-2 text-sm rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring"
              >
              <p
                v-if="confirmPassword && !passwordsMatch"
                class="text-xs text-error mt-1"
              >
                Passwords do not match.
              </p>
            </div>
          </div>
        </div>

        <div class="card p-4 mb-4 border-warning bg-warning/5">
          <p class="text-sm text-warning font-medium">
            There is no password recovery. If you forget this password, you'll need your recovery phrase to restore access.
          </p>
        </div>

        <div v-if="biometricAvailable" class="card p-4 mb-4">
          <label class="flex items-start gap-3 cursor-pointer">
            <input
              v-model="enableBiometricOnSetup"
              type="checkbox"
              class="mt-0.5 h-4 w-4 rounded border-border"
            >
            <span>
              <span class="block text-sm font-medium text-foreground">Enable biometric unlock on this device</span>
              <span class="block text-xs text-muted-foreground mt-0.5">
                Use Touch ID / Face ID after setup. You can change this later in Settings.
              </span>
            </span>
          </label>
        </div>

        <p v-if="error" class="text-sm text-error mb-3">{{ error }}</p>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors disabled:opacity-50"
          :disabled="!passwordValid || !passwordsMatch"
          @click="proceedFromPassword"
        >
          {{ mode === 'create' ? 'Create Wallet' : 'Restore Wallet' }}
        </button>
      </div>

      <!-- ============================================ -->
      <!-- GENERATING — animated progress with log lines -->
      <!-- ============================================ -->
      <div v-else-if="step === 'generating'" class="text-center">
        <!-- Orbital animation -->
        <div class="relative w-24 h-24 mx-auto mb-6">
          <!-- Outer orbit -->
          <div class="absolute inset-0 rounded-full border border-border/40" />
          <div class="absolute inset-0 animate-spin" style="animation-duration: 3s;">
            <div class="absolute top-0 left-1/2 -translate-x-1/2 -translate-y-1/2 w-2.5 h-2.5 rounded-full bg-primary" />
          </div>
          <!-- Middle orbit -->
          <div class="absolute inset-3 rounded-full border border-border/30" />
          <div class="absolute inset-3 animate-spin" style="animation-duration: 2s; animation-direction: reverse;">
            <div class="absolute top-0 left-1/2 -translate-x-1/2 -translate-y-1/2 w-2 h-2 rounded-full bg-primary/70" />
          </div>
          <!-- Inner core -->
          <div class="absolute inset-6 rounded-full bg-primary/10 flex items-center justify-center">
            <svg class="w-6 h-6 text-primary animate-pulse" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 5.25a3 3 0 013 3m3 0a6 6 0 01-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1121.75 8.25z" />
            </svg>
          </div>
        </div>

        <h2 class="text-xl font-bold mb-1 text-foreground">
          {{ mode === 'create' ? 'Creating Your Identity' : 'Restoring Your Wallet' }}
        </h2>
        <p class="text-sm text-muted-foreground mb-6">
          This involves cryptographic key derivation and may take a moment.
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

      </div>

      <!-- ============================================ -->
      <!-- BACKUP                                       -->
      <!-- ============================================ -->
      <div v-else-if="step === 'backup'" class="text-center">
        <h1 class="text-2xl font-bold mb-2">Your Recovery Phrase</h1>
        <p class="text-sm text-muted-foreground mb-6">
          Write these 24 words down on paper and store them somewhere safe.
          This is the ONLY way to recover your identity if you forget your password.
        </p>

        <div class="card p-5 mb-6">
          <div class="grid grid-cols-2 sm:grid-cols-3 gap-2">
            <div
              v-for="(word, i) in mnemonicWords"
              :key="i"
              class="flex items-center gap-2 text-sm py-1.5 px-2.5 rounded bg-muted/30"
            >
              <span class="text-xs text-muted-foreground w-5 text-right font-mono">{{ String(i + 1).padStart(2, '0') }}</span>
              <span class="font-mono font-medium">{{ word }}</span>
            </div>
          </div>

          <!-- Copy button -->
          <button
            class="mt-3 w-full flex items-center justify-center gap-2 py-2 px-3 rounded-md text-xs font-medium border border-border transition-colors"
            :class="copied
              ? 'bg-success/10 text-success border-success/30'
              : 'text-muted-foreground hover:bg-muted/50 hover:text-foreground'"
            @click="copyMnemonic"
          >
            <svg v-if="!copied" class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15.666 3.888A2.25 2.25 0 0013.5 2.25h-3c-1.03 0-1.9.693-2.166 1.638m7.332 0c.055.194.084.4.084.612v0a.75.75 0 01-.75.75H9.75a.75.75 0 01-.75-.75v0c0-.212.03-.418.084-.612m7.332 0c.646.049 1.288.11 1.927.184 1.1.128 1.907 1.077 1.907 2.185V19.5a2.25 2.25 0 01-2.25 2.25H6.75A2.25 2.25 0 014.5 19.5V6.257c0-1.108.806-2.057 1.907-2.185a48.208 48.208 0 011.927-.184" />
            </svg>
            <svg v-else class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
            </svg>
            {{ copied ? 'Copied to clipboard' : 'Copy recovery phrase' }}
          </button>
        </div>

        <div class="card p-4 mb-6 border-warning bg-warning/5">
          <p class="text-sm text-warning font-medium">
            Never share your recovery phrase. Anyone with these words can access your credentials.
          </p>
        </div>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors"
          @click="confirmBackup"
        >
          I've Written It Down
        </button>
      </div>

      <!-- ============================================ -->
      <!-- DONE                                         -->
      <!-- ============================================ -->
      <div v-else-if="step === 'done'" class="text-center">
        <div class="w-16 h-16 rounded-full bg-success/10 flex items-center justify-center mx-auto mb-4">
          <svg class="w-8 h-8 text-success" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
          </svg>
        </div>
        <h1 class="text-2xl font-bold mb-2">You're Ready</h1>
        <p class="text-muted-foreground mb-2">
          {{ mode === 'create'
            ? 'Your identity has been created and encrypted.'
            : 'Your wallet has been restored and encrypted.'
          }}
        </p>
        <p class="text-sm text-muted-foreground mb-6">
          All your data stays on this device, protected by your password.
        </p>
        <p v-if="biometricHint" class="text-xs text-muted-foreground mb-4">
          {{ biometricHint }}
        </p>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors"
          @click="enterApp"
        >
          Start Learning
        </button>

        <p class="text-xs text-muted-foreground mt-4 italic tracking-wide">
          I am, because we all are
        </p>
      </div>

        </div>
      </div>
    </div>
  </div>
</template>
