<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { AppButton, AppBadge, EmptyState } from '@/components/ui'
import { useSentinel } from '@/composables/useSentinel'
import { useLocalApi } from '@/composables/useLocalApi'
import { getLoadedClassifierInfo } from '@/composables/useSentinel'
import SentinelTrainingWizard from '@/components/integrity/SentinelTrainingWizard.vue'
import type {
  IntegritySession,
  SentinelDaoInfo,
  SentinelHoldoutRef,
  ActivePasteClassifier,
} from '@/types'

const router = useRouter()
const { t } = useI18n()

const { invoke } = useLocalApi()
const {
  getProfile,
  getAIModelStatus,
  resetProfile,
  aiScoringEnabled,
  setAIScoringEnabled,
  pasteClassifierEnabled,
  setPasteClassifierEnabled,
} = useSentinel()

const loadedClassifier = ref(getLoadedClassifierInfo())
const activeDaoClassifier = ref<ActivePasteClassifier | null>(null)

const showWizard = ref(false)
const sessions = ref<IntegritySession[]>([])
const loading = ref(true)
const profile = ref<Record<string, unknown> | null>(null)
const sentinelDao = ref<SentinelDaoInfo | null>(null)
const holdouts = ref<SentinelHoldoutRef[]>([])
const aiStatus = ref<{
  keystrokeAE: { trained: boolean; epochs: number; samples: number; loss: number } | null
  mouseCNN: { trained: boolean; epochs: number; samples: number; loss: number } | null
  faceEmbedder: { enrolled: boolean; progress: number } | null
} | null>(null)

const activeTab = ref<'overview' | 'sessions' | 'signals' | 'profile'>('overview')

// ---------------------------------------------------------------------------
// Signal weights (matches sentinel spec)
// ---------------------------------------------------------------------------
const signalWeights = computed(() => [
  { name: t('sentinel.signals.items.typingConsistency.name'), key: 'typing_consistency', weight: 20, description: t('sentinel.signals.items.typingConsistency.description') },
  { name: t('sentinel.signals.items.mouseConsistency.name'), key: 'mouse_consistency', weight: 15, description: t('sentinel.signals.items.mouseConsistency.description') },
  { name: t('sentinel.signals.items.isHuman.name'), key: 'is_human_likely', weight: 15, description: t('sentinel.signals.items.isHuman.description') },
  { name: t('sentinel.signals.items.tabSwitches.name'), key: 'tab_switches', weight: 15, description: t('sentinel.signals.items.tabSwitches.description') },
  { name: t('sentinel.signals.items.pasteEvents.name'), key: 'paste_events', weight: 10, description: t('sentinel.signals.items.pasteEvents.description') },
  { name: t('sentinel.signals.items.devtools.name'), key: 'devtools_detected', weight: 10, description: t('sentinel.signals.items.devtools.description') },
  { name: t('sentinel.signals.items.facePresent.name'), key: 'face_present', weight: 15, description: t('sentinel.signals.items.facePresent.description') },
  { name: t('sentinel.signals.items.aiPasteAnomaly.name'), key: 'ai_paste_anomaly', weight: 5, description: t('sentinel.signals.items.aiPasteAnomaly.description') },
])

// ---------------------------------------------------------------------------
// Anomaly flag types
// ---------------------------------------------------------------------------
const anomalyFlagTypes = computed(() => [
  { type: 'tab_switching', severity: 'warning' as const, description: t('sentinel.flags.items.tabSwitching.description'), trigger: t('sentinel.flags.items.tabSwitching.trigger') },
  { type: 'paste_detected', severity: 'warning' as const, description: t('sentinel.flags.items.pasteDetected.description'), trigger: t('sentinel.flags.items.pasteDetected.trigger') },
  { type: 'devtools_detected', severity: 'critical' as const, description: t('sentinel.flags.items.devtoolsDetected.description'), trigger: t('sentinel.flags.items.devtoolsDetected.trigger') },
  { type: 'bot_suspected', severity: 'critical' as const, description: t('sentinel.flags.items.botSuspected.description'), trigger: t('sentinel.flags.items.botSuspected.trigger') },
  { type: 'no_face', severity: 'info' as const, description: t('sentinel.flags.items.noFace.description'), trigger: t('sentinel.flags.items.noFace.trigger') },
  { type: 'multiple_faces', severity: 'warning' as const, description: t('sentinel.flags.items.multipleFaces.description'), trigger: t('sentinel.flags.items.multipleFaces.trigger') },
  { type: 'multi_account', severity: 'critical' as const, description: t('sentinel.flags.items.multiAccount.description'), trigger: t('sentinel.flags.items.multiAccount.trigger') },
  { type: 'low_integrity', severity: 'warning' as const, description: t('sentinel.flags.items.lowIntegrity.description'), trigger: t('sentinel.flags.items.lowIntegrity.trigger') },
  { type: 'behavior_shift', severity: 'warning' as const, description: t('sentinel.flags.items.behaviorShift.description'), trigger: t('sentinel.flags.items.behaviorShift.trigger') },
])

