<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useProfiles } from '@/composables/useProfiles'
import { useLocalApi } from '@/composables/useLocalApi'
import { biometricSupported, storeVaultPasswordForBiometric } from '@/composables/useBiometricVault'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import Starfield from '@/components/auth/Starfield.vue'
import GoalPicker from '@/components/goals/GoalPicker.vue'
import SkillBootstrapPanel from '@/components/skills/SkillBootstrapPanel.vue'
import { useGoals } from '@/composables/useGoals'
import type { AccountRole } from '@/types'

const router = useRouter()
const route = useRoute()
const { profiles, refreshProfiles, createProfile, restoreProfileWithMnemonic, activeProfileId } = useProfiles()
const { invoke } = useLocalApi()

const vaultExists = computed(() => profiles.value.length > 0)
const username = ref('')
const displayName = ref('')
// Display name mirrors the username until the user edits it directly
// (Instagram-style: same by default, separable on demand).
const displayNameDirty = ref(false)
watch(username, (u) => {
  if (!displayNameDirty.value) displayName.value = u
})
const USERNAME_RE = /^[a-z0-9_]{3,32}$/
const usernameValid = computed(() => USERNAME_RE.test(username.value.trim().toLowerCase()))

// Live availability against the DHT username registry (debounced).
// 'unknown' = network unreachable — warn but don't block signup; the
// claim publishes (and conflicts resolve deterministically) once online.
type Availability = 'idle' | 'checking' | 'available' | 'taken' | 'unknown'
const availability = ref<Availability>('idle')
let availabilityTimer: ReturnType<typeof setTimeout> | null = null
watch(username, (u) => {
  availability.value = 'idle'
  if (availabilityTimer) clearTimeout(availabilityTimer)
  const candidate = u.trim().toLowerCase()
  if (!USERNAME_RE.test(candidate)) return
  availabilityTimer = setTimeout(async () => {
    availability.value = 'checking'
    try {
      const res = await invoke<{ available: boolean; authoritative: boolean }>(
        'check_username_availability',
        { username: candidate },
      )
      availability.value = res.available
        ? res.authoritative ? 'available' : 'unknown'
        : 'taken'
    } catch {
      availability.value = 'unknown'
    }
  }, 400)
})

type Step =
  | 'welcome'
  | 'role'
  | 'birthdate'
  | 'password'
  | 'generating'
  | 'backup'
  | 'goals'
  | 'bootstrap'
  | 'link-child'
  | 'done'
type Mode = 'create' | 'import'

const mode = ref<Mode>('create')
const step = ref<Step>('welcome')

// ── Role & birthdate (learners only) ────────────────────────────
const selectedRole = ref<AccountRole | null>(null)
const birthdate = ref('')

const roleCards: { id: AccountRole; title: string; desc: string; icon: string }[] = [
  {
    id: 'learner',
    title: 'Learner',
    desc: 'Take courses, earn verifiable credentials, and grow your skills.',
    icon: 'M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253',
  },
  {
    id: 'instructor',
    title: 'Instructor',
    desc: 'Author courses and tutorials, review submissions, and mentor learners. You can switch into learner mode any time.',
    icon: 'M8.25 6.75h12M8.25 12h12m-12 5.25h12M3.75 6.75h.007v.008H3.75V6.75zm.375 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zM3.75 12h.007v.008H3.75V12zm.375 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm-.375 5.25h.007v.008H3.75v-.008zm.375 0a.375.375 0 11-.75 0 .375.375 0 01.75 0z',
  },
  {
    id: 'parent',
    title: 'Parent / Guardian',
    desc: "Oversee your child's learning: link their profile and follow their progress from your own device.",
    icon: 'M15 19.128a9.38 9.38 0 002.625.372 9.337 9.337 0 004.121-.952 4.125 4.125 0 00-7.533-2.493M15 19.128v-.003c0-1.113-.285-2.16-.786-3.07M15 19.128v.106A12.318 12.318 0 018.624 21c-2.331 0-4.512-.645-6.374-1.766l-.001-.109a6.375 6.375 0 0111.964-3.07M12 6.375a3.375 3.375 0 11-6.75 0 3.375 3.375 0 016.75 0zm8.25 2.25a2.625 2.625 0 11-5.25 0 2.625 2.625 0 015.25 0z',
  },
]

