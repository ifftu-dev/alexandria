<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppBadge } from '@/components/ui'
import type { FullReputationAssertion, ReputationQuery } from '@/types'

const { invoke } = useLocalApi()

const assertions = ref<FullReputationAssertion[]>([])
const loading = ref(true)
const activeTab = ref<'instructor' | 'learner'>('instructor')

onMounted(async () => {
  try {
    const query: ReputationQuery = {}
    assertions.value = await invoke<FullReputationAssertion[]>('get_reputation', { query })
  } catch (e) {
    console.error('Failed to load reputation:', e)
  } finally {
    loading.value = false
  }
})

const instructorAssertions = computed(() => assertions.value.filter(a => a.role === 'instructor'))
const learnerAssertions = computed(() => assertions.value.filter(a => a.role === 'learner'))

const currentAssertions = computed(() => activeTab.value === 'instructor' ? instructorAssertions.value : learnerAssertions.value)

// Stats
const totalImpact = computed(() => {
  return instructorAssertions.value.reduce((sum, a) => sum + a.score, 0)
})
const avgConfidence = computed(() => {
  const list = currentAssertions.value
  if (list.length === 0) return 0
  return list.reduce((sum, a) => sum + a.confidence, 0) / list.length
})

function formatConfidence(n: number): string {
  return `${Math.round(n * 100)}%`
}
</script>

