<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { AppButton, AppBadge, EmptyState } from '@/components/ui'
import { useSentinel } from '@/composables/useSentinel'
import { useLocalApi } from '@/composables/useLocalApi'
import SentinelTrainingWizard from '@/components/integrity/SentinelTrainingWizard.vue'
import type { IntegritySession } from '@/types'

const { invoke } = useLocalApi()
const { getProfile, getAIModelStatus, resetProfile } = useSentinel()

const showWizard = ref(false)
const sessions = ref<IntegritySession[]>([])
const loading = ref(true)
const profile = ref<Record<string, unknown> | null>(null)
const aiStatus = ref<{
  keystrokeAE: { trained: boolean; loss: number; samples: number } | null
  mouseCNN: { trained: boolean; loss: number; samples: number } | null
  faceEmbedder: { enrolled: boolean; progress: number } | null
} | null>(null)

const activeTab = ref<'overview' | 'sessions' | 'signals' | 'profile'>('overview')

// ---------------------------------------------------------------------------
// Signal weights (matches mark2 sentinel spec)
// ---------------------------------------------------------------------------
const signalWeights = [
  { name: 'Typing Consistency', key: 'typing_consistency', weight: 20, description: 'EMA-based deviation from stored typing profile (dwell time, flight time, WPM)' },
  { name: 'Mouse Consistency', key: 'mouse_consistency', weight: 15, description: 'Velocity and acceleration deviation from stored mouse movement profile' },
  { name: 'Human Likelihood', key: 'is_human_likely', weight: 15, description: 'Velocity variance check — bots exhibit constant speed with zero variance' },
  { name: 'Tab Focus', key: 'tab_switches', weight: 15, description: 'Tab/window focus changes during assessment — excessive switching is flagged' },
  { name: 'Paste Activity', key: 'paste_events', weight: 10, description: 'Clipboard paste event count and total pasted character volume' },
  { name: 'DevTools Detection', key: 'devtools_detected', weight: 10, description: 'Browser developer tools heuristic — checks for debugger, firebug, and DOM probing' },
  { name: 'Camera Verification', key: 'face_present', weight: 15, description: 'Continuous face verification every 3s via LBP embeddings (camera opt-in only)' },
]

// ---------------------------------------------------------------------------
// Anomaly flag types
// ---------------------------------------------------------------------------
const anomalyFlagTypes = [
  { type: 'tab_switching', severity: 'warning' as const, description: 'Excessive tab/window switching', trigger: '> 10 switches per snapshot window' },
  { type: 'paste_detected', severity: 'warning' as const, description: 'Clipboard paste activity detected', trigger: '> 500 characters pasted' },
  { type: 'devtools_detected', severity: 'critical' as const, description: 'Browser developer tools opened', trigger: 'DevTools heuristic returns true' },
  { type: 'bot_suspected', severity: 'critical' as const, description: 'Automated input pattern detected', trigger: 'Mouse velocity variance near zero' },
  { type: 'no_face', severity: 'info' as const, description: 'No face detected in camera frame', trigger: 'Face absent on camera-opted session' },
  { type: 'multiple_faces', severity: 'warning' as const, description: 'Multiple faces in camera frame', trigger: 'Face count > 1 during verification' },
  { type: 'multi_account', severity: 'critical' as const, description: 'Device fingerprint linked to multiple accounts', trigger: 'Same device FP across > 1 user' },
  { type: 'low_integrity', severity: 'warning' as const, description: 'Integrity score below safe threshold', trigger: 'Composite score < 0.40' },
  { type: 'behavior_shift', severity: 'warning' as const, description: 'Significant behavioral profile deviation', trigger: 'Consistency score < 0.35' },
]

// ---------------------------------------------------------------------------
// Computed
// ---------------------------------------------------------------------------

function getOutcome(session: IntegritySession): 'clean' | 'flagged' | 'suspended' {
  const score = session.integrity_score
  if (score === null || score === undefined) return 'clean'
  if (score >= 0.8) return 'clean'
  if (score >= 0.4) return 'flagged'
  return 'suspended'
}

