<script setup lang="ts">
// One child's mirrored activity: courses & progress, submissions,
// classrooms — everything their device has pushed over the sealed
// guardian link.
import { computed, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useGuardian, childAge } from '@/composables/useGuardian'
import { AppBadge, AppButton, AppTabs, EmptyState } from '@/components/ui'
import type { GuardianLinkInfo } from '@/types'

const route = useRoute()
const { t } = useI18n()
const linkId = route.params.linkId as string

const { links, refreshLinks, syncNow, childActivity, revokeLink } = useGuardian()

const link = computed<GuardianLinkInfo | null>(
  () => links.value.find(l => l.id === linkId) ?? null,
)

interface EnrollmentRow {
  id: string
  course_id: string
  enrolled_at: string
  completed_at: string | null
  status: string
}
interface ProgressRow {
  id: string
  enrollment_id: string
  element_id: string
  status: string
  score: number | null
  time_spent: number | null
  updated_at: string
}
interface SubmissionRow {
  id: string
  element_id: string | null
  score: number | null
  feedback?: string | null
  status?: string
  created_at: string
}
interface ClassroomRow {
  classroom_id: string
  role: string
  display_name: string | null
  joined_at: string
}
interface CourseRow {
  id: string
  title: string
  kind: string
}

const enrollments = ref<EnrollmentRow[]>([])
const progress = ref<ProgressRow[]>([])
const submissions = ref<SubmissionRow[]>([])
const irlSubmissions = ref<SubmissionRow[]>([])
const classrooms = ref<ClassroomRow[]>([])
const courses = ref<CourseRow[]>([])
const loading = ref(true)
const syncing = ref(false)
const revoking = ref(false)

const activeTab = ref('overview')
const tabs = computed(() => [
  { key: 'overview', label: t('guardian.detail.tabOverview') },
  { key: 'courses', label: t('guardian.detail.tabCourses') },
  { key: 'submissions', label: t('guardian.detail.tabSubmissions') },
  { key: 'classrooms', label: t('guardian.detail.tabClassrooms') },
])

const courseTitle = (courseId: string) =>
  courses.value.find(c => c.id === courseId)?.title ?? `${courseId.slice(0, 12)}…`

const progressByEnrollment = computed(() => {
  const map = new Map<string, ProgressRow[]>()
  for (const p of progress.value) {
    const list = map.get(p.enrollment_id) ?? []
    list.push(p)
    map.set(p.enrollment_id, list)
  }
  return map
})

const totals = computed(() => {
  const completed = progress.value.filter(p => p.status === 'completed').length
  const timeSpent = progress.value.reduce((n, p) => n + (p.time_spent ?? 0), 0)
  const scored = progress.value.filter(p => p.score !== null)
  const avgScore = scored.length
    ? scored.reduce((n, p) => n + (p.score ?? 0), 0) / scored.length
    : null
  return {
    enrollments: enrollments.value.length,
    completedElements: completed,
    timeSpentHours: (timeSpent / 3600).toFixed(1),
    avgScore,
    submissions: submissions.value.length + irlSubmissions.value.length,
    classrooms: classrooms.value.length,
  }
})

onMounted(async () => {
  await refreshLinks()
  await loadActivity()
})

async function loadActivity() {
  loading.value = true
  try {
    ;[enrollments.value, progress.value, submissions.value, irlSubmissions.value, classrooms.value, courses.value] =
      await Promise.all([
        childActivity<EnrollmentRow>(linkId, 'enrollments'),
        childActivity<ProgressRow>(linkId, 'element_progress'),
        childActivity<SubmissionRow>(linkId, 'element_submissions'),
        childActivity<SubmissionRow>(linkId, 'plugin_irl_submissions'),
        childActivity<ClassroomRow>(linkId, 'classroom_members'),
        childActivity<CourseRow>(linkId, 'courses'),
      ])
  } finally {
    loading.value = false
  }
}

async function runSync() {
  syncing.value = true
  try {
    await syncNow()
    await loadActivity()
  } finally {
    syncing.value = false
  }
}

async function unlink() {
  if (!window.confirm(t('guardian.detail.unlinkConfirm'))) {
    return
  }
  revoking.value = true
  try {
    await revokeLink(linkId)
  } finally {
    revoking.value = false
  }
}

