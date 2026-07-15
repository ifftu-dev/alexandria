<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useProfiles } from '@/composables/useProfiles'
import { useLocalApi } from '@/composables/useLocalApi'
import { biometricSupported, storeVaultPasswordForBiometric } from '@/composables/useBiometricVault'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import Starfield from '@/components/auth/Starfield.vue'
import LocaleDropdown from '@/components/settings/LocaleDropdown.vue'
import { BirthdateInput } from '@/components/ui'
import GoalPicker from '@/components/goals/GoalPicker.vue'
import SkillBootstrapPanel from '@/components/skills/SkillBootstrapPanel.vue'
import { useGoals } from '@/composables/useGoals'
import { useLocale } from '@/composables/useLocale'
import { useI18n } from 'vue-i18n'
import type { AccountRole } from '@/types'

const { t } = useI18n()
const router = useRouter()
const route = useRoute()
const { profiles, refreshProfiles, createProfile, restoreProfileWithMnemonic, activeProfileId } = useProfiles()
const { invoke } = useLocalApi()

// Language can be chosen before any profile exists (via the LocaleDropdown on
// the welcome step, which writes to the pre-unlock locale cache); the pick is
// seeded into the new profile on completion.
const { persistLocaleToProfile } = useLocale()

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

const roleCards = computed<{ id: AccountRole; title: string; desc: string; icon: string }[]>(() => [
  {
    id: 'learner',
    title: t('onboarding.roles.learnerTitle'),
    desc: t('onboarding.roles.learnerDesc'),
    icon: 'M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253',
  },
  {
    id: 'instructor',
    title: t('onboarding.roles.instructorTitle'),
    desc: t('onboarding.roles.instructorDesc'),
    icon: 'M8.25 6.75h12M8.25 12h12m-12 5.25h12M3.75 6.75h.007v.008H3.75V6.75zm.375 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zM3.75 12h.007v.008H3.75V12zm.375 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm-.375 5.25h.007v.008H3.75v-.008zm.375 0a.375.375 0 11-.75 0 .375.375 0 01.75 0z',
  },
  {
    id: 'parent',
    title: t('onboarding.roles.parentTitle'),
    desc: t('onboarding.roles.parentDesc'),
    icon: 'M15 19.128a9.38 9.38 0 002.625.372 9.337 9.337 0 004.121-.952 4.125 4.125 0 00-7.533-2.493M15 19.128v-.003c0-1.113-.285-2.16-.786-3.07M15 19.128v.106A12.318 12.318 0 018.624 21c-2.331 0-4.512-.645-6.374-1.766l-.001-.109a6.375 6.375 0 0111.964-3.07M12 6.375a3.375 3.375 0 11-6.75 0 3.375 3.375 0 016.75 0zm8.25 2.25a2.625 2.625 0 11-5.25 0 2.625 2.625 0 015.25 0z',
  },
])

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
    { id: 'welcome', label: t('onboarding.wizard.welcome') },
    { id: 'role', label: t('onboarding.wizard.role') },
  ]
  if (selectedRole.value === 'learner') steps.push({ id: 'birthdate', label: t('onboarding.wizard.birthdate') })
  steps.push({ id: 'password', label: t('onboarding.wizard.password') })
  if (mode.value === 'create') {
    steps.push({ id: 'generating', label: t('onboarding.wizard.createAccount') }, { id: 'backup', label: t('onboarding.wizard.recoveryPhrase') })
  } else {
    steps.push({ id: 'generating', label: t('onboarding.wizard.restoreAccount') })
  }
  // Learners set goals right after the account exists (goals persist to the
  // vault-scoped `learner.targets` synced setting).
  if (selectedRole.value === 'learner') {
    steps.push({ id: 'goals', label: t('onboarding.wizard.goals') })
    steps.push({ id: 'bootstrap', label: t('onboarding.wizard.skills') })
  }
  if (selectedRole.value === 'parent') steps.push({ id: 'link-child', label: t('onboarding.wizard.linkChild') })
  steps.push({ id: 'done', label: t('onboarding.wizard.complete') })
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
    linkedChildName.value = link.peer_display_name ?? t('onboarding.linkChild.childFallback')
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
    return t('onboarding.errors.passwordTooShortDetail')
  }

  if (raw.includes('Recovery phrase must be')) {
    return t('onboarding.errors.phraseLengthUnsupported')
  }

  if (raw.toLowerCase().includes('mnemonic') || raw.toLowerCase().includes('checksum')) {
    return t('onboarding.errors.phraseInvalid')
  }

  return action === 'create'
    ? t('onboarding.errors.createFailed', { error: raw })
    : t('onboarding.errors.restoreFailed', { error: raw })
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
    error.value = t('onboarding.errors.birthdateInvalid')
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
    error.value = t('onboarding.errors.usernameInvalid')
    return
  }
  if (availability.value === 'taken') {
    error.value = t('onboarding.errors.usernameTaken')
    return
  }
  if (!displayName.value.trim()) {
    error.value = t('onboarding.errors.displayNameRequired')
    return
  }

  if (!passwordValid.value) {
    error.value = t('onboarding.errors.passwordTooShort')
    return
  }
  if (!passwordsMatch.value) {
    error.value = t('onboarding.errors.passwordsDoNotMatch')
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
          ? t('onboarding.biometric.enabledSecure')
          : t('onboarding.biometric.enabledSession')
      }
    } catch {
      biometricHint.value = t('onboarding.biometric.skipped')
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
    error.value = t('onboarding.errors.phraseRequired')
    return
  }

  const words = phrase.split(/\s+/)
  if (words.length !== 12 && words.length !== 15 && words.length !== 24) {
    error.value = t('onboarding.errors.phraseLengthUnsupported')
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
          ? t('onboarding.biometric.enabledSecure')
          : t('onboarding.biometric.enabledSession')
      }
    } catch {
      biometricHint.value = t('onboarding.biometric.skipped')
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
  // Carry a pre-unlock language choice into the freshly-created profile.
  void persistLocaleToProfile()
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
    <div class="onb-stars">
      <Starfield />
    </div>

    <div class="w-full max-w-5xl relative z-10">
      <div class="onb-frame grid lg:grid-cols-[300px_minmax(0,1fr)]">
        <aside class="onb-rail hidden lg:flex lg:flex-col">
          <div class="onb-glyph">
            <svg class="w-6 h-6" viewBox="0 0 32 32" fill="none">
              <path d="M16 2L4 8v16l12 6 12-6V8L16 2z" stroke="currentColor" stroke-width="2.2" fill="none" />
              <path d="M16 8v16M8 12l8 4 8-4" stroke="currentColor" stroke-width="2.2" />
            </svg>
          </div>
          <h2 class="onb-lead">{{ $t('onboarding.aside.title') }}</h2>
          <p class="onb-lead-sub">{{ $t('onboarding.aside.subtitle') }}</p>

          <div class="onb-steps">
            <div
              v-for="(wizardStep, index) in wizardSteps"
              :key="wizardStep.id"
              class="onb-step"
              :class="{ 'onb-step--done': index < activeStepIndex, 'onb-step--now': index === activeStepIndex }"
            >
              <span class="onb-step__n">
                <svg v-if="index < activeStepIndex" class="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3.2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="m5 12 5 5L20 6" />
                </svg>
                <template v-else>{{ index + 1 }}</template>
              </span>
              <span class="onb-step__label">{{ wizardStep.label }}</span>
            </div>
          </div>

          <div class="onb-motif">{{ $t('onboarding.motif.ubuntu') }}</div>
        </aside>

        <div class="onb-content">
          <div class="mb-5">
            <div class="onb-kick">{{ $t('onboarding.stepOf', { current: activeStepIndex + 1, total: wizardSteps.length }) }}</div>
            <div class="h-1 rounded-full bg-muted/50 overflow-hidden">
              <div class="h-full bg-primary transition-all duration-500" :style="{ width: `${progressPercent}%` }" />
            </div>
          </div>

      <!-- ============================================ -->
      <!-- WELCOME                                      -->
      <!-- ============================================ -->
      <div v-if="step === 'welcome'" class="text-center">
        <!-- Language picker — chosen before any profile exists; the choice is
             saved to the new profile and synced across devices on completion.
             Compact custom dropdown showing each language in its own script. -->
        <div class="mb-6 flex justify-center">
          <LocaleDropdown />
        </div>

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
          {{ $t('onboarding.motif.ubuntu') }}
        </p>
        <p class="text-muted-foreground mb-8 text-sm">
          {{ $t('onboarding.welcome.tagline') }}
        </p>

        <div class="onb-panel mb-6 text-start">
          <h2 class="text-sm font-semibold mb-3">{{ $t('onboarding.welcome.whatHappens') }}</h2>
          <ul class="space-y-2 text-sm text-muted-foreground">
            <li class="flex items-start gap-2">
              <span class="text-primary mt-0.5 font-mono text-xs w-4 text-end shrink-0">01</span>
              {{ $t('onboarding.welcome.step1') }}
            </li>
            <li class="flex items-start gap-2">
              <span class="text-primary mt-0.5 font-mono text-xs w-4 text-end shrink-0">02</span>
              {{ $t('onboarding.welcome.step2') }}
            </li>
            <li class="flex items-start gap-2">
              <span class="text-primary mt-0.5 font-mono text-xs w-4 text-end shrink-0">03</span>
              {{ $t('onboarding.welcome.step3') }}
            </li>
            <li class="flex items-start gap-2">
              <span class="text-primary mt-0.5 font-mono text-xs w-4 text-end shrink-0">04</span>
              {{ $t('onboarding.welcome.step4') }}
            </li>
          </ul>
        </div>

        <button
          class="onb-cta"
          @click="startCreate"
        >
          {{ $t('onboarding.welcome.createAccount') }}
        </button>

        <button
          class="w-full mt-3 py-2.5 px-4 rounded-md text-sm font-medium border border-border text-foreground hover:bg-muted/50 transition-colors"
          @click="startImport"
        >
          {{ $t('onboarding.welcome.restoreAccount') }}
        </button>

        <button
          v-if="vaultExists"
          class="w-full mt-3 py-2 text-sm text-primary hover:underline transition-colors"
          @click="router.replace('/unlock')"
        >
          {{ $t('onboarding.welcome.signIn') }}
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
          {{ $t('common.actions.back') }}
        </button>

        <h1 class="onb-h2">{{ $t('onboarding.role.heading') }}</h1>
        <p class="onb-sub">{{ $t('onboarding.role.subtitle') }}</p>

        <div class="flex flex-col gap-3">
          <button
            v-for="card in roleCards"
            :key="card.id"
            class="onb-role"
            :class="{ 'onb-role--sel': selectedRole === card.id }"
            @click="chooseRole(card.id)"
          >
            <span class="onb-role__ic">
              <svg class="w-[22px] h-[22px]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
                <path stroke-linecap="round" stroke-linejoin="round" :d="card.icon" />
              </svg>
            </span>
            <span class="onb-role__body">
              <span class="onb-role__title">{{ card.title }}</span>
              <span class="onb-role__desc">{{ card.desc }}</span>
            </span>
            <span class="onb-role__chk">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3.2">
                <path stroke-linecap="round" stroke-linejoin="round" d="m5 12 5 5L20 6" />
              </svg>
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
          {{ $t('common.actions.back') }}
        </button>

        <h1 class="onb-h2">{{ $t('onboarding.birthdate.heading') }}</h1>
        <p class="onb-sub">
          {{ $t('onboarding.birthdate.subtitle') }}
        </p>

        <div class="card p-5 mb-4">
          <label class="block text-xs font-medium text-muted-foreground mb-1.5">
            {{ $t('onboarding.birthdate.label') }}
          </label>
          <BirthdateInput v-model="birthdate" :max-age="120" />
          <p v-if="ageYears !== null && birthdateValid" class="mt-3 text-sm text-foreground">
            {{ $t('onboarding.birthdate.ageStatement', { age: ageYears }) }}
          </p>
        </div>

        <div v-if="isMinorLearner" class="card p-4 mb-4 border-warning bg-warning/5">
          <p class="text-sm text-warning font-medium">
            {{ $t('onboarding.birthdate.minorNotice') }}
          </p>
        </div>

        <div
          v-if="error"
          class="mb-3 rounded-md border border-error/30 bg-error/5 px-3 py-2"
        >
          <p class="mt-1 text-sm text-error">{{ error }}</p>
        </div>

        <button
          class="onb-cta"
          :disabled="!birthdateValid"
          @click="proceedFromBirthdate"
        >
          {{ $t('common.actions.continue') }}
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
          {{ $t('common.actions.back') }}
        </button>

        <h1 class="onb-h2">
          {{ mode === 'create' ? $t('onboarding.password.titleCreate') : $t('onboarding.password.titleImport') }}
        </h1>
        <p class="onb-sub">
          {{ mode === 'create'
            ? $t('onboarding.password.subtitleCreate')
            : $t('onboarding.password.subtitleImport')
          }}
        </p>

        <!-- Import: Recovery Phrase input -->
        <div v-if="mode === 'import'" class="card p-5 mb-4">
          <label class="block text-xs font-medium text-muted-foreground mb-1.5">
            {{ $t('onboarding.password.recoveryLabel') }}
          </label>
          <textarea
            v-model="importMnemonic"
            :placeholder="$t('onboarding.password.recoveryPlaceholder')"
            rows="3"
            class="w-full px-3 py-2 text-sm font-mono rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring resize-none"
            @keyup.enter.exact.prevent="proceedFromPassword"
          />
          <p
            class="mt-1 text-xs"
            :class="importWordCountValid ? 'text-muted-foreground' : 'text-error'"
          >
            {{ $t('onboarding.password.wordsEntered', { count: importWordCount }, importWordCount) }} {{ $t('onboarding.password.wordsRule') }}
          </p>
        </div>

        <!-- Profile + password fields -->
        <div class="card p-5 mb-4">
          <div class="space-y-4">
            <div>
              <label class="block text-xs font-medium text-muted-foreground mb-1.5">
                {{ $t('onboarding.password.usernameLabel') }}
              </label>
              <div class="relative">
                <span class="absolute start-3 top-1/2 -translate-y-1/2 text-sm text-muted-foreground">@</span>
                <input
                  v-model="username"
                  type="text"
                  maxlength="32"
                  autocapitalize="none"
                  autocorrect="off"
                  spellcheck="false"
                  :placeholder="$t('onboarding.password.usernamePlaceholder')"
                  class="w-full ps-8 pe-3 py-2 text-sm rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring"
                >
              </div>
              <p class="mt-1 text-xs" :class="username && !usernameValid ? 'text-warning' : 'text-muted-foreground'">
                {{ $t('onboarding.password.usernameHint') }}
              </p>
              <p v-if="availability === 'checking'" class="mt-0.5 text-xs text-muted-foreground">
                {{ $t('onboarding.password.checking') }}
              </p>
              <p v-else-if="availability === 'available'" class="mt-0.5 text-xs text-success">
                {{ $t('onboarding.password.usernameAvailable', { handle: username.trim().toLowerCase() }) }}
              </p>
              <p v-else-if="availability === 'taken'" class="mt-0.5 text-xs text-error">
                {{ $t('onboarding.password.usernameTaken', { handle: username.trim().toLowerCase() }) }}
              </p>
              <p v-else-if="availability === 'unknown'" class="mt-0.5 text-xs text-warning">
                {{ $t('onboarding.password.usernameUnknown') }}
              </p>
            </div>
            <div>
              <label class="block text-xs font-medium text-muted-foreground mb-1.5">
                {{ $t('onboarding.password.displayNameLabel') }}
              </label>
              <input
                v-model="displayName"
                type="text"
                maxlength="64"
                :placeholder="$t('onboarding.password.displayNamePlaceholder')"
                class="w-full px-3 py-2 text-sm rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring"
                @input="displayNameDirty = true"
              >
              <p class="mt-1 text-xs text-muted-foreground">
                {{ $t('onboarding.password.displayNameHint') }}
              </p>
            </div>
            <div>
              <label class="block text-xs font-medium text-muted-foreground mb-1.5">
                {{ $t('onboarding.password.passwordLabel') }}
              </label>
              <input
                v-model="password"
                type="password"
                :placeholder="$t('onboarding.password.passwordPlaceholder')"
                class="w-full px-3 py-2 text-sm rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring"
                @keyup.enter.exact.prevent="proceedFromPassword"
              >
              <p
                class="mt-1 text-xs"
                :class="passwordValid ? 'text-success' : 'text-muted-foreground'"
              >
                {{ $t('onboarding.password.passwordCounter', { count: passwordLength }) }}
              </p>
            </div>
            <div>
              <label class="block text-xs font-medium text-muted-foreground mb-1.5">
                {{ $t('onboarding.password.confirmLabel') }}
              </label>
              <input
                v-model="confirmPassword"
                type="password"
                :placeholder="$t('onboarding.password.confirmPlaceholder')"
                class="w-full px-3 py-2 text-sm rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring"
                @keyup.enter.exact.prevent="proceedFromPassword"
              >
              <p
                v-if="confirmPassword && !passwordsMatch"
                class="text-xs text-error mt-1"
              >
                {{ $t('onboarding.password.passwordsMismatch') }}
              </p>
              <p
                v-else-if="confirmPassword && passwordsMatch"
                class="text-xs text-success mt-1"
              >
                {{ $t('onboarding.password.passwordsMatch') }}
              </p>
            </div>
          </div>
        </div>

        <div class="card p-4 mb-4 border-warning bg-warning/5">
          <p class="text-sm text-warning font-medium">
            {{ $t('onboarding.password.noRecoveryWarning') }}
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
              <span class="block text-sm font-medium text-foreground">{{ $t('onboarding.password.biometricLabel') }}</span>
              <span class="block text-xs text-muted-foreground mt-0.5">
                {{ $t('onboarding.password.biometricHint') }}
              </span>
            </span>
          </label>
        </div>

        <div
          v-if="error"
          class="mb-3 rounded-md border border-error/30 bg-error/5 px-3 py-2"
        >
          <p class="text-xs font-semibold uppercase tracking-wide text-error">{{ $t('onboarding.password.errorHeading') }}</p>
          <p class="mt-1 text-sm text-error">{{ error }}</p>
        </div>

        <button
          class="onb-cta"
          @click="proceedFromPassword"
        >
          {{ mode === 'create' ? $t('onboarding.password.submitCreate') : $t('onboarding.password.submitImport') }}
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
          {{ mode === 'create' ? $t('onboarding.generating.titleCreate') : $t('onboarding.generating.titleImport') }}
        </h2>
        <p class="text-sm text-muted-foreground mb-6">
          {{ $t('onboarding.generating.subtitle') }}
        </p>

        <!-- Live log output -->
        <div class="card p-4 text-start mb-4">
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
              <span>{{ $t('onboarding.generating.initializing') }}</span>
            </div>
          </div>
        </div>

      </div>

      <!-- ============================================ -->
      <!-- BACKUP                                       -->
      <!-- ============================================ -->
      <div v-else-if="step === 'backup'" class="text-center">
        <h1 class="onb-h2">{{ $t('onboarding.backup.heading') }}</h1>
        <p class="text-sm text-muted-foreground mb-6">
          {{ $t('onboarding.backup.subtitle') }}
        </p>

        <div class="card p-5 mb-6">
          <div class="grid grid-cols-2 sm:grid-cols-3 gap-2">
            <div
              v-for="(word, i) in mnemonicWords"
              :key="i"
              class="flex items-center gap-2 text-sm py-1.5 px-2.5 rounded bg-muted/30"
            >
              <span class="text-xs text-muted-foreground w-5 text-end font-mono">{{ String(i + 1).padStart(2, '0') }}</span>
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
            {{ copied ? $t('onboarding.backup.copied') : $t('onboarding.backup.copy') }}
          </button>
        </div>

        <div class="card p-4 mb-6 border-warning bg-warning/5">
          <p class="text-sm text-warning font-medium">
            {{ $t('onboarding.backup.warning') }}
          </p>
        </div>

        <button
          class="onb-cta"
          @click="confirmBackup"
        >
          {{ $t('onboarding.backup.confirm') }}
        </button>
      </div>

      <!-- ============================================ -->
      <!-- GOALS (learners)                             -->
      <!-- ============================================ -->
      <div v-else-if="step === 'goals'">
        <h1 class="onb-h2">{{ $t('onboarding.goals.heading') }}</h1>
        <p class="onb-sub">
          {{ $t('onboarding.goals.subtitle') }}
        </p>

        <GoalPicker @added="() => {}" />

        <div class="mt-6 flex items-center justify-between gap-3">
          <button
            class="text-sm text-muted-foreground hover:text-foreground transition-colors"
            @click="step = 'bootstrap'"
          >
            {{ $t('onboarding.goals.skip') }}
          </button>
          <button
            class="py-2.5 px-5 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors disabled:opacity-50"
            @click="step = 'bootstrap'"
          >
            {{ $t('onboarding.goals.continue', { count: learnerGoals.length }, learnerGoals.length) }}
          </button>
        </div>
      </div>

      <!-- ============================================ -->
      <!-- BOOTSTRAP SKILLS (learners)                  -->
      <!-- ============================================ -->
      <div v-else-if="step === 'bootstrap'">
        <h1 class="onb-h2">{{ $t('onboarding.bootstrap.heading') }}</h1>
        <p class="onb-sub">
          {{ $t('onboarding.bootstrap.subtitle') }}
        </p>

        <SkillBootstrapPanel @claimed="(n) => { bootstrapClaimed += n }" />

        <div class="mt-6 flex items-center justify-between gap-3">
          <button
            class="text-sm text-muted-foreground hover:text-foreground transition-colors"
            @click="step = 'done'"
          >
            {{ $t('onboarding.bootstrap.skip') }}
          </button>
          <button
            class="py-2.5 px-5 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors"
            @click="step = 'done'"
          >
            {{ $t('onboarding.bootstrap.continue', { count: bootstrapClaimed }, bootstrapClaimed) }}
          </button>
        </div>
      </div>

      <!-- ============================================ -->
      <!-- LINK CHILD (parents)                         -->
      <!-- ============================================ -->
      <div v-else-if="step === 'link-child'">
        <h1 class="onb-h2">{{ $t('onboarding.linkChild.heading') }}</h1>
        <p class="onb-sub">
          {{ $t('onboarding.linkChild.subtitle') }}
        </p>

        <div class="card p-5 mb-4">
          <label class="block text-xs font-medium text-muted-foreground mb-1.5">
            {{ $t('onboarding.linkChild.label') }}
          </label>
          <textarea
            v-model="childInviteCode"
            rows="3"
            :placeholder="$t('onboarding.linkChild.placeholder')"
            class="w-full px-3 py-2 text-sm font-mono rounded-md border border-border bg-background focus:outline-none focus:ring-2 focus:ring-ring resize-none"
          />
          <p class="mt-1 text-xs text-muted-foreground">
            {{ $t('onboarding.linkChild.hint') }}
          </p>
        </div>

        <div
          v-if="linkChildError"
          class="mb-3 rounded-md border border-error/30 bg-error/5 px-3 py-2"
        >
          <p class="text-sm text-error">{{ linkChildError }}</p>
        </div>

        <button
          class="onb-cta"
          :disabled="!childInviteCode.trim() || linkingChild"
          @click="linkChild"
        >
          {{ linkingChild ? $t('onboarding.linkChild.linking') : $t('onboarding.linkChild.submit') }}
        </button>

        <button
          class="w-full mt-3 py-2 text-sm text-muted-foreground hover:text-foreground transition-colors"
          @click="step = 'done'"
        >
          {{ $t('onboarding.linkChild.skip') }}
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
        <h1 class="onb-h2">{{ $t('onboarding.done.heading') }}</h1>
        <p class="text-muted-foreground mb-2">
          {{ mode === 'create'
            ? $t('onboarding.done.bodyCreate')
            : $t('onboarding.done.bodyImport')
          }}
        </p>
        <p class="text-sm text-muted-foreground mb-6">
          {{ $t('onboarding.done.dataNote') }}
        </p>
        <p v-if="biometricHint" class="text-xs text-muted-foreground mb-4">
          {{ biometricHint }}
        </p>

        <div v-if="isMinorLearner" class="card p-4 mb-4 border-warning bg-warning/5 text-start">
          <p class="text-sm text-warning font-medium">
            {{ $t('onboarding.done.minorNotice') }}
          </p>
        </div>

        <p v-if="linkedChildName" class="text-sm text-success mb-4">
          {{ $t('onboarding.done.linkedWith', { name: linkedChildName }) }}
        </p>

        <button
          class="onb-cta"
          @click="enterApp"
        >
          {{ isMinorLearner ? $t('onboarding.done.enterGuardian') : selectedRole === 'parent' ? $t('onboarding.done.enterParent') : $t('onboarding.done.enterLearner') }}
        </button>

        <p class="text-xs text-muted-foreground mt-4 italic tracking-wide">
          {{ $t('onboarding.motif.ubuntu') }}
        </p>
      </div>

        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Onboarding shell — a single framed "app window": gradient brand rail with a
   numbered step list on the left, content on the right. Matches the accent-
   options design study. Colours come from the live theme tokens (stays indigo). */
/* Single container: one framed glass window sitting on the subtle starfield.
   The frame is translucent + blurred so the starfield reads faintly through
   the whole surface, rather than stacking an opaque card on top of the page. */
/* Starfield: kept faint so it reads as texture behind the one glass frame. */
.onb-stars {
  position: absolute;
  inset: 0;
  opacity: 0.4;
  pointer-events: none;
}
.onb-frame {
  border: 1px solid color-mix(in srgb, var(--app-border) 80%, transparent);
  border-radius: 1rem;
  overflow: hidden;
  background: color-mix(in srgb, var(--app-background) 76%, transparent);
  backdrop-filter: blur(14px) saturate(120%);
  -webkit-backdrop-filter: blur(14px) saturate(120%);
  box-shadow: 0 24px 60px -20px rgb(0 0 0 / 0.7);
}
.onb-rail {
  padding: 1.875rem 1.625rem;
  background: linear-gradient(180deg, color-mix(in srgb, var(--app-primary) 10%, transparent), transparent);
  border-inline-end: 1px solid color-mix(in srgb, var(--app-border) 70%, transparent);
}
.onb-glyph {
  width: 2.375rem;
  height: 2.375rem;
  border-radius: 0.6875rem;
  display: grid;
  place-items: center;
  background: var(--app-primary);
  color: var(--app-primary-foreground);
  box-shadow: 0 6px 18px -6px var(--app-primary);
  margin-bottom: 1.625rem;
}
.onb-lead {
  font-size: 1.1875rem;
  font-weight: 650;
  line-height: 1.32;
  letter-spacing: -0.015em;
  color: var(--app-foreground);
  margin: 0;
  text-wrap: balance;
}
.onb-lead-sub {
  font-size: 0.8125rem;
  color: var(--app-muted-foreground);
  margin: 0.375rem 0 1.875rem;
  line-height: 1.5;
}
.onb-steps {
  display: flex;
  flex-direction: column;
  gap: 0.1rem;
}
.onb-step {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.5rem 0;
  font-size: 0.8125rem;
  color: var(--app-muted-foreground);
}
.onb-step__n {
  width: 1.5rem;
  height: 1.5rem;
  border-radius: 50%;
  flex: none;
  display: grid;
  place-items: center;
  font-size: 0.6875rem;
  font-weight: 700;
  border: 1.5px solid var(--app-border);
  background: transparent;
  transition: border-color 0.15s, background-color 0.15s, box-shadow 0.15s;
}
.onb-step__label {
  font-weight: 500;
}
.onb-step--done {
  color: var(--app-foreground);
}
.onb-step--done .onb-step__n {
  background: var(--app-primary);
  color: var(--app-primary-foreground);
  border-color: var(--app-primary);
}
.onb-step--now {
  color: var(--app-foreground);
  font-weight: 600;
}
.onb-step--now .onb-step__n {
  border-color: var(--app-primary);
  color: var(--app-primary);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--app-primary) 15%, transparent);
}
.onb-motif {
  margin-top: auto;
  padding-top: 1.5rem;
  font-size: 0.6875rem;
  font-style: italic;
  letter-spacing: 0.02em;
  color: var(--app-muted-foreground);
}
.onb-content {
  padding: 1.75rem 1.5rem;
}
@media (min-width: 1024px) {
  .onb-content {
    padding: 2.75rem 2.875rem;
  }
}
.onb-kick {
  font-size: 0.6875rem;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--app-primary);
  margin-bottom: 0.625rem;
}
/* Step heading + subtitle — matched to the accent-options study. */
.onb-h2 {
  font-size: 1.625rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  line-height: 1.15;
  color: var(--app-foreground);
  margin: 0 0 0.4375rem;
  text-wrap: balance;
}
.onb-sub {
  font-size: 0.875rem;
  line-height: 1.5;
  color: var(--app-muted-foreground);
  margin: 0 0 1.625rem;
}
/* Subtle inline panel — a faint tint on the glass frame rather than a second
   opaque card, so it reads as content within the one container. */
