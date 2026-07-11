<script setup lang="ts">
// Per-learner drill-down for one authored course.
import { onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { EmptyState, StatusBadge } from '@/components/ui'
import type { Course, CourseLearner } from '@/types'

const { invoke } = useLocalApi()
const route = useRoute()

const courseId = route.params.id as string
const course = ref<Course | null>(null)
const learners = ref<CourseLearner[]>([])
const loading = ref(true)
const error = ref('')

onMounted(async () => {
  try {
    ;[course.value, learners.value] = await Promise.all([
      invoke<Course>('get_course', { courseId }),
      invoke<CourseLearner[]>('instructor_course_learners', { courseId }),
    ])
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
})

function hours(seconds: number): string {
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`
  return `${(seconds / 3600).toFixed(1)}h`
}
</script>

<template>
  <div class="space-y-6">
    <div>
      <p class="text-xs uppercase tracking-wide text-muted-foreground">{{ $t('instructor.courseLearners.eyebrow') }}</p>
      <h1 class="text-2xl font-bold text-foreground">{{ course?.title ?? $t('instructor.courseLearners.fallbackTitle') }}</h1>
    </div>

    <p v-if="error" class="text-sm text-error">{{ error }}</p>

    <div v-if="loading" class="space-y-2">
      <div v-for="i in 4" :key="i" class="h-12 animate-pulse rounded-lg bg-muted-foreground/8" />
    </div>

    <EmptyState
      v-else-if="!learners.length"
      :title="$t('instructor.courseLearners.emptyTitle')"
      :description="$t('instructor.courseLearners.emptyDesc')"
    />

    <div v-else class="overflow-x-auto rounded-xl border border-border bg-card">
      <table class="w-full text-sm">
        <thead>
          <tr class="border-b border-border text-left text-xs text-muted-foreground">
            <th class="px-4 py-3 font-medium">{{ $t('instructor.courseLearners.colLearner') }}</th>
            <th class="px-4 py-3 font-medium">{{ $t('instructor.courseLearners.colStatus') }}</th>
            <th class="px-4 py-3 font-medium text-right">{{ $t('instructor.courseLearners.colProgress') }}</th>
            <th class="px-4 py-3 font-medium text-right">{{ $t('instructor.courseLearners.colAvgScore') }}</th>
            <th class="px-4 py-3 font-medium text-right">{{ $t('instructor.courseLearners.colTimeSpent') }}</th>
            <th class="px-4 py-3 font-medium">{{ $t('instructor.courseLearners.colLastActivity') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="l in learners" :key="l.enrollment_id" class="border-b border-border/50 last:border-0">
            <td class="px-4 py-3">
              <span class="font-medium text-foreground">
                {{ l.display_name ?? (l.learner_did ? `${l.learner_did.slice(0, 20)}…` : $t('instructor.courseLearners.localLearner')) }}
              </span>
            </td>
            <td class="px-4 py-3"><StatusBadge :status="l.enrollment_status" /></td>
            <td class="px-4 py-3 text-right">
              {{ l.completed_elements }}/{{ l.total_elements }}
              <span class="text-xs text-muted-foreground">
                ({{ l.total_elements ? Math.round((l.completed_elements / l.total_elements) * 100) : 0 }}%)
              </span>
            </td>
            <td class="px-4 py-3 text-right">
              {{ l.avg_score === null ? '—' : `${Math.round(l.avg_score * 100)}%` }}
            </td>
            <td class="px-4 py-3 text-right">{{ hours(l.time_spent_seconds) }}</td>
            <td class="px-4 py-3 text-xs text-muted-foreground">{{ l.last_activity?.slice(0, 16) ?? '—' }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