// ---------------------------------------------------------------------------
// Computed
// ---------------------------------------------------------------------------

function getOutcome(session: IntegritySession): 'clean' | 'flagged' | 'suspended' {
  // Server-authoritative: backend computes status from cumulative anomaly
  // severity + running integrity score. Clean == active|completed.
  if (session.status === 'suspended') return 'suspended'
  if (session.status === 'flagged') return 'flagged'
  return 'clean'
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
    // Sentinel DAO + holdout data — both may be absent on fresh
    // installs; failures are logged but don't block the page.
    try {
      sentinelDao.value = await invoke<SentinelDaoInfo>('sentinel_dao_get_info')
    } catch (e) {
      console.warn('Sentinel DAO info unavailable:', e)
      sentinelDao.value = null
    }
    try {
      holdouts.value = await invoke<SentinelHoldoutRef[]>('sentinel_holdout_list')
    } catch {
      holdouts.value = []
    }
    try {
      activeDaoClassifier.value = await invoke<ActivePasteClassifier | null>(
        'sentinel_get_active_paste_classifier',
      )
    } catch (e) {
      console.warn('Active paste classifier unavailable:', e)
      activeDaoClassifier.value = null
    }
    loadedClassifier.value = getLoadedClassifierInfo()
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

// Copy a session id to the clipboard (used to feed the Sponsor issue flow).
const copiedSessionId = ref<string | null>(null)
async function copySessionId(id: string) {
  try {
    await navigator.clipboard.writeText(id)
    copiedSessionId.value = id
    setTimeout(() => { if (copiedSessionId.value === id) copiedSessionId.value = null }, 1500)
  } catch { /* clipboard unavailable */ }
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
          <h1 class="text-xl font-bold text-foreground">{{ $t('sentinel.title') }}</h1>
          <p class="mt-0.5 text-sm text-muted-foreground">
            {{ $t('sentinel.subtitle') }}
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
          {{ $t('sentinel.actions.resetProfile') }}
        </AppButton>
        <AppButton
          variant="secondary"
          size="sm"
          @click="router.push('/dashboard/sentinel/propose-prior')"
        >
          {{ $t('sentinel.actions.proposeCheat') }}
        </AppButton>
        <AppButton
          variant="primary"
          size="sm"
          @click="showWizard = true"
        >
          {{ profile ? $t('sentinel.actions.recalibrate') : $t('sentinel.actions.train') }}
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
            <p class="text-sm font-medium text-emerald-800 dark:text-emerald-300">{{ $t('sentinel.privacy.title') }}</p>
            <p class="mt-1 text-xs text-emerald-700 dark:text-emerald-400">
              {{ $t('sentinel.privacy.body') }}
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
            {{ tab === 'overview' ? $t('sentinel.tabs.overview') : tab === 'sessions' ? $t('sentinel.tabs.sessions') : tab === 'signals' ? $t('sentinel.tabs.signals') : $t('sentinel.tabs.profile') }}
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
            <p class="text-xs font-medium text-muted-foreground">{{ $t('sentinel.overview.totalSessions') }}</p>
            <p class="mt-1 text-2xl font-bold text-foreground">{{ sessions.length }}</p>
          </div>
          <!-- Avg Integrity -->
          <div class="rounded-lg shadow-sm p-4">
            <p class="text-xs font-medium text-muted-foreground">{{ $t('sentinel.overview.avgIntegrity') }}</p>
            <p class="mt-1 text-2xl font-bold text-foreground">{{ integrityPercent }}%</p>
          </div>
          <!-- Avg Consistency -->
          <div class="rounded-lg shadow-sm p-4">
            <p class="text-xs font-medium text-muted-foreground">{{ $t('sentinel.overview.avgConsistency') }}</p>
            <p class="mt-1 text-2xl font-bold text-foreground">{{ consistencyPercent }}%</p>
          </div>
          <!-- Clean Rate -->
          <div class="rounded-lg shadow-sm p-4">
            <p class="text-xs font-medium text-muted-foreground">{{ $t('sentinel.overview.cleanRate') }}</p>
            <p class="mt-1 text-2xl font-bold text-foreground">{{ sessionBreakdown.cleanPct }}%</p>
          </div>
        </div>

        <!-- Session Outcome Breakdown -->
        <div class="card p-5">
          <h2 class="mb-4 text-sm font-semibold text-foreground">{{ $t('sentinel.breakdown.title') }}</h2>

          <div v-if="sessions.length === 0" class="py-6 text-center text-sm text-muted-foreground">
            {{ $t('sentinel.breakdown.empty') }}
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
                <span class="text-muted-foreground">{{ $t('sentinel.breakdown.clean') }}</span>
                <span class="font-medium text-foreground">{{ sessionBreakdown.clean }}</span>
              </div>
              <div class="flex items-center gap-1.5">
                <span class="inline-block h-2.5 w-2.5 rounded-full bg-amber-500" />
                <span class="text-muted-foreground">{{ $t('sentinel.breakdown.flagged') }}</span>
                <span class="font-medium text-foreground">{{ sessionBreakdown.flagged }}</span>
              </div>
              <div class="flex items-center gap-1.5">
                <span class="inline-block h-2.5 w-2.5 rounded-full bg-red-500" />
                <span class="text-muted-foreground">{{ $t('sentinel.breakdown.suspended') }}</span>
                <span class="font-medium text-foreground">{{ sessionBreakdown.suspended }}</span>
              </div>
            </div>
          </template>
        </div>

        <!-- Sentinel DAO status + holdout summary (follow-ups #1 and #3) -->
        <div v-if="sentinelDao" class="grid grid-cols-1 gap-4 md:grid-cols-2">
          <!-- DAO committee card -->
          <div class="card p-5">
            <div class="mb-2 flex items-center justify-between">
              <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.community.title') }}</h2>
              <AppBadge :variant="sentinelDao.committee.length > 0 ? 'success' : 'warning'">
                {{ sentinelDao.committee.length > 0 ? $t('sentinel.community.active') : $t('sentinel.community.pending') }}
              </AppBadge>
            </div>
            <p class="text-xs text-muted-foreground">
              {{ $t('sentinel.community.description') }}
            </p>
            <div v-if="sentinelDao.committee.length === 0" class="mt-3 rounded-md bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-400">
              {{ $t('sentinel.community.noCouncil') }}
              <div class="mt-2">
                <AppButton size="sm" variant="secondary" @click="router.push('/community')">
                  {{ $t('sentinel.actions.openCommunity') }}
                </AppButton>
              </div>
            </div>
            <details v-else class="mt-3">
              <summary class="cursor-pointer text-xs text-muted-foreground">{{ $t('common.advanced.toggle') }}</summary>
              <p class="mt-2 text-[0.65rem] text-muted-foreground">{{ $t('sentinel.community.members') }}</p>
              <div class="mt-1 space-y-1">
                <div
                  v-for="m in sentinelDao.committee"
                  :key="m.stake_address"
                  class="flex items-center justify-between text-xs"
                >
                  <code class="truncate font-mono text-muted-foreground">{{ m.stake_address }}</code>
                  <AppBadge :variant="m.role === 'chair' ? 'primary' : 'secondary'">
                    {{ m.role }}
                  </AppBadge>
                </div>
              </div>
            </details>
          </div>

          <!-- Holdout summary card -->
          <div class="card p-5">
            <div class="mb-2 flex items-center justify-between">
              <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.holdout.title') }}</h2>
              <AppBadge :variant="holdouts.length > 0 ? 'success' : 'secondary'">
                {{ $t('sentinel.holdout.sets', { count: holdouts.length }, holdouts.length) }}
              </AppBadge>
            </div>
            <p class="text-xs text-muted-foreground">
              {{ $t('sentinel.holdout.description', { threshold: holdouts[0]?.threshold ?? 'N' }) }}
            </p>
            <div v-if="holdouts.length === 0" class="mt-3 rounded-md bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
              <i18n-t keypath="sentinel.holdout.empty" tag="span">
                <template #command><code class="font-mono">sentinel_holdout_upload</code></template>
              </i18n-t>
            </div>
            <div v-else class="mt-3">
              <AppButton size="sm" variant="secondary" @click="router.push('/dashboard/sentinel/holdout-evaluate')">
                {{ $t('sentinel.holdout.run') }}
              </AppButton>
            </div>
          </div>
        </div>

        <!-- Cheat-test diagnostic -->
        <div class="card p-5">
          <div class="flex items-center justify-between">
            <div>
              <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.cheatCard.title') }}</h2>
              <p class="mt-1 text-xs text-muted-foreground">
                {{ $t('sentinel.cheatCard.description') }}
              </p>
            </div>
            <AppButton size="sm" variant="secondary" @click="router.push('/dashboard/sentinel/cheat-test')">
              {{ $t('sentinel.cheatCard.open') }}
            </AppButton>
          </div>
        </div>

        <!-- Engine Status -->
        <div class="card p-5">
          <div class="mb-4 flex items-center justify-between">
            <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.engine.title') }}</h2>
            <AppBadge :variant="profile ? 'success' : 'warning'">
              {{ profile ? $t('sentinel.engine.calibrated') : $t('sentinel.engine.notCalibrated') }}
            </AppBadge>
          </div>

          <div v-if="!profile" class="rounded-lg border border-dashed border-border p-6 text-center">
            <svg class="mx-auto h-8 w-8 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
            </svg>
            <p class="mt-2 text-sm font-medium text-foreground">{{ $t('sentinel.engine.emptyTitle') }}</p>
            <p class="mt-1 text-xs text-muted-foreground">
              {{ $t('sentinel.engine.emptyBody') }}
            </p>
            <AppButton variant="primary" size="sm" class="mt-4" @click="showWizard = true">
              {{ $t('sentinel.actions.train') }}
            </AppButton>
          </div>

          <div v-else class="grid grid-cols-1 sm:grid-cols-3 gap-3">
            <div class="rounded-lg shadow-sm p-3 text-center">
              <svg class="mx-auto h-5 w-5 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M3.75 6.75h16.5M3.75 12h16.5M12 17.25h8.25" />
              </svg>
              <p class="mt-1 text-xs font-medium text-muted-foreground">{{ $t('sentinel.engine.typing') }}</p>
              <p class="text-sm font-semibold text-foreground">
                {{ ((profile as any)?.typingPattern?.speedWpm ?? 0).toFixed(0) }} {{ $t('sentinel.engine.wpm') }}
              </p>
            </div>
            <div class="rounded-lg shadow-sm p-3 text-center">
              <svg class="mx-auto h-5 w-5 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M15.042 21.672L13.684 16.6m0 0l-2.51 2.225.569-9.47 5.227 7.917-3.286-.672zM12 2.25V4.5m5.834.166l-1.591 1.591M20.25 10.5H18M7.757 14.743l-1.59 1.59M6 10.5H3.75m4.007-4.243l-1.59-1.59" />
              </svg>
              <p class="mt-1 text-xs font-medium text-muted-foreground">{{ $t('sentinel.engine.mouse') }}</p>
              <p class="text-sm font-semibold text-foreground">
                {{ ((profile as any)?.mousePattern?.avgVelocity ?? 0).toFixed(1) }} px/ms
              </p>
            </div>
            <div class="rounded-lg shadow-sm p-3 text-center">
              <svg class="mx-auto h-5 w-5 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09zM18.259 8.715L18 9.75l-.259-1.035a3.375 3.375 0 00-2.455-2.456L14.25 6l1.036-.259a3.375 3.375 0 002.455-2.456L18 2.25l.259 1.035a3.375 3.375 0 002.455 2.456L21.75 6l-1.036.259a3.375 3.375 0 00-2.455 2.456z" />
              </svg>
              <p class="mt-1 text-xs font-medium text-muted-foreground">{{ $t('sentinel.engine.aiModels') }}</p>
              <p class="text-sm font-semibold text-foreground">
                {{ $t('sentinel.engine.ready', { count: [aiStatus?.keystrokeAE?.trained, aiStatus?.mouseCNN?.trained, aiStatus?.faceEmbedder?.enrolled].filter(Boolean).length }) }}
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
            :title="$t('sentinel.sessions.emptyTitle')"
            :description="$t('sentinel.sessions.emptyBody')"
          >
            <template #action>
              <div class="flex flex-col items-center gap-2">
                <svg class="h-10 w-10 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
                </svg>
                <p class="text-xs text-muted-foreground">
                  {{ $t('sentinel.sessions.emptyHint') }}
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
                <!-- Session id — copy to feed the Sponsor issuance flow. -->
                <button
                  class="mt-1.5 flex max-w-full items-center gap-1 font-mono text-xs text-muted-foreground transition-colors hover:text-foreground"
                  :title="$t('sentinel.sessions.copyTitle')"
                  @click="copySessionId(session.id)"
                >
                  <svg class="h-3 w-3 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
                  </svg>
                  <span class="truncate">{{ copiedSessionId === session.id ? $t('common.actions.copied') : session.id }}</span>
                </button>
              </div>

              <!-- Right: integrity score -->
              <div v-if="session.integrity_score !== null && session.integrity_score !== undefined" class="flex-shrink-0 text-end">
                <p class="font-mono text-2xl font-bold text-foreground">
                  {{ (session.integrity_score * 100).toFixed(0) }}<span class="text-sm font-normal text-muted-foreground">%</span>
                </p>
                <AppBadge :variant="scoreColor(session.integrity_score)" class="mt-1">
                  {{ session.integrity_score >= 0.8 ? $t('sentinel.sessions.scoreHigh') : session.integrity_score >= 0.5 ? $t('sentinel.sessions.scoreMedium') : $t('sentinel.sessions.scoreLow') }}
                </AppBadge>
              </div>
              <div v-else class="flex-shrink-0 text-end">
                <p class="font-mono text-lg text-muted-foreground">--</p>
                <p class="text-[0.65rem] text-muted-foreground">{{ $t('sentinel.sessions.inProgress') }}</p>
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
          <h2 class="mb-1 text-sm font-semibold text-foreground">{{ $t('sentinel.signals.title') }}</h2>
          <p class="mb-5 text-xs text-muted-foreground">
            {{ $t('sentinel.signals.description') }}
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
          <h2 class="mb-3 text-sm font-semibold text-foreground">{{ $t('sentinel.signals.distribution') }}</h2>
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
        <!-- Per-model training status (backend candle) -->
        <div class="card p-5">
          <h2 class="mb-4 text-sm font-semibold text-foreground">{{ $t('sentinel.profile.trainingTitle') }}</h2>
          <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
            <div class="rounded bg-muted/40 p-4">
              <div class="flex items-center justify-between">
                <div class="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                  {{ $t('sentinel.profile.keystrokeModel') }}
                </div>
                <AppBadge :variant="aiStatus?.keystrokeAE?.trained ? 'success' : 'secondary'">
                  {{ aiStatus?.keystrokeAE?.trained ? $t('sentinel.profile.trained') : $t('sentinel.profile.untrained') }}
                </AppBadge>
              </div>
              <div class="mt-3 grid grid-cols-3 gap-2 text-xs">
                <div>
                  <div class="text-muted-foreground">{{ $t('sentinel.profile.epochs') }}</div>
                  <div class="font-mono text-foreground">{{ aiStatus?.keystrokeAE?.epochs ?? '—' }}</div>
                </div>
                <div>
                  <div class="text-muted-foreground">{{ $t('sentinel.profile.samples') }}</div>
                  <div class="font-mono text-foreground">{{ aiStatus?.keystrokeAE?.samples ?? '—' }}</div>
                </div>
                <div>
                  <div class="text-muted-foreground">{{ $t('sentinel.profile.loss') }}</div>
                  <div class="font-mono text-foreground">
                    {{ aiStatus?.keystrokeAE?.loss !== undefined ? aiStatus.keystrokeAE.loss.toFixed(3) : '—' }}
                  </div>
                </div>
              </div>
            </div>
            <div class="rounded bg-muted/40 p-4">
              <div class="flex items-center justify-between">
                <div class="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                  {{ $t('sentinel.profile.mouseModel') }}
                </div>
                <AppBadge :variant="aiStatus?.mouseCNN?.trained ? 'success' : 'secondary'">
                  {{ aiStatus?.mouseCNN?.trained ? $t('sentinel.profile.trained') : $t('sentinel.profile.untrained') }}
                </AppBadge>
              </div>
              <div class="mt-3 grid grid-cols-3 gap-2 text-xs">
                <div>
                  <div class="text-muted-foreground">{{ $t('sentinel.profile.epochs') }}</div>
                  <div class="font-mono text-foreground">{{ aiStatus?.mouseCNN?.epochs ?? '—' }}</div>
                </div>
                <div>
                  <div class="text-muted-foreground">{{ $t('sentinel.profile.samples') }}</div>
                  <div class="font-mono text-foreground">{{ aiStatus?.mouseCNN?.samples ?? '—' }}</div>
                </div>
                <div>
                  <div class="text-muted-foreground">{{ $t('sentinel.profile.loss') }}</div>
                  <div class="font-mono text-foreground">
                    {{ aiStatus?.mouseCNN?.loss !== undefined ? aiStatus.mouseCNN.loss.toFixed(3) : '—' }}
                  </div>
                </div>
              </div>
            </div>
          </div>
          <p class="mt-3 text-xs text-muted-foreground">
            {{ $t('sentinel.profile.note') }}
          </p>
        </div>

        <!-- Behavioral Profile -->
        <div class="card p-5">
          <div class="mb-4 flex items-center justify-between">
            <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.profile.behavioralTitle') }}</h2>
            <AppBadge :variant="profile ? 'success' : 'warning'">
              {{ profile ? $t('sentinel.engine.calibrated') : $t('sentinel.engine.notCalibrated') }}
            </AppBadge>
          </div>

          <div v-if="profile" class="grid grid-cols-1 gap-4 sm:grid-cols-3">
            <!-- Typing -->
            <div class="rounded-lg shadow-sm p-4">
              <p class="mb-3 text-xs font-medium uppercase tracking-wider text-muted-foreground">{{ $t('sentinel.profile.typing') }}</p>
              <div class="space-y-2">
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">{{ $t('sentinel.profile.avgDwell') }}</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ ((profile as any)?.typingPattern?.avgDwellTime ?? 0).toFixed(0) }}ms
                  </span>
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">{{ $t('sentinel.profile.avgFlight') }}</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ ((profile as any)?.typingPattern?.avgFlightTime ?? (profile as any)?.typingPattern?.avgFlightMs ?? 0).toFixed(0) }}ms
                  </span>
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">{{ $t('sentinel.profile.speed') }}</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ ((profile as any)?.typingPattern?.speedWpm ?? 0).toFixed(0) }} {{ $t('sentinel.engine.wpm') }}
                  </span>
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">{{ $t('sentinel.profile.samples') }}</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ (profile as any)?.typingPattern?.sampleCount ?? 0 }}
                  </span>
                </div>
              </div>
            </div>

            <!-- Mouse -->
            <div class="rounded-lg shadow-sm p-4">
              <p class="mb-3 text-xs font-medium uppercase tracking-wider text-muted-foreground">{{ $t('sentinel.profile.mouse') }}</p>
              <div class="space-y-2">
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">{{ $t('sentinel.profile.velocity') }}</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ ((profile as any)?.mousePattern?.avgVelocity ?? 0).toFixed(2) }} px/ms
                  </span>
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">{{ $t('sentinel.profile.acceleration') }}</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ ((profile as any)?.mousePattern?.avgAcceleration ?? 0).toFixed(2) }} px/ms²
                  </span>
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">{{ $t('sentinel.profile.clickPrecision') }}</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ ((profile as any)?.mousePattern?.clickPrecision ?? 0).toFixed(2) }}
                  </span>
                </div>
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">{{ $t('sentinel.profile.samples') }}</span>
                  <span class="font-mono text-xs font-medium text-foreground">
                    {{ (profile as any)?.mousePattern?.sampleCount ?? 0 }}
                  </span>
                </div>
              </div>
            </div>

            <!-- AI Models -->
            <div class="rounded-lg shadow-sm p-4">
              <p class="mb-3 text-xs font-medium uppercase tracking-wider text-muted-foreground">{{ $t('sentinel.profile.aiModels') }}</p>
              <div v-if="aiStatus" class="space-y-2.5">
                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">{{ $t('sentinel.profile.keystrokeModelShort') }}</span>
                  <AppBadge :variant="aiStatus.keystrokeAE?.trained ? 'success' : 'warning'" class="text-[0.6rem]">
                    {{ aiStatus.keystrokeAE?.trained ? $t('sentinel.profile.ready') : $t('sentinel.profile.pending') }}
                  </AppBadge>
                </div>
                <div v-if="aiStatus.keystrokeAE?.trained" class="flex items-center justify-between">
                  <span class="ps-2 text-[0.65rem] text-muted-foreground">{{ $t('sentinel.profile.errorSamples') }}</span>
                  <span class="font-mono text-[0.65rem] text-muted-foreground">
                    {{ aiStatus.keystrokeAE.loss.toFixed(4) }} / {{ aiStatus.keystrokeAE.samples }}
                  </span>
                </div>

                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">{{ $t('sentinel.profile.mouseModelShort') }}</span>
                  <AppBadge :variant="aiStatus.mouseCNN?.trained ? 'success' : 'warning'" class="text-[0.6rem]">
                    {{ aiStatus.mouseCNN?.trained ? $t('sentinel.profile.ready') : $t('sentinel.profile.pending') }}
                  </AppBadge>
                </div>
                <div v-if="aiStatus.mouseCNN?.trained" class="flex items-center justify-between">
                  <span class="ps-2 text-[0.65rem] text-muted-foreground">{{ $t('sentinel.profile.errorSamples') }}</span>
                  <span class="font-mono text-[0.65rem] text-muted-foreground">
                    {{ aiStatus.mouseCNN.loss.toFixed(4) }} / {{ aiStatus.mouseCNN.samples }}
                  </span>
                </div>

                <div class="flex items-center justify-between">
                  <span class="text-xs text-muted-foreground">{{ $t('sentinel.profile.faceCheck') }}</span>
                  <AppBadge :variant="aiStatus.faceEmbedder?.enrolled ? 'success' : 'secondary'" class="text-[0.6rem]">
                    {{ aiStatus.faceEmbedder?.enrolled ? $t('sentinel.profile.enrolled') : $t('sentinel.profile.notSet') }}
                  </AppBadge>
                </div>
                <div v-if="aiStatus.faceEmbedder?.enrolled" class="flex items-center justify-between">
                  <span class="ps-2 text-[0.65rem] text-muted-foreground">{{ $t('sentinel.profile.setupProgress') }}</span>
                  <span class="font-mono text-[0.65rem] text-muted-foreground">
                    {{ Math.round(aiStatus.faceEmbedder.progress * 100) }}%
                  </span>
                </div>
              </div>
              <div v-else class="py-2 text-center text-xs text-muted-foreground">
                {{ $t('sentinel.profile.noAiData') }}
              </div>
            </div>
          </div>

          <div v-else class="rounded-lg border border-dashed border-border p-6 text-center">
            <p class="text-sm font-medium text-foreground">{{ $t('sentinel.profile.noProfileTitle') }}</p>
            <p class="mt-1 text-xs text-muted-foreground">
              {{ $t('sentinel.profile.noProfileBody') }}
            </p>
            <AppButton variant="primary" size="sm" class="mt-3" @click="showWizard = true">
              {{ $t('sentinel.actions.train') }}
            </AppButton>
          </div>
        </div>

        <!-- Anomaly Flag Types -->
        <div class="card p-5">
          <h2 class="mb-1 text-sm font-semibold text-foreground">{{ $t('sentinel.flags.title') }}</h2>
          <p class="mb-5 text-xs text-muted-foreground">
            {{ $t('sentinel.flags.description') }}
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
                  {{ $t('sentinel.flags.triggerLabel', { trigger: flag.trigger }) }}
                </p>
              </div>
            </div>
          </div>
        </div>

        <!-- Outcome Rules -->
        <div class="card p-5">
          <h2 class="mb-3 text-sm font-semibold text-foreground">{{ $t('sentinel.outcomes.title') }}</h2>
          <div class="space-y-2">
            <div class="flex items-start gap-3 rounded-lg shadow-sm p-3">
              <span class="mt-0.5 inline-block h-2.5 w-2.5 flex-shrink-0 rounded-full bg-emerald-500" />
              <div>
                <p class="text-xs font-medium text-foreground">{{ $t('sentinel.outcomes.clean') }}</p>
                <p class="text-[0.65rem] text-muted-foreground">{{ $t('sentinel.outcomes.cleanRule') }}</p>
              </div>
            </div>
            <div class="flex items-start gap-3 rounded-lg shadow-sm p-3">
              <span class="mt-0.5 inline-block h-2.5 w-2.5 flex-shrink-0 rounded-full bg-amber-500" />
              <div>
                <p class="text-xs font-medium text-foreground">{{ $t('sentinel.outcomes.flagged') }}</p>
                <p class="text-[0.65rem] text-muted-foreground">{{ $t('sentinel.outcomes.flaggedRule') }}</p>
              </div>
            </div>
            <div class="flex items-start gap-3 rounded-lg shadow-sm p-3">
              <span class="mt-0.5 inline-block h-2.5 w-2.5 flex-shrink-0 rounded-full bg-red-500" />
              <div>
                <p class="text-xs font-medium text-foreground">{{ $t('sentinel.outcomes.suspended') }}</p>
                <p class="text-[0.65rem] text-muted-foreground">{{ $t('sentinel.outcomes.suspendedRule') }}</p>
              </div>
            </div>
          </div>
        </div>

        <!-- AI scoring toggle -->
        <div class="card p-5">
          <div class="flex items-center justify-between gap-4">
            <div>
              <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.aiScoring.title') }}</h2>
              <p class="mt-1 text-xs text-muted-foreground">
                {{ $t('sentinel.aiScoring.description') }}
              </p>
            </div>
            <AppButton
              :variant="aiScoringEnabled ? 'primary' : 'secondary'"
              size="sm"
              @click="setAIScoringEnabled(!aiScoringEnabled)"
            >
              {{ aiScoringEnabled ? $t('sentinel.aiScoring.enabled') : $t('sentinel.aiScoring.disabled') }}
            </AppButton>
          </div>
        </div>

        <!-- Per-signal opt-out: paste classifier -->
        <div class="card p-5">
          <div class="flex items-center justify-between gap-4">
            <div>
              <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.pasteClassifier.title') }}</h2>
              <p class="mt-1 text-xs text-muted-foreground">
                {{ $t('sentinel.pasteClassifier.description') }}
              </p>
            </div>
            <AppButton
              :variant="pasteClassifierEnabled ? 'primary' : 'secondary'"
              size="sm"
              @click="setPasteClassifierEnabled(!pasteClassifierEnabled)"
            >
              {{ pasteClassifierEnabled ? $t('sentinel.aiScoring.enabled') : $t('sentinel.aiScoring.disabled') }}
            </AppButton>
          </div>
          <div class="mt-4 grid grid-cols-2 gap-3 text-xs">
            <div class="rounded bg-muted/40 p-2">
              <div class="text-muted-foreground">{{ $t('sentinel.pasteClassifier.loadedModel') }}</div>
              <div class="font-mono text-foreground">
                {{ loadedClassifier.version ?? '—' }}
                <span v-if="loadedClassifier.source" class="ms-1 text-muted-foreground">
                  ({{ loadedClassifier.source }})
                </span>
              </div>
            </div>
            <div class="rounded bg-muted/40 p-2">
              <div class="text-muted-foreground">{{ $t('sentinel.pasteClassifier.activeModel') }}</div>
              <div v-if="activeDaoClassifier" class="font-mono text-foreground">
                {{ activeDaoClassifier.version }}
                <span class="ms-1 text-muted-foreground">
                  TPR={{ activeDaoClassifier.eval_tpr.toFixed(2) }}
                  FPR={{ activeDaoClassifier.eval_fpr.toFixed(2) }}
                </span>
              </div>
              <div v-else class="font-mono text-muted-foreground">{{ $t('sentinel.pasteClassifier.bundledFallback') }}</div>
            </div>
          </div>
        </div>

        <!-- Reset -->
        <div v-if="profile" class="card p-5">
          <div class="flex items-center justify-between">
            <div>
              <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.reset.title') }}</h2>
              <p class="mt-1 text-xs text-muted-foreground">
                {{ $t('sentinel.reset.description') }}
              </p>
            </div>
            <AppButton
              variant="danger"
              size="sm"
              @click="handleResetProfile"
            >
              {{ $t('sentinel.reset.button') }}
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
            <p class="text-sm font-medium text-foreground">{{ $t('sentinel.howItWorks.title') }}</p>
            <p class="mt-1 text-xs text-muted-foreground">
              {{ $t('sentinel.howItWorks.body') }}
            </p>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
