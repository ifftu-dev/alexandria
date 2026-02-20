<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, EmptyState, AppTabs, AppBadge } from '@/components/ui'
import type { FullReputationAssertion, ReputationQuery } from '@/types'

const { invoke } = useLocalApi()

const assertions = ref<FullReputationAssertion[]>([])
const loading = ref(true)
const activeTab = ref('instructor')

const tabs = [
  { key: 'instructor', label: 'Instructor' },
  { key: 'learner', label: 'Learner' },
]

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

function filterByRole(role: string): FullReputationAssertion[] {
  return assertions.value.filter(a => a.role === role)
}
</script>

<template>
  <div>
    <h1 class="text-xl font-bold mb-1">Reputation</h1>
    <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
      Your evidence-derived reputation scores. Scoped by role, skill, and proficiency level.
    </p>

    <AppTabs v-model="activeTab" :tabs="tabs" class="mb-6" />

    <AppSpinner v-if="loading" label="Loading reputation..." />

    <template v-else>
      <EmptyState
        v-if="filterByRole(activeTab).length === 0"
        :title="`No ${activeTab} reputation yet`"
        :description="activeTab === 'instructor'
          ? 'Instructor reputation is computed from learner outcomes on your courses.'
          : 'Learner reputation mirrors your skill proofs directly.'"
      />

      <div v-else class="space-y-3">
        <div
          v-for="assertion in filterByRole(activeTab)"
          :key="assertion.id"
          class="card p-4"
        >
          <div class="flex items-start justify-between mb-2">
            <div>
              <div class="text-sm font-medium">{{ assertion.skill_id ?? 'Global' }}</div>
              <div class="flex items-center gap-2 mt-1">
                <AppBadge variant="primary">{{ assertion.role }}</AppBadge>
                <AppBadge v-if="assertion.proficiency_level" variant="secondary">
                  {{ assertion.proficiency_level }}
                </AppBadge>
              </div>
            </div>
            <div class="text-right">
              <div class="text-lg font-bold text-[rgb(var(--color-primary))]">
                {{ assertion.score.toFixed(3) }}
              </div>
              <div class="text-xs text-[rgb(var(--color-muted-foreground))]">
                {{ (assertion.confidence * 100).toFixed(0) }}% confidence
              </div>
            </div>
          </div>

          <!-- Distribution metrics -->
          <div v-if="assertion.distribution" class="mt-2 pt-2 border-t border-[rgb(var(--color-border))] grid grid-cols-4 gap-2 text-xs text-[rgb(var(--color-muted-foreground))]">
            <div>
              <div class="font-medium text-[rgb(var(--color-foreground))]">{{ assertion.distribution.median_impact.toFixed(3) }}</div>
              Median
            </div>
            <div>
              <div class="font-medium text-[rgb(var(--color-foreground))]">{{ assertion.distribution.learner_count }}</div>
              Learners
            </div>
            <div>
              <div class="font-medium text-[rgb(var(--color-foreground))]">{{ assertion.distribution.impact_variance.toFixed(3) }}</div>
              Variance
            </div>
            <div>
              <div class="font-medium text-[rgb(var(--color-foreground))]">{{ assertion.evidence_count }}</div>
              Evidence
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