<template>
  <div>
    <!-- Header -->
    <div class="mb-8">
      <h1 class="text-3xl font-bold text-[rgb(var(--color-foreground))]">My Reputation</h1>
      <p class="mt-2 text-[rgb(var(--color-muted-foreground))]">
        Evidence-based, skill-scoped reputation derived from your teaching impact and learning achievements.
      </p>
    </div>

    <!-- Skeleton -->
    <div v-if="loading" class="space-y-6">
      <div class="grid gap-4 sm:grid-cols-3">
        <div v-for="i in 3" :key="i" class="animate-pulse rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
          <div class="h-3 w-24 rounded bg-[rgb(var(--color-muted-foreground)/0.15)] mb-3" />
          <div class="h-8 w-16 rounded bg-[rgb(var(--color-muted-foreground)/0.2)]" />
        </div>
      </div>
      <div class="space-y-3">
        <div v-for="i in 3" :key="i" class="animate-pulse rounded-lg border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-4">
          <div class="flex items-center justify-between mb-3">
            <div class="h-4 w-32 rounded bg-[rgb(var(--color-muted-foreground)/0.15)]" />
            <div class="h-6 w-12 rounded bg-[rgb(var(--color-muted-foreground)/0.2)]" />
          </div>
          <div class="h-1.5 w-full rounded-full bg-[rgb(var(--color-muted-foreground)/0.1)]" />
        </div>
      </div>
    </div>

    <template v-else>
      <!-- Tab Navigation -->
      <div class="mb-6 flex gap-1 rounded-lg bg-[rgb(var(--color-muted))] p-1">
        <button
          class="flex-1 rounded-md px-4 py-2 text-sm font-medium transition-colors"
          :class="activeTab === 'instructor'
            ? 'bg-[rgb(var(--color-card))] text-[rgb(var(--color-foreground))] shadow-sm'
            : 'text-[rgb(var(--color-muted-foreground))] hover:text-[rgb(var(--color-foreground))]'"
          @click="activeTab = 'instructor'"
        >
          Instructor Impact
        </button>
        <button
          class="flex-1 rounded-md px-4 py-2 text-sm font-medium transition-colors"
          :class="activeTab === 'learner'
            ? 'bg-[rgb(var(--color-card))] text-[rgb(var(--color-foreground))] shadow-sm'
            : 'text-[rgb(var(--color-muted-foreground))] hover:text-[rgb(var(--color-foreground))]'"
          @click="activeTab = 'learner'"
        >
          Learner Profile
        </button>
      </div>

      <!-- Stats -->
      <div class="mb-8 grid gap-4" :class="activeTab === 'instructor' ? 'sm:grid-cols-3' : 'sm:grid-cols-2'">
        <div v-if="activeTab === 'instructor'" class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
          <p class="text-sm text-[rgb(var(--color-muted-foreground))]">Total Impact</p>
          <p class="mt-2 text-3xl font-bold text-[rgb(var(--color-primary))]">{{ totalImpact.toFixed(2) }}</p>
        </div>
        <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
          <p class="text-sm text-[rgb(var(--color-muted-foreground))]">{{ activeTab === 'instructor' ? 'Skills Taught' : 'Skills Demonstrated' }}</p>
          <p class="mt-2 text-3xl font-bold text-[rgb(var(--color-foreground))]">{{ currentAssertions.length }}</p>
        </div>
        <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
          <p class="text-sm text-[rgb(var(--color-muted-foreground))]">Average Confidence</p>
          <p class="mt-2 text-3xl font-bold text-[rgb(var(--color-primary))]">{{ formatConfidence(avgConfidence) }}</p>
        </div>
      </div>

      <!-- Empty state -->
      <div v-if="currentAssertions.length === 0" class="py-16 text-center">
        <div class="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-[rgb(var(--color-muted)/0.3)]">
          <svg class="h-8 w-8 text-[rgb(var(--color-muted-foreground)/0.5)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M11.48 3.499a.562.562 0 011.04 0l2.125 5.111a.563.563 0 00.475.345l5.518.442c.499.04.701.663.321.988l-4.204 3.602a.563.563 0 00-.182.557l1.285 5.385a.562.562 0 01-.84.61l-4.725-2.885a.563.563 0 00-.586 0L6.982 20.54a.562.562 0 01-.84-.61l1.285-5.386a.562.562 0 00-.182-.557l-4.204-3.602a.563.563 0 01.321-.988l5.518-.442a.563.563 0 00.475-.345L11.48 3.5z" />
          </svg>
        </div>
        <h3 class="text-sm font-medium text-[rgb(var(--color-foreground))]">
          No {{ activeTab }} reputation yet
        </h3>
        <p class="mt-1 text-sm text-[rgb(var(--color-muted-foreground))] max-w-sm mx-auto">
          {{ activeTab === 'instructor'
            ? 'Publish courses and help learners earn skill proofs to build your impact.'
            : 'Complete courses and earn skill proofs to build your profile.' }}
        </p>
      </div>

      <!-- Assertions -->
      <div v-else class="space-y-3">
        <div
          v-for="assertion in currentAssertions"
          :key="assertion.id"
          class="rounded-lg border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-4 transition-colors hover:border-[rgb(var(--color-primary)/0.3)]"
        >
          <div class="flex items-center justify-between">
            <div>
              <p class="font-medium text-[rgb(var(--color-foreground))]">{{ assertion.skill_id ?? 'Global' }}</p>
              <div class="mt-1 flex items-center gap-2">
                <AppBadge variant="primary">{{ assertion.role }}</AppBadge>
                <AppBadge v-if="assertion.proficiency_level" variant="secondary">
                  {{ assertion.proficiency_level }}
                </AppBadge>
                <span v-if="assertion.evidence_count" class="text-xs text-[rgb(var(--color-muted-foreground))]">
                  {{ assertion.evidence_count }} evidence
                </span>
              </div>
            </div>
            <div class="text-right">
              <p class="text-lg font-bold text-[rgb(var(--color-primary))]">{{ assertion.score.toFixed(3) }}</p>
              <p class="text-xs text-[rgb(var(--color-muted-foreground))]">{{ formatConfidence(assertion.confidence) }} confidence</p>
            </div>
          </div>

          <!-- Confidence bar -->
          <div class="mt-3">
            <div class="flex items-center justify-between text-xs text-[rgb(var(--color-muted-foreground))]">
              <span>Confidence</span>
              <span>{{ formatConfidence(assertion.confidence) }}</span>
            </div>
            <div class="mt-1 h-1.5 overflow-hidden rounded-full bg-[rgb(var(--color-muted)/0.3)]">
              <div
                class="h-full rounded-full bg-[rgb(var(--color-accent,var(--color-primary)))] transition-all"
                :style="{ width: `${Math.round(assertion.confidence * 100)}%` }"
              />
            </div>
          </div>

          <!-- Distribution metrics -->
          <div v-if="assertion.distribution" class="mt-3 grid grid-cols-2 sm:grid-cols-4 gap-3 border-t border-[rgb(var(--color-border)/0.5)] pt-3">
            <div>
              <p class="text-[10px] uppercase tracking-wider text-[rgb(var(--color-muted-foreground))]">Median</p>
              <p class="text-sm font-medium font-mono text-[rgb(var(--color-foreground))]">{{ assertion.distribution.median_impact.toFixed(3) }}</p>
            </div>
            <div>
              <p class="text-[10px] uppercase tracking-wider text-[rgb(var(--color-muted-foreground))]">Learners</p>
              <p class="text-sm font-medium font-mono text-[rgb(var(--color-foreground))]">{{ assertion.distribution.learner_count }}</p>
            </div>
            <div>
              <p class="text-[10px] uppercase tracking-wider text-[rgb(var(--color-muted-foreground))]">Variance</p>
              <p class="text-sm font-medium font-mono text-[rgb(var(--color-foreground))]">{{ assertion.distribution.impact_variance.toFixed(3) }}</p>
            </div>
            <div>
              <p class="text-[10px] uppercase tracking-wider text-[rgb(var(--color-muted-foreground))]">Evidence</p>
              <p class="text-sm font-medium font-mono text-[rgb(var(--color-foreground))]">{{ assertion.evidence_count }}</p>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