.onb-panel {
  border-radius: 0.8125rem;
  background: color-mix(in srgb, var(--app-foreground) 4%, transparent);
  border: 1px solid color-mix(in srgb, var(--app-border) 60%, transparent);
  padding: 1.25rem 1.375rem;
}
/* Role / option cards — icon tile + title + subtitle + check radio. */
.onb-role {
  display: flex;
  align-items: center;
  gap: 0.9375rem;
  width: 100%;
  padding: 1rem 1.0625rem;
  border-radius: 0.8125rem;
  background: var(--app-card);
  border: 1.5px solid var(--app-border);
  text-align: start;
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s;
}
.onb-role:hover {
  border-color: color-mix(in srgb, var(--app-primary) 55%, var(--app-border));
}
.onb-role--sel {
  border-color: var(--app-primary);
  background: color-mix(in srgb, var(--app-primary) 13%, transparent);
}
.onb-role__ic {
  width: 2.75rem;
  height: 2.75rem;
  border-radius: 0.6875rem;
  flex: none;
  display: grid;
  place-items: center;
  background: color-mix(in srgb, var(--app-foreground) 8%, transparent);
  color: var(--app-muted-foreground);
  transition: background 0.15s, color 0.15s;
}
.onb-role--sel .onb-role__ic {
  background: var(--app-primary);
  color: var(--app-primary-foreground);
}
.onb-role__body {
  flex: 1;
  min-width: 0;
}
.onb-role__title {
  display: block;
  font-size: 0.9375rem;
  font-weight: 650;
  color: var(--app-foreground);
}
.onb-role__desc {
  display: block;
  margin-top: 0.125rem;
  font-size: 0.78125rem;
  line-height: 1.4;
  color: var(--app-muted-foreground);
}
.onb-role__chk {
  width: 1.375rem;
  height: 1.375rem;
  border-radius: 50%;
  flex: none;
  border: 2px solid var(--app-border);
  display: grid;
  place-items: center;
  transition: background 0.15s, border-color 0.15s;
}
.onb-role--sel .onb-role__chk {
  background: var(--app-primary);
  border-color: var(--app-primary);
}
.onb-role__chk svg {
  width: 0.8125rem;
  height: 0.8125rem;
  stroke: var(--app-primary-foreground);
  opacity: 0;
}
.onb-role--sel .onb-role__chk svg {
  opacity: 1;
}
/* Primary call-to-action — accent fill with a soft glow. */
.onb-cta {
  width: 100%;
  height: 3rem;
  border: none;
  border-radius: 0.75rem;
  background: var(--app-primary);
  color: var(--app-primary-foreground);
  font-size: 0.9375rem;
  font-weight: 700;
  cursor: pointer;
  transition: background 0.15s, box-shadow 0.15s, opacity 0.15s;
  box-shadow: 0 10px 24px -10px var(--app-primary);
}
.onb-cta:hover:not(:disabled) {
  background: var(--app-primary-hover);
}
.onb-cta:disabled {
  opacity: 0.5;
  cursor: default;
  box-shadow: none;
}
</style>