const BIRTHDATE_RE = /^\d{4}-\d{2}-\d{2}$/
const ageYears = computed<number | null>(() => {
  if (!BIRTHDATE_RE.test(birthdate.value)) return null
  const born = new Date(`${birthdate.value}T00:00:00Z`)
  if (Number.isNaN(born.getTime())) return null
  const now = new Date()
  let age = now.getUTCFullYear() - born.getUTCFullYear()
  const monthDay = (d: Date) => (d.getUTCMonth() + 1) * 100 + d.getUTCDate()
  if (monthDay(now) < monthDay(born)) age -= 1
  return age
})
const birthdateValid = computed(() => {
  const a = ageYears.value
  return a !== null && a >= 0 && a <= 120 && new Date(`${birthdate.value}T00:00:00Z`) <= new Date()
})
const isMinorLearner = computed(
  () => selectedRole.value === 'learner' && ageYears.value !== null && ageYears.value < 18,
)
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

  try {
    await refreshProfiles()
    biometricAvailable.value = await biometricSupported()
  } catch {
    biometricAvailable.value = false
  }

  const requestedMode = Array.isArray(route.query.mode) ? route.query.mode[0] : route.query.mode
  if (requestedMode === 'import') {
    startImport()
  } else if (requestedMode === 'create') {
    startCreate()
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
const passwordLength = computed(() => password.value.length)
const mnemonicWords = computed(() => mnemonic.value.trim().split(/\s+/).filter(Boolean))
const importWordCount = computed(() => importMnemonic.value.trim().split(/\s+/).filter(Boolean).length)
const importWordCountValid = computed(() => {
  if (mode.value !== 'import' || importMnemonic.value.trim().length === 0) {
    return true
  }

  return [12, 15, 24].includes(importWordCount.value)
})

const wizardSteps = computed<{ id: Step; label: string }[]>(() => {
  const steps: { id: Step; label: string }[] = [
    { id: 'welcome', label: 'Welcome' },
    { id: 'role', label: 'Your Role' },
  ]
  if (selectedRole.value === 'learner') steps.push({ id: 'birthdate', label: 'Birthdate' })
  steps.push({ id: 'password', label: 'Secure Vault' })
  if (mode.value === 'create') {
    steps.push({ id: 'generating', label: 'Generate Keys' }, { id: 'backup', label: 'Backup Phrase' })
  } else {
    steps.push({ id: 'generating', label: 'Restore Keys' })
  }
  // Learners set goals right after the wallet exists (goals persist to the
  // vault-scoped `learner.targets` synced setting).
  if (selectedRole.value === 'learner') {
    steps.push({ id: 'goals', label: 'Your Goals' })
    steps.push({ id: 'bootstrap', label: 'Your Skills' })
  }
  if (selectedRole.value === 'parent') steps.push({ id: 'link-child', label: 'Link Your Child' })
  steps.push({ id: 'done', label: 'Complete' })
  return steps
})

// ── Learner: goals + skill-bootstrap steps ──────────────────────
const { goals: learnerGoals } = useGoals()
const bootstrapClaimed = ref(0)

// ── Parent: link-child step ─────────────────────────────────────
const childInviteCode = ref('')
const linkingChild = ref(false)
const linkChildError = ref('')
const linkedChildName = ref<string | null>(null)

async function linkChild() {
  if (!childInviteCode.value.trim()) return
  linkingChild.value = true
  linkChildError.value = ''
  try {
    const link = await invoke<{ peer_display_name: string | null; status: string }>(
      'guardian_accept_invite',
      { code: childInviteCode.value.trim() },
    )
    linkedChildName.value = link.peer_display_name ?? 'your child'
    step.value = 'done'
  } catch (e) {
    linkChildError.value = String(e)
  } finally {
    linkingChild.value = false
  }
}
const activeStepIndex = computed(() => {
  const idx = wizardSteps.value.findIndex((s) => s.id === step.value)
  return idx >= 0 ? idx : 0
})
const progressPercent = computed(() => {
  const maxIndex = wizardSteps.value.length - 1
  if (maxIndex <= 0) return 0
  return Math.round((activeStepIndex.value / maxIndex) * 100)
})

function formatOnboardingError(cause: unknown, action: 'create' | 'restore'): string {
  const raw = cause instanceof Error ? cause.message : String(cause)

  if (raw.includes('Password must be at least')) {
    return 'Your vault password must be at least 12 characters long. Add a few more characters, then try again.'
  }

  if (raw.includes('Recovery phrase must be')) {
    return 'That recovery phrase length is not supported. Use a 12-, 15-, or 24-word phrase, then try again.'
  }

  if (raw.toLowerCase().includes('mnemonic') || raw.toLowerCase().includes('checksum')) {
    return 'That recovery phrase does not look valid. Check the word order and spelling, then try again.'
  }

  return action === 'create'
    ? `We couldn't create your wallet yet. ${raw}`
    : `We couldn't restore your wallet yet. ${raw}`
}

function startCreate() {
  mode.value = 'create'
  step.value = 'role'
  error.value = ''
}

function startImport() {
  mode.value = 'import'
  step.value = 'role'
  error.value = ''
}

function chooseRole(role: AccountRole) {
  selectedRole.value = role
  error.value = ''
  if (role !== 'learner') birthdate.value = ''
  step.value = role === 'learner' ? 'birthdate' : 'password'
}

function proceedFromBirthdate() {
  error.value = ''
  if (!birthdateValid.value) {
    error.value = 'Enter a valid birthdate — it cannot be in the future or more than 120 years ago.'
    return
  }
  step.value = 'password'
}

function goBack() {
  error.value = ''
  if (step.value === 'role') {
    step.value = 'welcome'
  } else if (step.value === 'birthdate') {
    step.value = 'role'
  } else if (step.value === 'password') {
    password.value = ''
    confirmPassword.value = ''
    importMnemonic.value = ''
    step.value = selectedRole.value === 'learner' ? 'birthdate' : 'role'
  }
}

async function proceedFromPassword() {
  error.value = ''

  // Username and display name are both mandatory.
  if (!usernameValid.value) {
    error.value = 'Choose a username: 3–32 characters, lowercase letters, numbers, and underscores only.'
    return
  }
  if (availability.value === 'taken') {
    error.value = 'That username is already taken — pick another.'
    return
  }
  if (!displayName.value.trim()) {
    error.value = 'Choose a display name to continue.'
    return
  }

  if (!passwordValid.value) {
    error.value = 'Your vault password is too short. Use at least 12 characters, then try again.'
    return
  }
  if (!passwordsMatch.value) {
    error.value = 'The confirmation password does not match yet. Enter the same password in both fields to continue.'
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
    const result = await createProfile(
      username.value.trim().toLowerCase(),
      displayName.value.trim(),
      password.value,
      undefined,
      {
        role: selectedRole.value ?? undefined,
        birthdate: selectedRole.value === 'learner' ? birthdate.value : undefined,
      },
    )
    mnemonic.value = result.mnemonic
    try {
      if (enableBiometricOnSetup.value && biometricAvailable.value) {
        const mode = await storeVaultPasswordForBiometric(result.summary.id, password.value)
        biometricHint.value = mode === 'secure'
          ? 'Biometric unlock enabled on this device.'
          : 'Biometric unlock enabled for this app session (dev runtime keychain limitation).'
      }
    } catch {
      biometricHint.value = 'Biometric unlock setup skipped. You can still unlock with password.'
    }
    step.value = 'backup'
  } catch (e) {
    error.value = formatOnboardingError(e, 'create')
    step.value = 'password'
  }
}

async function restoreWallet() {
  const phrase = importMnemonic.value.trim()
  if (!phrase) {
    error.value = 'Enter your recovery phrase to continue. Alexandria accepts 12-, 15-, or 24-word phrases.'
    return
  }

  const words = phrase.split(/\s+/)
  if (words.length !== 12 && words.length !== 15 && words.length !== 24) {
    error.value = 'That recovery phrase length is not supported. Use a 12-, 15-, or 24-word phrase, then try again.'
    return
  }

  step.value = 'generating'
  progressLines.value = []
  currentStep.value = ''

  try {
    await restoreProfileWithMnemonic(
      username.value.trim().toLowerCase(),
      displayName.value.trim(),
      phrase,
      password.value,
      undefined,
      {
        role: selectedRole.value ?? undefined,
        birthdate: selectedRole.value === 'learner' ? birthdate.value : undefined,
      },
    )
    try {
      if (enableBiometricOnSetup.value && biometricAvailable.value && activeProfileId.value) {
        const mode = await storeVaultPasswordForBiometric(activeProfileId.value, password.value)
        biometricHint.value = mode === 'secure'
          ? 'Biometric unlock enabled on this device.'
          : 'Biometric unlock enabled for this app session (dev runtime keychain limitation).'
      }
    } catch {
      biometricHint.value = 'Biometric unlock setup skipped. You can still unlock with password.'
    }
    step.value = nextAfterWallet()
  } catch (e) {
    error.value = formatOnboardingError(e, 'restore')
    step.value = 'password'
  }
}

/** After the wallet exists: learners set goals, parents link a child. */
function nextAfterWallet(): Step {
  if (selectedRole.value === 'learner') return 'goals'
  if (selectedRole.value === 'parent') return 'link-child'
  return 'done'
}

async function copyMnemonic() {
  await navigator.clipboard.writeText(mnemonic.value)
  copied.value = true
  setTimeout(() => { copied.value = false }, 2000)
}

function confirmBackup() {
  step.value = nextAfterWallet()
}

function enterApp() {
  // Minors are gated until a parent/guardian accepts their invite;
  // the backend created the profile in `pending_guardian` state.
  // Parents land on their oversight dashboard.
  if (isMinorLearner.value) {
    router.replace('/guardian-gate')
  } else if (selectedRole.value === 'parent') {
    router.replace('/guardian')
  } else {
    router.replace('/home')
  }
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
      <!-- ROLE SELECTION                               -->
      <!-- ============================================ -->
      <div v-else-if="step === 'role'">
        <button
          class="mb-4 text-sm text-muted-foreground hover:text-foreground transition-colors flex items-center gap-1"
          @click="goBack"
        >
          <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
          </svg>
          Back
        </button>

        <h1 class="text-2xl font-bold mb-2 text-center">How will you use Alexandria?</h1>
        <p class="text-sm text-muted-foreground mb-6 text-center">
          This shapes your home screen and tools. Instructors can switch into learner mode any time.
        </p>

        <div class="space-y-3">
          <button
            v-for="card in roleCards"
            :key="card.id"
            class="w-full card p-5 text-left flex items-start gap-4 transition-colors border hover:border-primary/60 hover:bg-primary/5"
            :class="selectedRole === card.id ? 'border-primary bg-primary/5' : 'border-border'"
            @click="chooseRole(card.id)"
          >
            <span class="mt-0.5 flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
              <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" :d="card.icon" />
              </svg>
            </span>
            <span>
              <span class="block text-sm font-semibold text-foreground">{{ card.title }}</span>
              <span class="mt-0.5 block text-sm text-muted-foreground">{{ card.desc }}</span>
            </span>
          </button>
        </div>
      </div>

      <!-- ============================================ -->
      <!-- BIRTHDATE (learners only)                    -->
      <!-- ============================================ -->
      <div v-else-if="step === 'birthdate'">
        <button
          class="mb-4 text-sm text-muted-foreground hover:text-foreground transition-colors flex items-center gap-1"
          @click="goBack"
        >
          <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
          </svg>
          Back
        </button>

        <h1 class="text-2xl font-bold mb-2 text-center">When were you born?</h1>
        <p class="text-sm text-muted-foreground mb-6 text-center">
          Your birthdate stays on this device — it is never published to the network.
        </p>

        <div class="card p-5 mb-4">
          <label class="block text-xs font-medium text-muted-foreground mb-1.5">
            Birthdate
          </label>
          <input
            v-model="birthdate"
            type="date"
            class="w-full px-3 py-2 text-sm rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring"
            @keyup.enter.exact.prevent="proceedFromBirthdate"
          >
          <p v-if="ageYears !== null && birthdateValid" class="mt-2 text-sm text-foreground">
            You are <span class="font-semibold">{{ ageYears }}</span> years old.
          </p>
        </div>

        <div v-if="isMinorLearner" class="card p-4 mb-4 border-warning bg-warning/5">
          <p class="text-sm text-warning font-medium">
            Because you're under 18, a parent or guardian must activate your profile before you can start learning.
            You'll get an invite code for them at the end of setup.
          </p>
        </div>

        <div
          v-if="error"
          class="mb-3 rounded-md border border-error/30 bg-error/5 px-3 py-2"
        >
          <p class="mt-1 text-sm text-error">{{ error }}</p>
        </div>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors disabled:opacity-50"
          :disabled="!birthdateValid"
          @click="proceedFromBirthdate"
        >
          Continue
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
            @keyup.enter.exact.prevent="proceedFromPassword"
          />
          <p
            class="mt-1 text-xs"
            :class="importWordCountValid ? 'text-muted-foreground' : 'text-error'"
          >
            {{ importWordCount }} words entered. Recovery phrases must contain 12, 15, or 24 words.
          </p>
        </div>

        <!-- Profile + password fields -->
        <div class="card p-5 mb-4">
          <div class="space-y-4">
            <div>
              <label class="block text-xs font-medium text-muted-foreground mb-1.5">
                Username
              </label>
              <div class="relative">
                <span class="absolute left-3 top-1/2 -translate-y-1/2 text-sm text-muted-foreground">@</span>
                <input
                  v-model="username"
                  type="text"
                  maxlength="32"
                  autocapitalize="none"
                  autocorrect="off"
                  spellcheck="false"
                  placeholder="your_handle"
                  class="w-full pl-8 pr-3 py-2 text-sm rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring"
                >
              </div>
              <p class="mt-1 text-xs" :class="username && !usernameValid ? 'text-warning' : 'text-muted-foreground'">
                3–32 characters · lowercase letters, numbers, underscores. How others find you.
              </p>
              <p v-if="availability === 'checking'" class="mt-0.5 text-xs text-muted-foreground">
                Checking availability…
              </p>
              <p v-else-if="availability === 'available'" class="mt-0.5 text-xs text-success">
                ✓ @{{ username.trim().toLowerCase() }} is available
              </p>
              <p v-else-if="availability === 'taken'" class="mt-0.5 text-xs text-error">
                ✕ @{{ username.trim().toLowerCase() }} is already taken
              </p>
              <p v-else-if="availability === 'unknown'" class="mt-0.5 text-xs text-warning">
                ⚠ Can't verify availability right now — you can continue, but the name may conflict once online.
              </p>
            </div>
            <div>
              <label class="block text-xs font-medium text-muted-foreground mb-1.5">
                Display name
              </label>
              <input
                v-model="displayName"
                type="text"
                maxlength="64"
                placeholder="Your name as others see it"
                class="w-full px-3 py-2 text-sm rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring"
                @input="displayNameDirty = true"
              >
              <p class="mt-1 text-xs text-muted-foreground">
                Defaults to your username — change it any time.
              </p>
            </div>
            <div>
              <label class="block text-xs font-medium text-muted-foreground mb-1.5">
                Password
              </label>
              <input
                v-model="password"
                type="password"
                placeholder="At least 12 characters"
                class="w-full px-3 py-2 text-sm rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring"
                @keyup.enter.exact.prevent="proceedFromPassword"
              >
              <p
                class="mt-1 text-xs"
                :class="passwordValid ? 'text-success' : 'text-muted-foreground'"
              >
                {{ passwordLength }}/12 characters minimum
              </p>
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
                @keyup.enter.exact.prevent="proceedFromPassword"
              >
              <p
                v-if="confirmPassword && !passwordsMatch"
                class="text-xs text-error mt-1"
              >
                Passwords do not match.
              </p>
              <p
                v-else-if="confirmPassword && passwordsMatch"
                class="text-xs text-success mt-1"
              >
                Passwords match.
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

        <div
          v-if="error"
          class="mb-3 rounded-md border border-error/30 bg-error/5 px-3 py-2"
        >
          <p class="text-xs font-semibold uppercase tracking-wide text-error">Check These Details</p>
          <p class="mt-1 text-sm text-error">{{ error }}</p>
        </div>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors"
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
      <!-- GOALS (learners)                             -->
      <!-- ============================================ -->
      <div v-else-if="step === 'goals'">
        <h1 class="text-2xl font-bold mb-2 text-center">What are you working toward?</h1>
        <p class="text-sm text-muted-foreground mb-6 text-center">
          Pick an exam, curriculum, or job — we'll map it to a skill graph and
          chart your path. You can change or add goals any time.
        </p>

        <GoalPicker @added="() => {}" />

        <div class="mt-6 flex items-center justify-between gap-3">
          <button
            class="text-sm text-muted-foreground hover:text-foreground transition-colors"
            @click="step = 'bootstrap'"
          >
            Skip for now
          </button>
          <button
            class="py-2.5 px-5 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors disabled:opacity-50"
            @click="step = 'bootstrap'"
          >
            {{ learnerGoals.length ? `Continue with ${learnerGoals.length} goal${learnerGoals.length === 1 ? '' : 's'}` : 'Continue' }}
          </button>
        </div>
      </div>

      <!-- ============================================ -->
      <!-- BOOTSTRAP SKILLS (learners)                  -->
      <!-- ============================================ -->
      <div v-else-if="step === 'bootstrap'">
        <h1 class="text-2xl font-bold mb-2 text-center">What do you already know?</h1>
        <p class="text-sm text-muted-foreground mb-6 text-center">
          Upload a resume or transcript and we'll map it to skills you can claim.
          Credentials from accredited schools count for more than a self-made
          resume. You can verify any skill with an assessment later.
        </p>

        <SkillBootstrapPanel @claimed="(n) => { bootstrapClaimed += n }" />

        <div class="mt-6 flex items-center justify-between gap-3">
          <button
            class="text-sm text-muted-foreground hover:text-foreground transition-colors"
            @click="step = 'done'"
          >
            Skip for now
          </button>
          <button
            class="py-2.5 px-5 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors"
            @click="step = 'done'"
          >
            {{ bootstrapClaimed ? `Continue with ${bootstrapClaimed} skill${bootstrapClaimed === 1 ? '' : 's'}` : 'Continue' }}
          </button>
        </div>
      </div>

      <!-- ============================================ -->
      <!-- LINK CHILD (parents)                         -->
      <!-- ============================================ -->
      <div v-else-if="step === 'link-child'">
        <h1 class="text-2xl font-bold mb-2 text-center">Link your child</h1>
        <p class="text-sm text-muted-foreground mb-6 text-center">
          Your child's activation screen shows an invite code. Paste it here to
          activate their profile and follow their learning from this account.
        </p>

        <div class="card p-5 mb-4">
          <label class="block text-xs font-medium text-muted-foreground mb-1.5">
            Invite code
          </label>
          <textarea
            v-model="childInviteCode"
            rows="3"
            placeholder="Paste the invite code from your child's device"
            class="w-full px-3 py-2 text-sm font-mono rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring resize-none"
          />
          <p class="mt-1 text-xs text-muted-foreground">
            Both devices need to be online to complete the link.
          </p>
        </div>

        <div
          v-if="linkChildError"
          class="mb-3 rounded-md border border-error/30 bg-error/5 px-3 py-2"
        >
          <p class="text-sm text-error">{{ linkChildError }}</p>
        </div>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors disabled:opacity-50"
          :disabled="!childInviteCode.trim() || linkingChild"
          @click="linkChild"
        >
          {{ linkingChild ? 'Linking…' : 'Link Child' }}
        </button>

        <button
          class="w-full mt-3 py-2 text-sm text-muted-foreground hover:text-foreground transition-colors"
          @click="step = 'done'"
        >
          Skip for now — I'll add them from my dashboard
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

        <div v-if="isMinorLearner" class="card p-4 mb-4 border-warning bg-warning/5 text-left">
          <p class="text-sm text-warning font-medium">
            One more step: your profile needs a parent or guardian.
            Next you'll get an invite code to share with them — your profile
            activates as soon as they accept it from their own device.
          </p>
        </div>

        <p v-if="linkedChildName" class="text-sm text-success mb-4">
          Linked with {{ linkedChildName }} — their profile is now active.
        </p>

        <button
          class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors"
          @click="enterApp"
        >
          {{ isMinorLearner ? 'Invite My Guardian' : selectedRole === 'parent' ? 'Open My Dashboard' : 'Start Learning' }}
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
