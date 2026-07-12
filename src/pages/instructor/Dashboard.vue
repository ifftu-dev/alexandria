<script setup lang="ts">
// Instructor-mode home: per-course aggregates + inbox shortcut.
// Aggregates cover what this node knows (local-first) — see
// commands/instructor.rs for the scope caveat.
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, EmptyState, StatusBadge } from '@/components/ui'
import type { CourseOverview, InboxItem } from '@/types'

const { invoke } = useLocalApi()
const router = useRouter()

const overview = ref<CourseOverview[]>([])
const inboxCount = ref(0)
const loading = ref(true)

const totals = computed(() => ({
  courses: overview.value.length,
  published: overview.value.filter(c => c.status === 'published').length,
  enrollments: overview.value.reduce((n, c) => n + c.enrollment_count, 0),
  pendingReviews: overview.value.reduce((n, c) => n + c.pending_reviews, 0),
}))

onMounted(async () => {
  try {
    const [ov, inbox] = await Promise.all([
      invoke<CourseOverview[]>('instructor_overview'),
      invoke<InboxItem[]>('instructor_inbox').catch(() => []),
    ])
    overview.value = ov
    inboxCount.value = inbox.length
  } finally {
    loading.value = false
  }
})

function pct(c: CourseOverview): string {
  if (!c.enrollment_count) return '—'
  return `${Math.round((c.completed_count / c.enrollment_count) * 100)}%`
}

function score(c: CourseOverview): string {
  return c.avg_score === null ? '—' : `${Math.round(c.avg_score * 100)}%`
}
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-start justify-between gap-4">
      <div>
        <h1 class="text-2xl font-bold text-foreground">{{ $t('instructor.dashboard.title') }}</h1>
        <p class="mt-1 text-sm text-muted-foreground">
          {{ $t('instructor.dashboard.subtitle') }}
        </p>
      </div>
      <div class="flex shrink-0 gap-2">
        <AppButton variant="outline" size="sm" @click="router.push('/instructor/inbox')">
          {{ $t('instructor.dashboard.inbox') }}
          <span v-if="inboxCount" class="ms-1.5 rounded-full bg-error px-1.5 text-[10px] font-bold text-white">
            {{ inboxCount }}
          </span>
        </AppButton>
        <AppButton size="sm" @click="router.push('/instructor/composer/new?kind=course')">
          {{ $t('instructor.dashboard.compose') }}
        </AppButton>
      </div>
    </div>

    <!-- Stat cards -->
    <div class="grid grid-cols-2 gap-4 lg:grid-cols-4">
      <div class="rounded-xl border border-border bg-card p-5">
        <p class="text-xs text-muted-foreground">{{ $t('instructor.dashboard.statCourses') }}</p>
        <p class="mt-1 text-2xl font-bold text-foreground">{{ totals.courses }}</p>
      </div>
      <div class="rounded-xl border border-border bg-card p-5">
        <p class="text-xs text-muted-foreground">{{ $t('instructor.dashboard.statPublished') }}</p>
        <p class="mt-1 text-2xl font-bold text-foreground">{{ totals.published }}</p>
      </div>
      <div class="rounded-xl border border-border bg-card p-5">
        <p class="text-xs text-muted-foreground">{{ $t('instructor.dashboard.statEnrollments') }}</p>
        <p class="mt-1 text-2xl font-bold text-primary">{{ totals.enrollments }}</p>
      </div>
      <button
        class="rounded-xl border border-border bg-card p-5 text-start transition-colors hover:border-primary/50"
        @click="router.push('/instructor/inbox')"
      >
        <p class="text-xs text-muted-foreground">{{ $t('instructor.dashboard.statPendingReviews') }}</p>
        <p class="mt-1 text-2xl font-bold" :class="totals.pendingReviews ? 'text-warning' : 'text-foreground'">
          {{ totals.pendingReviews }}
        </p>
      </button>
    </div>

    <!-- Course table -->
    <div v-if="loading" class="space-y-2">
      <div v-for="i in 4" :key="i" class="h-14 animate-pulse rounded-lg bg-muted-foreground/8" />
    </div>

    <EmptyState
      v-else-if="!overview.length"
      :title="$t('instructor.dashboard.emptyTitle')"
      :description="$t('instructor.dashboard.emptyDesc')"
    />

    <div v-else class="overflow-x-auto rounded-xl border border-border bg-card">
      <table class="w-full text-sm">
        <thead>
          <tr class="border-b border-border text-start text-xs text-muted-foreground">
            <th class="px-4 py-3 font-medium">{{ $t('instructor.dashboard.colCourse') }}</th>
            <th class="px-4 py-3 font-medium">{{ $t('instructor.dashboard.colStatus') }}</th>
            <th class="px-4 py-3 font-medium text-end">{{ $t('instructor.dashboard.colEnrolled') }}</th>
            <th class="px-4 py-3 font-medium text-end">{{ $t('instructor.dashboard.colCompletion') }}</th>
            <th class="px-4 py-3 font-medium text-end">{{ $t('instructor.dashboard.colAvgScore') }}</th>
            <th class="px-4 py-3 font-medium text-end">{{ $t('instructor.dashboard.colReviews') }}</th>
            <th class="px-4 py-3 font-medium">{{ $t('instructor.dashboard.colLastActivity') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="c in overview"
            :key="c.course_id"
            class="cursor-pointer border-b border-border/50 last:border-0 hover:bg-muted/20"
            @click="router.push(`/instructor/courses/${c.course_id}/learners`)"
          >
            <td class="px-4 py-3">
              <span class="font-medium text-foreground">{{ c.title }}</span>
              <span class="ms-2 rounded-full bg-muted px-1.5 py-0.5 text-[10px] uppercase text-muted-foreground">{{ c.kind }}</span>
            </td>
            <td class="px-4 py-3"><StatusBadge :status="c.status" /></td>
            <td class="px-4 py-3 text-end">{{ c.enrollment_count }}</td>
            <td class="px-4 py-3 text-end">{{ pct(c) }}</td>
            <td class="px-4 py-3 text-end">{{ score(c) }}</td>
            <td class="px-4 py-3 text-end">
              <span v-if="c.pending_reviews" class="rounded-full bg-warning/15 px-2 py-0.5 text-xs font-semibold text-warning">
                {{ c.pending_reviews }}
              </span>
              <span v-else class="text-muted-foreground">—</span>
            </td>
            <td class="px-4 py-3 text-xs text-muted-foreground">{{ c.last_activity?.slice(0, 16) ?? '—' }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>