function pct(v: number | null): string {
  return v === null ? '—' : `${Math.round(v * 100)}%`
}
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-start justify-between gap-4">
      <div class="flex items-center gap-3">
        <span class="flex h-12 w-12 items-center justify-center rounded-full bg-[color:var(--mode-guardian-accent)]/15 text-xl font-bold text-[color:var(--mode-guardian-accent)]">
          {{ (link?.peer_display_name ?? '?').charAt(0).toUpperCase() }}
        </span>
        <div>
          <h1 class="text-2xl font-bold text-foreground">
            {{ link?.peer_display_name ?? $t('guardian.detail.unnamedChild') }}
            <span v-if="link && childAge(link) !== null" class="text-base font-normal text-muted-foreground">
              · {{ $t('guardian.detail.yearsOld', { age: childAge(link) }) }}
            </span>
          </h1>
          <p class="text-xs text-muted-foreground">
            {{ link?.last_sync_at ? $t('guardian.children.lastSynced', { time: link.last_sync_at.slice(0, 16) }) : $t('guardian.children.notSynced') }}
          </p>
        </div>
      </div>
      <div class="flex shrink-0 gap-2">
        <AppButton variant="outline" size="sm" :loading="syncing" @click="runSync">{{ $t('guardian.actions.syncNow') }}</AppButton>
        <AppButton variant="danger" size="sm" :loading="revoking" @click="unlink">{{ $t('guardian.actions.unlink') }}</AppButton>
      </div>
    </div>

    <AppTabs v-model="activeTab" :tabs="tabs" />

    <div v-if="loading" class="space-y-2">
      <div v-for="i in 4" :key="i" class="h-14 animate-pulse rounded-lg bg-muted-foreground/8" />
    </div>

    <!-- Overview -->
    <template v-else-if="activeTab === 'overview'">
      <div class="grid grid-cols-2 gap-4 lg:grid-cols-3">
        <div class="rounded-xl border border-border bg-card p-5">
          <p class="text-xs text-muted-foreground">{{ $t('guardian.detail.statEnrolledCourses') }}</p>
          <p class="mt-1 text-2xl font-bold text-foreground">{{ totals.enrollments }}</p>
        </div>
        <div class="rounded-xl border border-border bg-card p-5">
          <p class="text-xs text-muted-foreground">{{ $t('guardian.detail.statElementsCompleted') }}</p>
          <p class="mt-1 text-2xl font-bold text-foreground">{{ totals.completedElements }}</p>
        </div>
        <div class="rounded-xl border border-border bg-card p-5">
          <p class="text-xs text-muted-foreground">{{ $t('guardian.detail.statTimeSpent') }}</p>
          <p class="mt-1 text-2xl font-bold text-foreground">{{ totals.timeSpentHours }}h</p>
        </div>
        <div class="rounded-xl border border-border bg-card p-5">
          <p class="text-xs text-muted-foreground">{{ $t('guardian.detail.statAvgScore') }}</p>
          <p class="mt-1 text-2xl font-bold text-foreground">{{ pct(totals.avgScore) }}</p>
        </div>
        <div class="rounded-xl border border-border bg-card p-5">
          <p class="text-xs text-muted-foreground">{{ $t('guardian.detail.statSubmissions') }}</p>
          <p class="mt-1 text-2xl font-bold text-foreground">{{ totals.submissions }}</p>
        </div>
        <div class="rounded-xl border border-border bg-card p-5">
          <p class="text-xs text-muted-foreground">{{ $t('guardian.detail.statClassrooms') }}</p>
          <p class="mt-1 text-2xl font-bold text-foreground">{{ totals.classrooms }}</p>
        </div>
      </div>

      <!-- Recent activity -->
      <div class="rounded-xl border border-border bg-card p-5">
        <h2 class="mb-3 text-sm font-semibold text-foreground">{{ $t('guardian.detail.recentActivity') }}</h2>
        <EmptyState
          v-if="!progress.length"
          :title="$t('guardian.detail.noActivityTitle')"
          :description="$t('guardian.detail.noActivityDescription')"
        />
        <ul v-else class="space-y-2">
          <li
            v-for="p in [...progress].sort((a, b) => b.updated_at.localeCompare(a.updated_at)).slice(0, 10)"
            :key="p.id"
            class="flex items-center gap-3 text-sm"
          >
            <span
              class="h-2 w-2 shrink-0 rounded-full"
              :class="p.status === 'completed' ? 'bg-success' : 'bg-warning'"
            />
            <span class="text-foreground capitalize">{{ p.status.replace('_', ' ') }}</span>
            <span class="text-muted-foreground">{{ $t('guardian.detail.anElement') }}</span>
            <span v-if="p.score !== null" class="text-muted-foreground">{{ $t('guardian.detail.scored', { score: pct(p.score) }) }}</span>
            <span class="ml-auto shrink-0 text-xs text-muted-foreground">{{ p.updated_at.slice(0, 16) }}</span>
          </li>
        </ul>
      </div>
    </template>

    <!-- Courses -->
    <template v-else-if="activeTab === 'courses'">
      <EmptyState
        v-if="!enrollments.length"
        :title="$t('guardian.detail.noEnrollmentsTitle')"
        :description="$t('guardian.detail.noEnrollmentsDescription')"
      />
      <div v-else class="space-y-3">
        <div
          v-for="en in enrollments"
          :key="en.id"
          class="rounded-xl border border-border bg-card p-5"
        >
          <div class="flex items-center gap-2">
            <h3 class="font-semibold text-foreground">{{ courseTitle(en.course_id) }}</h3>
            <AppBadge :variant="en.status === 'completed' ? 'success' : 'primary'" class="capitalize">
              {{ en.status }}
            </AppBadge>
            <span class="ml-auto text-xs text-muted-foreground">{{ $t('guardian.detail.enrolledOn', { date: en.enrolled_at.slice(0, 10) }) }}</span>
          </div>
          <p class="mt-2 text-sm text-muted-foreground">
            {{ $t('guardian.detail.elementsCompletedMinutes', {
              completed: (progressByEnrollment.get(en.id) ?? []).filter(p => p.status === 'completed').length,
              minutes: (((progressByEnrollment.get(en.id) ?? []).reduce((n, p) => n + (p.time_spent ?? 0), 0)) / 60).toFixed(0),
            }) }}
          </p>
        </div>
      </div>
    </template>

    <!-- Submissions -->
    <template v-else-if="activeTab === 'submissions'">
      <EmptyState
        v-if="!submissions.length && !irlSubmissions.length"
        :title="$t('guardian.detail.noSubmissionsTitle')"
        :description="$t('guardian.detail.noSubmissionsDescription')"
      />
      <div v-else class="space-y-3">
        <div
          v-for="s in [...submissions, ...irlSubmissions].sort((a, b) => b.created_at.localeCompare(a.created_at))"
          :key="s.id"
          class="rounded-xl border border-border bg-card p-4"
        >
          <div class="flex items-center gap-2 text-sm">
            <span class="font-medium text-foreground">{{ $t('guardian.detail.submission') }}</span>
            <AppBadge v-if="s.status" class="capitalize">{{ s.status }}</AppBadge>
            <span class="text-muted-foreground">{{ $t('guardian.detail.score', { score: pct(s.score) }) }}</span>
            <span class="ml-auto text-xs text-muted-foreground">{{ s.created_at.slice(0, 16) }}</span>
          </div>
          <p v-if="s.feedback" class="mt-1 text-sm text-muted-foreground">“{{ s.feedback }}”</p>
        </div>
      </div>
    </template>

    <!-- Classrooms -->
    <template v-else>
      <EmptyState
        v-if="!classrooms.length"
        :title="$t('guardian.detail.noClassroomsTitle')"
        :description="$t('guardian.detail.noClassroomsDescription')"
      />
      <div v-else class="space-y-3">
        <div
          v-for="c in classrooms"
          :key="c.classroom_id"
          class="flex items-center gap-3 rounded-xl border border-border bg-card p-4"
        >
          <span class="flex h-9 w-9 items-center justify-center rounded-lg bg-primary/10 text-sm font-bold text-primary">
            {{ (c.display_name ?? 'C').charAt(0).toUpperCase() }}
          </span>
          <div class="min-w-0">
            <p class="truncate text-sm font-medium text-foreground">{{ c.classroom_id.slice(0, 20) }}…</p>
            <p class="text-xs text-muted-foreground capitalize">{{ $t('guardian.detail.roleJoined', { role: c.role, date: c.joined_at.slice(0, 10) }) }}</p>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
