<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { AppButton, AppBadge, EmptyState, DataRow } from '@/components/ui'
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


</script>

<template>
  <div class="space-y-6">
    <!-- Header -->
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-xl font-bold text-[rgb(var(--color-foreground))]">Sentinel</h1>
        <p class="mt-1 text-sm text-[rgb(var(--color-muted-foreground))]">
          Behavioral integrity engine. All processing stays on your device.
        </p>
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

    <!-- Training Wizard Modal -->
    <div v-if="showWizard" class="mx-auto max-w-2xl">
      <SentinelTrainingWizard
        @complete="onWizardComplete"
        @cancel="showWizard = false"
      />
    </div>

    <!-- Profile & AI Status -->
    <div v-if="!showWizard" class="space-y-6">
      <!-- Profile Card -->
      <div class="card p-5">
        <div class="mb-4 flex items-center justify-between">
          <h2 class="text-sm font-semibold text-[rgb(var(--color-foreground))]">Behavioral Profile</h2>
          <AppBadge :variant="profile ? 'success' : 'warning'">
            {{ profile ? 'Calibrated' : 'Not Calibrated' }}
          </AppBadge>
        </div>

        <div v-if="profile" class="grid grid-cols-1 gap-4 sm:grid-cols-3">
          <!-- Typing -->
          <div class="rounded-lg border border-[rgb(var(--color-border))] p-4">
            <p class="mb-2 text-xs font-medium text-[rgb(var(--color-muted-foreground))]">TYPING</p>
            <div class="space-y-1.5">
              <DataRow label="Avg Dwell" mono>
                {{ ((profile as any)?.typingPattern?.avgDwellTime ?? 0).toFixed(0) }}ms
              </DataRow>
              <DataRow label="Avg Flight" mono>
                {{ ((profile as any)?.typingPattern?.avgFlightMs ?? (profile as any)?.typingPattern?.avgFlightTime ?? 0).toFixed(0) }}ms
              </DataRow>
              <DataRow label="Speed" mono>
                {{ ((profile as any)?.typingPattern?.speedWpm ?? 0).toFixed(0) }} WPM
              </DataRow>
            </div>
          </div>

          <!-- Mouse -->
          <div class="rounded-lg border border-[rgb(var(--color-border))] p-4">
            <p class="mb-2 text-xs font-medium text-[rgb(var(--color-muted-foreground))]">MOUSE</p>
            <div class="space-y-1.5">
              <DataRow label="Velocity" mono>
                {{ ((profile as any)?.mousePattern?.avgVelocity ?? 0).toFixed(2) }} px/ms
              </DataRow>
              <DataRow label="Samples" mono>
                {{ (profile as any)?.mousePattern?.sampleCount ?? 0 }}
              </DataRow>
            </div>
          </div>

          <!-- AI Models -->
          <div class="rounded-lg border border-[rgb(var(--color-border))] p-4">
            <p class="mb-2 text-xs font-medium text-[rgb(var(--color-muted-foreground))]">AI MODELS</p>
            <div v-if="aiStatus" class="space-y-1.5">
              <div class="flex items-center justify-between text-xs">
                <span class="text-[rgb(var(--color-muted-foreground))]">Keystroke AE</span>
                <AppBadge :variant="aiStatus.keystrokeAE?.trained ? 'success' : 'warning'" class="text-[0.6rem]">
                  {{ aiStatus.keystrokeAE?.trained ? 'Ready' : 'Pending' }}
                </AppBadge>
              </div>
              <div class="flex items-center justify-between text-xs">
                <span class="text-[rgb(var(--color-muted-foreground))]">Mouse CNN</span>
                <AppBadge :variant="aiStatus.mouseCNN?.trained ? 'success' : 'warning'" class="text-[0.6rem]">
                  {{ aiStatus.mouseCNN?.trained ? 'Ready' : 'Pending' }}
                </AppBadge>
              </div>
              <div class="flex items-center justify-between text-xs">
                <span class="text-[rgb(var(--color-muted-foreground))]">Face LBP</span>
                <AppBadge :variant="aiStatus.faceEmbedder?.enrolled ? 'success' : 'secondary'" class="text-[0.6rem]">
                  {{ aiStatus.faceEmbedder?.enrolled ? 'Enrolled' : 'Not set' }}
                </AppBadge>
              </div>
            </div>
          </div>
        </div>

        <div v-else>
          <EmptyState
            title="No profile yet"
            description="Run the training wizard to calibrate Sentinel to your behavioral patterns."
          >
            <template #action>
              <AppButton variant="primary" size="sm" @click="showWizard = true">
                Train Sentinel
              </AppButton>
            </template>
          </EmptyState>
        </div>
      </div>

      <!-- Recent Sessions -->
      <div class="card p-5">
        <h2 class="mb-4 text-sm font-semibold text-[rgb(var(--color-foreground))]">Recent Sessions</h2>

        <div v-if="loading" class="flex items-center justify-center py-8">
          <div class="spinner" />
        </div>

        <div v-else-if="sessions.length === 0">
          <EmptyState
            title="No sessions yet"
            description="Integrity sessions are created automatically when you take assessments."
          />
        </div>

        <div v-else class="space-y-2">
          <div
            v-for="session in sessions.slice(0, 20)"
            :key="session.id"
            class="flex items-center justify-between rounded-lg border border-[rgb(var(--color-border))] px-4 py-3"
          >
            <div class="min-w-0">
              <div class="flex items-center gap-2">
                <p class="truncate text-sm font-medium text-[rgb(var(--color-foreground))]">
                  {{ session.enrollment_id }}
                </p>
                <AppBadge
                  :variant="session.status === 'active' ? 'primary' : session.status === 'completed' ? 'success' : 'secondary'"
                >
                  {{ session.status }}
                </AppBadge>
              </div>
              <p class="mt-0.5 text-xs text-[rgb(var(--color-muted-foreground))]">
                {{ formatDate(session.started_at) }}
                <span v-if="session.ended_at"> -- {{ formatDate(session.ended_at) }}</span>
              </p>
            </div>
            <div v-if="session.integrity_score !== null && session.integrity_score !== undefined" class="text-right">
              <p class="font-mono text-lg font-bold text-[rgb(var(--color-foreground))]">
                {{ (session.integrity_score * 100).toFixed(0) }}%
              </p>
              <AppBadge :variant="scoreColor(session.integrity_score)">
                {{ session.integrity_score >= 0.8 ? 'High' : session.integrity_score >= 0.5 ? 'Medium' : 'Low' }}
              </AppBadge>
            </div>
          </div>
        </div>
      </div>

      <!-- Privacy notice -->
      <div class="rounded-lg border border-emerald-200 bg-emerald-50 p-4 dark:border-emerald-800/40 dark:bg-emerald-900/20">
        <div class="flex gap-3">
          <svg class="h-5 w-5 flex-shrink-0 text-emerald-600 dark:text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
          </svg>
          <div>
            <p class="text-sm font-medium text-emerald-800 dark:text-emerald-300">Privacy by Design</p>
            <p class="mt-1 text-xs text-emerald-700 dark:text-emerald-400">
              Raw biometric data (keystrokes, mouse coordinates, video frames) never leaves your device.
              Only derived scores (0.0-1.0) are stored locally and used in evidence records.
              Your behavioral profile is stored in browser localStorage and can be deleted at any time.
            </p>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