const sessionBreakdown = computed(() => {
  const total = sessions.value.length
  if (total === 0) return { clean: 0, flagged: 0, suspended: 0, cleanPct: 0, flaggedPct: 0, suspendedPct: 0 }

  let clean = 0
  let flagged = 0
  let suspended = 0

  for (const s of sessions.value) {
    const outcome = getOutcome(s)
    if (outcome === 'clean') clean++
    else if (outcome === 'flagged') flagged++
    else suspended++
  }

  return {
    clean,
    flagged,
    suspended,
    cleanPct: Math.round((clean / total) * 100),
    flaggedPct: Math.round((flagged / total) * 100),
    suspendedPct: Math.round((suspended / total) * 100),
  }
})

const integrityPercent = computed(() => {
  const scored = sessions.value.filter(s => s.integrity_score !== null && s.integrity_score !== undefined)
  if (scored.length === 0) return 0
  const avg = scored.reduce((sum, s) => sum + (s.integrity_score ?? 0), 0) / scored.length
  return Math.round(avg * 100)
})

const consistencyPercent = computed(() => {
  // Consistency is derived from integrity variance — higher integrity with less variance = more consistent
  const scored = sessions.value.filter(s => s.integrity_score !== null && s.integrity_score !== undefined)
  if (scored.length < 2) return scored.length === 1 ? Math.round((scored[0]?.integrity_score ?? 0) * 100) : 0
  const scores = scored.map(s => s.integrity_score ?? 0)
  const mean = scores.reduce((a, b) => a + b, 0) / scores.length
  const variance = scores.reduce((sum, s) => sum + (s - mean) ** 2, 0) / scores.length
  const stdDev = Math.sqrt(variance)
  // Map std deviation to consistency: 0 stddev = 100%, 0.5 stddev = 0%
  return Math.round(Math.max(0, Math.min(100, (1 - stdDev * 2) * 100)))
})

// ---------------------------------------------------------------------------
// Data loading
// ---------------------------------------------------------------------------

onMounted(async () => {
  await loadData()
})

async function loadData() {
  loading.value = true
  try {
    sessions.value = await invoke<IntegritySession[]>('integrity_list_sessions')
    profile.value = getProfile() as unknown as Record<string, unknown>
    aiStatus.value = getAIModelStatus()
  } catch (e) {
    console.error('Failed to load sentinel data:', e)
  } finally {
    loading.value = false
  }
}

function onWizardComplete() {
  showWizard.value = false
  loadData()
}

async function handleResetProfile() {
  await resetProfile()
  profile.value = null
  aiStatus.value = getAIModelStatus()
}

function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function scoreColor(score: number): 'success' | 'warning' | 'error' {
  if (score >= 0.8) return 'success'
  if (score >= 0.5) return 'warning'
  return 'error'
}

function outcomeBadgeVariant(outcome: string): 'success' | 'warning' | 'error' {
  if (outcome === 'clean') return 'success'
  if (outcome === 'flagged') return 'warning'
  return 'error'
}

function severityBadgeVariant(severity: string): 'primary' | 'warning' | 'error' {
  if (severity === 'info') return 'primary'
  if (severity === 'warning') return 'warning'
  return 'error'
}
</script>

<template>
  <div class="space-y-6">
    <!-- ================================================================== -->
    <!-- HEADER                                                             -->
    <!-- ================================================================== -->
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-lg bg-primary/10">
          <svg class="h-5 w-5 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
          </svg>
        </div>
        <div>
          <h1 class="text-xl font-bold text-foreground">Sentinel</h1>
          <p class="mt-0.5 text-sm text-muted-foreground">
            Assessment integrity monitoring
          </p>
        </div>
      </div>
      <div class="flex items-center gap-2">
        <AppButton
          v-if="profile"
          variant="ghost"
          size="sm"
          @click="handleResetProfile"
        >
          Reset Profile
        </AppButton>
        <AppButton
          variant="primary"
          size="sm"
          @click="showWizard = true"
        >
          {{ profile ? 'Recalibrate' : 'Train Sentinel' }}
        </AppButton>
      </div>
    </div>

    <!-- ================================================================== -->
    <!-- TRAINING WIZARD                                                    -->
    <!-- ================================================================== -->
    <div v-if="showWizard" class="mx-auto max-w-2xl">
      <SentinelTrainingWizard
        @complete="onWizardComplete"
        @cancel="showWizard = false"
      />
    </div>

    <!-- ================================================================== -->
    <!-- MAIN CONTENT                                                       -->
    <!-- ================================================================== -->
    <div v-if="!showWizard" class="space-y-6">

      <!-- Privacy Banner -->
      <div class="rounded-lg border border-emerald-200 bg-emerald-50 p-4 dark:border-emerald-800/40 dark:bg-emerald-900/20">
        <div class="flex gap-3">
          <svg class="h-5 w-5 flex-shrink-0 text-emerald-600 dark:text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M16.5 10.5V6.75a4.5 4.5 0 10-9 0v3.75m-.75 11.25h10.5a2.25 2.25 0 002.25-2.25v-6.75a2.25 2.25 0 00-2.25-2.25H6.75a2.25 2.25 0 00-2.25 2.25v6.75a2.25 2.25 0 002.25 2.25z" />
          </svg>
          <div>
            <p class="text-sm font-medium text-emerald-800 dark:text-emerald-300">Privacy by Design</p>
            <p class="mt-1 text-xs text-emerald-700 dark:text-emerald-400">
              Raw biometric data (keystrokes, mouse coordinates, video frames) never leaves your device.
              Only derived scores (0.0–1.0) are stored locally and used in evidence records.
              Your behavioral profile is stored in browser localStorage and can be deleted at any time.
            </p>
          </div>
        </div>
      </div>

      <!-- ================================================================ -->
      <!-- TAB SWITCHER                                                     -->
      <!-- ================================================================ -->
      <div class="rounded-lg bg-muted p-1">
        <div class="flex gap-1">
          <button
            v-for="tab in (['overview', 'sessions', 'signals', 'profile'] as const)"
            :key="tab"
            class="flex-1 rounded-md px-3 py-2 text-sm font-medium transition-colors"
            :class="activeTab === tab
              ? 'bg-card text-foreground shadow-sm'
              : 'text-muted-foreground hover:text-foreground'"
            @click="activeTab = tab"
          >
            {{ tab === 'overview' ? 'Overview' : tab === 'sessions' ? 'Sessions' : tab === 'signals' ? 'Signals & Weights' : 'Profile & Flags' }}
          </button>
        </div>
      </div>

      <!-- ================================================================ -->
      <!-- SKELETON LOADER                                                  -->
      <!-- ================================================================ -->
      <div v-if="loading" class="space-y-6">
        <!-- Stats skeleton -->
        <div class="grid grid-cols-2 gap-4 sm:grid-cols-4">
          <div v-for="i in 4" :key="i" class="rounded-lg shadow-sm p-4">
            <div class="mb-2 h-3 w-20 animate-pulse rounded bg-muted" />
            <div class="h-7 w-16 animate-pulse rounded bg-muted" />
          </div>
        </div>
        <!-- Content skeleton -->
        <div class="space-y-3">
          <div v-for="i in 3" :key="i" class="rounded-lg shadow-sm p-4">
            <div class="mb-3 h-4 w-48 animate-pulse rounded bg-muted" />
            <div class="h-3 w-full animate-pulse rounded bg-muted" />
            <div class="mt-2 h-3 w-3/4 animate-pulse rounded bg-muted" />
          </div>
        </div>
      </div>

      <!-- ================================================================ -->
      <!-- OVERVIEW TAB                                                     -->
      <!-- ================================================================ -->
      <template v-else-if="activeTab === 'overview'">
        <!-- Stats Grid -->
        <div class="grid grid-cols-2 gap-4 sm:grid-cols-4">
          <!-- Total Sessions -->
          <div class="rounded-lg shadow-sm p-4">
            <p class="text-xs font-medium text-muted-foreground">Total Sessions</p>
            <p class="mt-1 text-2xl font-bold text-foreground">{{ sessions.length }}</p>
          </div>
          <!-- Avg Integrity -->
          <div class="rounded-lg shadow-sm p-4">
            <p class="text-xs font-medium text-muted-foreground">Avg Integrity</p>
            <p class="mt-1 text-2xl font-bold text-foreground">{{ integrityPercent }}%</p>
          </div>
          <!-- Avg Consistency -->
          <div class="rounded-lg shadow-sm p-4">
            <p class="text-xs font-medium text-muted-foreground">Avg Consistency</p>
            <p class="mt-1 text-2xl font-bold text-foreground">{{ consistencyPercent }}%</p>
          </div>
          <!-- Clean Rate -->
          <div class="rounded-lg shadow-sm p-4">
            <p class="text-xs font-medium text-muted-foreground">Clean Rate</p>
            <p class="mt-1 text-2xl font-bold text-foreground">{{ sessionBreakdown.cleanPct }}%</p>
          </div>
        </div>

        <!-- Session Outcome Breakdown -->
        <div class="card p-5">
          <h2 class="mb-4 text-sm font-semibold text-foreground">Session Outcome Breakdown</h2>

          <div v-if="sessions.length === 0" class="py-6 text-center text-sm text-muted-foreground">
            No sessions recorded yet. Outcomes will appear after you complete assessments.
          </div>

          <template v-else>
            <!-- Stacked bar -->
            <div class="flex h-3 overflow-hidden rounded-full">
              <div
                v-if="sessionBreakdown.cleanPct > 0"
                class="bg-emerald-500 transition-all duration-500"
                :style="{ width: sessionBreakdown.cleanPct + '%' }"
              />
              <div
                v-if="sessionBreakdown.flaggedPct > 0"
                class="bg-amber-500 transition-all duration-500"
                :style="{ width: sessionBreakdown.flaggedPct + '%' }"
              />
              <div
                v-if="sessionBreakdown.suspendedPct > 0"
                class="bg-red-500 transition-all duration-500"
                :style="{ width: sessionBreakdown.suspendedPct + '%' }"
              />
            </div>

            <!-- Legend -->
            <div class="mt-3 flex items-center gap-6 text-xs">
              <div class="flex items-center gap-1.5">
                <span class="inline-block h-2.5 w-2.5 rounded-full bg-emerald-500" />
                <span class="text-muted-foreground">Clean</span>
                <span class="font-medium text-foreground">{{ sessionBreakdown.clean }}</span>
              </div>
              <div class="flex items-center gap-1.5">
                <span class="inline-block h-2.5 w-2.5 rounded-full bg-amber-500" />
                <span class="text-muted-foreground">Flagged</span>
                <span class="font-medium text-foreground">{{ sessionBreakdown.flagged }}</span>
              </div>
              <div class="flex items-center gap-1.5">
                <span class="inline-block h-2.5 w-2.5 rounded-full bg-red-500" />
                <span class="text-muted-foreground">Suspended</span>
                <span class="font-medium text-foreground">{{ sessionBreakdown.suspended }}</span>
              </div>
            </div>
          </template>
        </div>

        <!-- Engine Status -->
        <div class="card p-5">
          <div class="mb-4 flex items-center justify-between">
            <h2 class="text-sm font-semibold text-foreground">Engine Status</h2>
            <AppBadge :variant="profile ? 'success' : 'warning'">
              {{ profile ? 'Calibrated' : 'Not Calibrated' }}
            </AppBadge>
          </div>

          <div v-if="!profile" class="rounded-lg border border-dashed border-border p-6 text-center">
            <svg class="mx-auto h-8 w-8 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
            </svg>
            <p class="mt-2 text-sm font-medium text-foreground">No profile calibrated</p>
            <p class="mt-1 text-xs text-muted-foreground">
              Run the training wizard to calibrate Sentinel to your behavioral patterns.
            </p>
            <AppButton variant="primary" size="sm" class="mt-4" @click="showWizard = true">
              Train Sentinel
            </AppButton>
          </div>

          <div v-else class="grid grid-cols-1 sm:grid-cols-3 gap-3">
            <div class="rounded-lg shadow-sm p-3 text-center">
              <svg class="mx-auto h-5 w-5 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M3.75 6.75h16.5M3.75 12h16.5M12 17.25h8.25" />
              </svg>
              <p class="mt-1 text-xs font-medium text-muted-foreground">Typing</p>
              <p class="text-sm font-semibold text-foreground">
                {{ ((profile as any)?.typingPattern?.speedWpm ?? 0).toFixed(0) }} WPM
              </p>
            </div>
            <div class="rounded-lg shadow-sm p-3 text-center">
              <svg class="mx-auto h-5 w-5 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M15.042 21.672L13.684 16.6m0 0l-2.51 2.225.569-9.47 5.227 7.917-3.286-.672zM12 2.25V4.5m5.834.166l-1.591 1.591M20.25 10.5H18M7.757 14.743l-1.59 1.59M6 10.5H3.75m4.007-4.243l-1.59-1.59" />
              </svg>
              <p class="mt-1 text-xs font-medium text-muted-foreground">Mouse</p>
              <p class="text-sm font-semibold text-foreground">
                {{ ((profile as any)?.mousePattern?.avgVelocity ?? 0).toFixed(1) }} px/ms
              </p>
            </div>
            <div class="rounded-lg shadow-sm p-3 text-center">
              <svg class="mx-auto h-5 w-5 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09zM18.259 8.715L18 9.75l-.259-1.035a3.375 3.375 0 00-2.455-2.456L14.25 6l1.036-.259a3.375 3.375 0 002.455-2.456L18 2.25l.259 1.035a3.375 3.375 0 002.455 2.456L21.75 6l-1.036.259a3.375 3.375 0 00-2.455 2.456z" />
              </svg>
              <p class="mt-1 text-xs font-medium text-muted-foreground">AI Models</p>
              <p class="text-sm font-semibold text-foreground">
                {{ [aiStatus?.keystrokeAE?.trained, aiStatus?.mouseCNN?.trained, aiStatus?.faceEmbedder?.enrolled].filter(Boolean).length }}/3 Ready
              </p>
            </div>
          </div>
        </div>
      </template>

      <!-- ================================================================ -->
      <!-- SESSIONS TAB                                                     -->
      <!-- ================================================================ -->
      <template v-else-if="activeTab === 'sessions'">
        <div v-if="sessions.length === 0">
          <EmptyState
            title="No sessions recorded"
            description="Integrity sessions are created automatically when you take assessments. Complete an assessment to see session data here."
          >
            <template #action>
              <div class="flex flex-col items-center gap-2">
                <svg class="h-10 w-10 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
                </svg>
                <p class="text-xs text-muted-foreground">
                  Sentinel monitors integrity signals in real-time during assessments.
                </p>
              </div>
            </template>
          </EmptyState>
        </div>

        <div v-else class="space-y-3">
          <div
            v-for="session in sessions.slice(0, 30)"
            :key="session.id"
            class="rounded-lg shadow-sm p-4 transition-shadow hover:shadow-md"
          >
            <div class="flex items-start justify-between gap-4">
              <!-- Left: session info -->
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-2">
                  <p class="truncate font-mono text-sm font-medium text-foreground">
                    {{ session.enrollment_id }}
                  </p>
                  <AppBadge
                    :variant="session.status === 'active' ? 'primary' : session.status === 'completed' ? 'success' : 'secondary'"
                  >
                    {{ session.status }}
                  </AppBadge>
                  <AppBadge :variant="outcomeBadgeVariant(getOutcome(session))">
                    {{ getOutcome(session) }}
                  </AppBadge>
                </div>
                <div class="mt-1.5 flex items-center gap-3 text-xs text-muted-foreground">
                  <span class="flex items-center gap-1">
                    <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
                    </svg>
                    {{ formatDate(session.started_at) }}
                  </span>
                  <span v-if="session.ended_at" class="flex items-center gap-1">
                    <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M5.25 7.5A2.25 2.25 0 017.5 5.25h9a2.25 2.25 0 012.25 2.25v9a2.25 2.25 0 01-2.25 2.25h-9a2.25 2.25 0 01-2.25-2.25v-9z" />
                    </svg>
                    {{ formatDate(session.ended_at) }}
                  </span>
                </div>
              </div>

              <!-- Right: integrity score -->
              <div v-if="session.integrity_score !== null && session.integrity_score !== undefined" class="flex-shrink-0 text-right">
                <p class="font-mono text-2xl font-bold text-foreground">
                  {{ (session.integrity_score * 100).toFixed(0) }}<span class="text-sm font-normal text-muted-foreground">%</span>
                </p>
                <AppBadge :variant="scoreColor(session.integrity_score)" class="mt-1">
                  {{ session.integrity_score >= 0.8 ? 'High' : session.integrity_score >= 0.5 ? 'Medium' : 'Low' }}
                </AppBadge>
              </div>
              <div v-else class="flex-shrink-0 text-right">
                <p class="font-mono text-lg text-muted-foreground">--</p>
                <p class="text-[0.65rem] text-muted-foreground">In progress</p>
              </div>
            </div>
          </div>
        </div>
      </template>

      <!-- ================================================================ -->
      <!-- SIGNALS & WEIGHTS TAB                                            -->
      <!-- ================================================================ -->
      <template v-else-if="activeTab === 'signals'">
        <div class="card p-5">
          <h2 class="mb-1 text-sm font-semibold text-foreground">Integrity Signal Weights</h2>
          <p class="mb-5 text-xs text-muted-foreground">
            The composite integrity score is a weighted average of these 7 behavioral signals.
            All signals are computed entirely on-device.
          </p>

          <div class="space-y-3">
            <div
              v-for="signal in signalWeights"
              :key="signal.key"
              class="rounded-lg shadow-sm p-4"
            >
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <p class="text-sm font-medium text-foreground">{{ signal.name }}</p>
                    <span class="rounded bg-muted px-1.5 py-0.5 font-mono text-xs font-bold text-foreground">
                      {{ signal.weight }}%
                    </span>
                  </div>
                  <p class="mt-1 text-xs text-muted-foreground">{{ signal.description }}</p>
                </div>
              </div>

              <!-- Weight bar -->
              <div class="mt-3 h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  class="h-full rounded-full bg-primary transition-all duration-500"
                  :style="{ width: signal.weight + '%' }"
                />
              </div>
            </div>
          </div>
        </div>

        <!-- Weight distribution visual -->
        <div class="card p-5">
          <h2 class="mb-3 text-sm font-semibold text-foreground">Weight Distribution</h2>
          <div class="flex h-4 overflow-hidden rounded-full">
            <div
              v-for="(signal, idx) in signalWeights"
              :key="signal.key"
              class="transition-all duration-500"
              :class="[
                idx === 0 ? 'bg-violet-500' : '',
                idx === 1 ? 'bg-blue-500' : '',
                idx === 2 ? 'bg-cyan-500' : '',
                idx === 3 ? 'bg-emerald-500' : '',
                idx === 4 ? 'bg-amber-500' : '',
                idx === 5 ? 'bg-orange-500' : '',
                idx === 6 ? 'bg-rose-500' : '',
              ]"
              :style="{ width: signal.weight + '%' }"
              :title="signal.name + ': ' + signal.weight + '%'"
            />
          </div>
          <div class="mt-3 flex flex-wrap gap-x-4 gap-y-1.5 text-xs">
            <div v-for="(signal, idx) in signalWeights" :key="signal.key" class="flex items-center gap-1.5">
              <span
                class="inline-block h-2 w-2 rounded-full"
                :class="[
                  idx === 0 ? 'bg-violet-500' : '',
                  idx === 1 ? 'bg-blue-500' : '',
                  idx === 2 ? 'bg-cyan-500' : '',
                  idx === 3 ? 'bg-emerald-500' : '',
                  idx === 4 ? 'bg-amber-500' : '',
                  idx === 5 ? 'bg-orange-500' : '',
                  idx === 6 ? 'bg-rose-500' : '',
                ]"
              />
              <span class="text-muted-foreground">{{ signal.name }}</span>
              <span class="font-medium text-foreground">{{ signal.weight }}%</span>
            </div>
          </div>
        </div>
      </template>

      <!-- ================================================================ -->
      <!-- PROFILE & FLAGS TAB                                              -->
      <!-- ================================================================ -->
      <template v-else-if="activeTab === 'profile'">
        <!-- Behavioral Profile -->
        <div class="card p-5">
          <div class="mb-4 flex items-center justify-between">
            <h2 class="text-sm font-semibold text-foreground">Behavioral Profile</h2>
            <AppBadge :variant="profile ? 'success' : 'warning'">
              {{ profile ? 'Calibrated' : 'Not Calibrated' }}
            </AppBadge>
          </div>

          <div v-if="profile" class="grid grid-cols-1 gap-4 sm:grid-cols-3">
            <!-- Typing -->
            <div class="rounded-lg shadow-sm p-4">
              <p class="mb-3 text-xs font-medium uppercase tracking-wider text-muted-foreground">Typing</p>
              <div class="space-y-2">
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">Avg Dwell</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ ((profile as any)?.typingPattern?.avgDwellTime ?? 0).toFixed(0) }}ms
                  </span>
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">Avg Flight</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ ((profile as any)?.typingPattern?.avgFlightTime ?? (profile as any)?.typingPattern?.avgFlightMs ?? 0).toFixed(0) }}ms
                  </span>
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">Speed</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ ((profile as any)?.typingPattern?.speedWpm ?? 0).toFixed(0) }} WPM
                  </span>
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">Samples</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ (profile as any)?.typingPattern?.sampleCount ?? 0 }}
                  </span>
                </div>
              </div>
            </div>

            <!-- Mouse -->
            <div class="rounded-lg shadow-sm p-4">
              <p class="mb-3 text-xs font-medium uppercase tracking-wider text-muted-foreground">Mouse</p>
              <div class="space-y-2">
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">Velocity</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ ((profile as any)?.mousePattern?.avgVelocity ?? 0).toFixed(2) }} px/ms
                  </span>
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">Acceleration</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ ((profile as any)?.mousePattern?.avgAcceleration ?? 0).toFixed(2) }} px/ms²
                  </span>
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">Click Precision</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ ((profile as any)?.mousePattern?.clickPrecision ?? 0).toFixed(2) }}
                  </span>
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">Samples</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ (profile as any)?.mousePattern?.sampleCount ?? 0 }}
                  </span>
                </div>
              </div>
            </div>

            <!-- AI Models -->
            <div class="rounded-lg shadow-sm p-4">
              <p class="mb-3 text-xs font-medium uppercase tracking-wider text-muted-foreground">AI Models</p>
              <div v-if="aiStatus" class="space-y-2.5">
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">Keystroke AE</span>
                  <AppBadge :variant="aiStatus.keystrokeAE?.trained ? 'success' : 'warning'" class="text-[0.6rem]">
                    {{ aiStatus.keystrokeAE?.trained ? 'Ready' : 'Pending' }}
                  </AppBadge>
                </div>
                <div v-if="aiStatus.keystrokeAE?.trained" class="flex items-center justify-between">
                  <span class="pl-2 text-[0.65rem] text-muted-foreground">Loss / Samples</span>
                  <span class="font-mono text-[0.65rem] text-muted-foreground">
                    {{ aiStatus.keystrokeAE.loss.toFixed(4) }} / {{ aiStatus.keystrokeAE.samples }}
                  </span>
                </div>

                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">Mouse CNN</span>
                  <AppBadge :variant="aiStatus.mouseCNN?.trained ? 'success' : 'warning'" class="text-[0.6rem]">
                    {{ aiStatus.mouseCNN?.trained ? 'Ready' : 'Pending' }}
                  </AppBadge>
                </div>
                <div v-if="aiStatus.mouseCNN?.trained" class="flex items-center justify-between">
                  <span class="pl-2 text-[0.65rem] text-muted-foreground">Loss / Samples</span>
                  <span class="font-mono text-[0.65rem] text-muted-foreground">
                    {{ aiStatus.mouseCNN.loss.toFixed(4) }} / {{ aiStatus.mouseCNN.samples }}
                  </span>
                </div>

                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">Face LBP</span>
                  <AppBadge :variant="aiStatus.faceEmbedder?.enrolled ? 'success' : 'secondary'" class="text-[0.6rem]">
                    {{ aiStatus.faceEmbedder?.enrolled ? 'Enrolled' : 'Not set' }}
                  </AppBadge>
                </div>
                <div v-if="aiStatus.faceEmbedder?.enrolled" class="flex items-center justify-between">
                  <span class="pl-2 text-[0.65rem] text-muted-foreground">Enrollment Progress</span>
                  <span class="font-mono text-[0.65rem] text-muted-foreground">
                    {{ Math.round(aiStatus.faceEmbedder.progress * 100) }}%
                  </span>
                </div>
              </div>
              <div v-else class="py-2 text-center text-xs text-muted-foreground">
                No AI model data
              </div>
            </div>
          </div>

          <div v-else class="rounded-lg border border-dashed border-border p-6 text-center">
            <p class="text-sm font-medium text-foreground">No profile yet</p>
            <p class="mt-1 text-xs text-muted-foreground">
              Run the training wizard to calibrate Sentinel to your behavioral patterns.
            </p>
            <AppButton variant="primary" size="sm" class="mt-3" @click="showWizard = true">
              Train Sentinel
            </AppButton>
          </div>
        </div>

        <!-- Anomaly Flag Types -->
        <div class="card p-5">
          <h2 class="mb-1 text-sm font-semibold text-foreground">Anomaly Flag Types</h2>
          <p class="mb-5 text-xs text-muted-foreground">
            Flags are raised automatically when behavioral signals exceed thresholds.
            Session outcomes are determined by flag count and severity.
          </p>

          <div class="space-y-2">
            <div
              v-for="flag in anomalyFlagTypes"
              :key="flag.type"
              class="flex items-start gap-4 rounded-lg shadow-sm p-4"
            >
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-2">
                  <code class="rounded bg-muted px-1.5 py-0.5 text-xs font-medium text-foreground">
                    {{ flag.type }}
                  </code>
                  <AppBadge :variant="severityBadgeVariant(flag.severity)">
                    {{ flag.severity }}
                  </AppBadge>
                </div>
                <p class="mt-1.5 text-xs text-foreground">{{ flag.description }}</p>
                <p class="mt-0.5 text-[0.65rem] text-muted-foreground">
                  Trigger: {{ flag.trigger }}
                </p>
              </div>
            </div>
          </div>
        </div>

        <!-- Outcome Rules -->
        <div class="card p-5">
          <h2 class="mb-3 text-sm font-semibold text-foreground">Outcome Determination</h2>
          <div class="space-y-2">
            <div class="flex items-start gap-3 rounded-lg shadow-sm p-3">
              <span class="mt-0.5 inline-block h-2.5 w-2.5 flex-shrink-0 rounded-full bg-emerald-500" />
              <div>
                <p class="text-xs font-medium text-foreground">Clean</p>
                <p class="text-[0.65rem] text-muted-foreground">Default outcome — no critical flags, fewer than 3 warnings, integrity &ge; 0.40</p>
              </div>
            </div>
            <div class="flex items-start gap-3 rounded-lg shadow-sm p-3">
              <span class="mt-0.5 inline-block h-2.5 w-2.5 flex-shrink-0 rounded-full bg-amber-500" />
              <div>
                <p class="text-xs font-medium text-foreground">Flagged</p>
                <p class="text-[0.65rem] text-muted-foreground">1 critical flag, OR 3+ warnings, OR integrity &lt; 0.40 — surfaces for admin review</p>
              </div>
            </div>
            <div class="flex items-start gap-3 rounded-lg shadow-sm p-3">
              <span class="mt-0.5 inline-block h-2.5 w-2.5 flex-shrink-0 rounded-full bg-red-500" />
              <div>
                <p class="text-xs font-medium text-foreground">Suspended</p>
                <p class="text-[0.65rem] text-muted-foreground">2+ critical flags, OR 1 critical + 2 warnings — assessment results may be invalidated</p>
              </div>
            </div>
          </div>
        </div>

        <!-- Reset -->
        <div v-if="profile" class="card p-5">
          <div class="flex items-center justify-between">
            <div>
              <h2 class="text-sm font-semibold text-foreground">Reset Behavioral Profile</h2>
              <p class="mt-1 text-xs text-muted-foreground">
                Delete your stored behavioral profile and AI model weights from this device.
                You will need to recalibrate before Sentinel can monitor future assessments.
              </p>
            </div>
            <AppButton
              variant="danger"
              size="sm"
              @click="handleResetProfile"
            >
              Reset Profile
            </AppButton>
          </div>
        </div>
      </template>

      <!-- ================================================================ -->
      <!-- PRIVACY NOTICE (bottom)                                          -->
      <!-- ================================================================ -->
      <div class="rounded-lg shadow-sm p-4">
        <div class="flex gap-3">
          <svg class="h-5 w-5 flex-shrink-0 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M11.25 11.25l.041-.02a.75.75 0 011.063.852l-.708 2.836a.75.75 0 001.063.853l.041-.021M21 12a9 9 0 11-18 0 9 9 0 0118 0zm-9-3.75h.008v.008H12V8.25z" />
          </svg>
          <div>
            <p class="text-sm font-medium text-foreground">How Sentinel Works</p>
            <p class="mt-1 text-xs text-muted-foreground">
              Sentinel uses dual scoring: rule-based deterministic checks (authoritative) and AI-based per-user models (advisory).
              During assessments, behavioral snapshots are captured at random 15–45 second intervals.
              Integrity and consistency scores are computed from 7 weighted signals.
              Session outcomes are determined by flag severity and count.
              Confirmed violations lower the trust factor on skill assessments by 0.20 per violation (floor: 0.10).
            </p>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
