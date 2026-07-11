<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import type { RouteLocationRaw } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { useCredentials } from '@/composables/useCredentials'
import { AppButton } from '@/components/ui'
import { sanitizeSvg } from '@/utils/sanitize'
import {
  extractSkillClaim,
  type Enrollment,
  type Course,
  type Chapter,
  type Element,
  type ElementProgress,
  type VerifiableCredential,
} from '@/types'

const { t } = useI18n()
const { invoke } = useLocalApi()
const { list: listCredentials, credentials } = useCredentials()

const enrollments = ref<Enrollment[]>([])
const courseMap = ref<Record<string, Course>>({})
const enrollmentProgress = ref<Record<string, number>>({})
const progressLoading = ref(false)
const loading = ref(true)
const showCompleted = ref(false)

// Stats
const total = computed(() => enrollments.value.length)
const inProgress = computed(() => enrollments.value.filter(e => !e.completed_at).length)
const completed = computed(() => enrollments.value.filter(e => !!e.completed_at).length)

// Filtered list
const filteredEnrollments = computed(() => {
  if (showCompleted.value) return enrollments.value
  return enrollments.value.filter(e => !e.completed_at)
})

function formatDate(dateStr: string | null): string {
  if (!dateStr) return '—'
  const d = new Date(dateStr)
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' })
}

function formatRelativeTime(dateStr: string | null): string {
  if (!dateStr) return '—'
  const now = Date.now()
  const then = new Date(dateStr).getTime()
  const diff = now - then
  const seconds = Math.floor(diff / 1000)
  const minutes = Math.floor(seconds / 60)
  const hours = Math.floor(minutes / 60)
  const days = Math.floor(hours / 24)
  const weeks = Math.floor(days / 7)
  const months = Math.floor(days / 30)

  if (seconds < 60) return t('courses.relativeTime.justNow')
  if (minutes < 60) return t('courses.relativeTime.minutesAgo', { count: minutes })
  if (hours < 24) return t('courses.relativeTime.hoursAgo', { count: hours })
  if (days < 7) return t('courses.relativeTime.daysAgo', { count: days })
  if (weeks < 5) return t('courses.relativeTime.weeksAgo', { count: weeks })
  return t('courses.relativeTime.monthsAgo', { count: months })
}

async function computeEnrollmentProgressPercent(enrollment: Enrollment): Promise<number> {
  try {
    const chapters = await invoke<Chapter[]>('list_chapters', { courseId: enrollment.course_id })
    const elementLists = await Promise.all(
      chapters.map((chapter) => invoke<Element[]>('list_elements', { chapterId: chapter.id }).catch(() => [])),
    )

    const totalElements = elementLists.reduce((sum, items) => sum + items.length, 0)
    if (totalElements === 0) return enrollment.completed_at ? 100 : 0

    const progressRows = await invoke<ElementProgress[]>('get_progress', { enrollmentId: enrollment.id })
    const completedCount = progressRows.filter((row) => row.status === 'completed').length
    if (enrollment.completed_at) return 100
    return Math.round((completedCount / totalElements) * 100)
  } catch {
    return enrollment.completed_at ? 100 : 0
  }
}

function progressPercentFor(enrollment: Enrollment): number {
  return enrollmentProgress.value[enrollment.id] ?? (enrollment.completed_at ? 100 : 0)
}

function isProgressReady(enrollment: Enrollment): boolean {
  return enrollment.completed_at !== null || enrollmentProgress.value[enrollment.id] !== undefined
}

/** Whether the enrolled content is a standalone tutorial vs a full course. */
function isTutorial(courseId: string): boolean {
  return courseMap.value[courseId]?.kind === 'tutorial'
}

/** Display label for the kind badge. */
function kindLabel(courseId: string): string {
  return isTutorial(courseId) ? t('courses.dashboard.kindTutorial') : t('courses.dashboard.kindCourse')
}

/**
 * Credentials earned for a course, matched by skill. Completion VCs link
 * to a course only via a backend-only table, so we match the credential's
 * skill claim against the course's `skill_ids` — the best signal exposed
 * to the frontend.
 */
function credentialsForCourse(courseId: string): VerifiableCredential[] {
  const skills = courseMap.value[courseId]?.skill_ids ?? []
  if (skills.length === 0) return []
  return credentials.value.filter((c) => {
    const claim = extractSkillClaim(c.credentialSubject)
    return claim ? skills.includes(claim.skillId) : false
  })
}

/**
 * Where the "View credential" action navigates: straight to the credential
 * detail when exactly one matches, the credentials list filtered to the
 * course's skill when several match, or the plain credentials list as a
 * fallback (skill matching can miss).
 */
function credentialLinkFor(enrollment: Enrollment): RouteLocationRaw {
  const matches = credentialsForCourse(enrollment.course_id)
  const only = matches.length === 1 ? matches[0] : undefined
  if (only?.id) {
    return { name: 'credential-detail', params: { id: only.id } }
  }
  const firstSkill = courseMap.value[enrollment.course_id]?.skill_ids?.[0]
  if (matches.length > 1 && firstSkill) {
    return { name: 'credentials', query: { skill: firstSkill } }
  }
  return { name: 'credentials' }
}

onMounted(async () => {
  try {
    enrollments.value = await invoke<Enrollment[]>('list_enrollments')

    // Fetch course details for enrolled courses (includes tutorials —
    // they are `courses` rows with `kind = 'tutorial'`).
    const courses = await invoke<Course[]>('list_courses')
    for (const c of courses) {
      courseMap.value[c.id] = c
    }

    // Load credentials so completed content can link to what was earned.
    void listCredentials()

    progressLoading.value = true
    const progressEntries = await Promise.all(
      enrollments.value.map(async (enrollment) => {
        const percent = await computeEnrollmentProgressPercent(enrollment)
        return [enrollment.id, percent] as const
      }),
    )

    enrollmentProgress.value = Object.fromEntries(progressEntries)
    progressLoading.value = false
  } catch (e) {
    console.error('Failed to load enrollments:', e)
  } finally {
    progressLoading.value = false
    loading.value = false
  }
})
</script>

<template>
  <div class="min-h-screen">
    <!-- Header -->
    <div>
      <h1 class="text-3xl font-bold">{{ $t('courses.dashboard.title') }}</h1>
      <p class="mt-2 text-muted-foreground">
        {{ $t('courses.dashboard.subtitle') }}
      </p>
    </div>

    <div class="px-4 sm:px-6 lg:px-8 space-y-6">
      <!-- Skeleton loader -->
      <template v-if="loading">
        <!-- Stats skeleton -->
        <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
          <div
            v-for="i in 3"
            :key="i"
            class="animate-pulse rounded-xl bg-card shadow-sm p-6"
          >
            <div class="h-3 w-24 rounded bg-muted-foreground/15 mb-3" />
            <div class="h-8 w-12 rounded bg-muted-foreground/20" />
          </div>
        </div>

        <!-- Card skeletons -->
        <div class="space-y-4">
          <div
            v-for="i in 3"
            :key="i"
            class="animate-pulse overflow-hidden rounded-xl bg-card shadow-sm"
          >
            <div class="flex flex-col sm:flex-row">
              <div class="h-32 sm:h-auto sm:w-48 bg-muted-foreground/10" />
              <div class="flex-1 p-5 space-y-3">
                <div class="h-4 w-48 rounded bg-muted-foreground/15" />
                <div class="h-3 w-full rounded bg-muted-foreground/10" />
                <div class="h-3 w-2/3 rounded bg-muted-foreground/10" />
                <div class="h-2 w-full rounded-full bg-muted-foreground/10 mt-4" />
                <div class="flex gap-4 mt-3">
                  <div class="h-3 w-24 rounded bg-muted-foreground/10" />
                  <div class="h-3 w-20 rounded bg-muted-foreground/10" />
                </div>
              </div>
            </div>
          </div>
        </div>
      </template>

      <!-- Loaded content -->
      <template v-else>
        <!-- Stats grid -->
        <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
          <div class="rounded-xl bg-card shadow-sm p-6">
            <p class="text-sm text-muted-foreground">{{ $t('courses.dashboard.totalEnrolled') }}</p>
            <p class="text-3xl font-bold mt-1">{{ total }}</p>
          </div>
          <div class="rounded-xl bg-card shadow-sm p-6">
            <p class="text-sm text-muted-foreground">{{ $t('courses.dashboard.inProgress') }}</p>
            <p class="text-3xl font-bold text-yellow-400 mt-1">{{ inProgress }}</p>
          </div>
          <div class="rounded-xl bg-card shadow-sm p-6">
            <p class="text-sm text-muted-foreground">{{ $t('courses.dashboard.completed') }}</p>
            <p class="text-3xl font-bold text-green-400 mt-1">{{ completed }}</p>
          </div>
        </div>

        <!-- Filter toggle -->
        <div class="flex gap-2">
          <button
            class="rounded-lg px-4 py-2 text-sm font-medium transition-colors"
            :class="!showCompleted
              ? 'bg-primary text-white'
              : 'bg-muted/30 text-muted-foreground hover:bg-muted/50'"
            @click="showCompleted = false"
          >
            {{ $t('courses.dashboard.filterInProgress') }}
          </button>
          <button
            class="rounded-lg px-4 py-2 text-sm font-medium transition-colors"
            :class="showCompleted
              ? 'bg-primary text-white'
              : 'bg-muted/30 text-muted-foreground hover:bg-muted/50'"
            @click="showCompleted = true"
          >
            {{ $t('courses.dashboard.filterAll') }}
          </button>
        </div>

        <!-- Empty state -->
        <div
          v-if="filteredEnrollments.length === 0 && enrollments.length === 0"
          class="flex flex-col items-center justify-center py-20 text-center"
        >
          <div class="flex h-20 w-20 items-center justify-center rounded-full bg-primary/10 mb-6">
            <svg
              class="h-10 w-10 text-primary"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="1.5"
            >
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                d="M12 6.042A8.967 8.967 0 006 3.75c-1.052 0-2.062.18-3 .512v14.25A8.987 8.987 0 016 18c2.305 0 4.408.867 6 2.292m0-14.25a8.966 8.966 0 016-2.292c1.052 0 2.062.18 3 .512v14.25A8.987 8.987 0 0018 18a8.967 8.967 0 00-6 2.292m0-14.25v14.25"
              />
            </svg>
          </div>
          <h3 class="text-lg font-semibold mb-2">{{ $t('courses.dashboard.emptyTitle') }}</h3>
          <p class="text-sm text-muted-foreground max-w-sm mb-6">
            {{ $t('courses.dashboard.emptyBody') }}
          </p>
          <AppButton @click="$router.push('/courses')">
            {{ $t('courses.dashboard.browseCourses') }}
          </AppButton>
        </div>

        <!-- Filtered empty (has enrollments but none match filter) -->
        <div
          v-else-if="filteredEnrollments.length === 0"
          class="flex flex-col items-center justify-center py-16 text-center"
        >
          <p class="text-sm text-muted-foreground">
            {{ $t('courses.dashboard.filteredEmpty') }}
          </p>
        </div>

        <!-- Course cards -->
        <div v-else class="space-y-4">
          <router-link
            v-for="enrollment in filteredEnrollments"
            :key="enrollment.id"
            :to="enrollment.status === 'active' ? `/learn/${enrollment.course_id}` : `/courses/${enrollment.course_id}`"
            class="group block overflow-hidden rounded-xl bg-card shadow-sm transition-all hover:shadow-md hover:-translate-y-0.5"
          >
            <div class="flex flex-col sm:flex-row">
              <!-- Thumbnail -->
              <div
                class="relative h-32 sm:h-auto sm:w-48 flex-shrink-0 overflow-hidden"
              >
                <div v-if="courseMap[enrollment.course_id]?.thumbnail_svg" class="w-full h-full" v-html="sanitizeSvg(courseMap[enrollment.course_id]?.thumbnail_svg ?? '')" />
                <div v-else class="w-full h-full bg-gradient-to-br from-primary/30 via-primary/15 to-card flex items-center justify-center">
                  <svg
                    class="h-10 w-10 text-primary/40"
                    fill="none"
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                    stroke-width="1"
                  >
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      d="M4.26 10.147a60.436 60.436 0 00-.491 6.347A48.627 48.627 0 0112 20.904a48.627 48.627 0 018.232-4.41 60.46 60.46 0 00-.491-6.347m-15.482 0a50.57 50.57 0 00-2.658-.813A59.905 59.905 0 0112 3.493a59.902 59.902 0 0110.399 5.84c-.896.248-1.783.52-2.658.814m-15.482 0A50.697 50.697 0 0112 13.489a50.702 50.702 0 017.74-3.342"
                    />
                  </svg>
                </div>
                <!-- Completed badge -->
                <div
                  v-if="enrollment.completed_at"
                  class="absolute top-3 right-3 sm:top-2 sm:right-2"
                >
                  <span class="inline-flex items-center gap-1 rounded-full bg-green-500/10 px-2.5 py-1 text-xs font-medium text-green-400">
                    <svg class="h-3 w-3" fill="currentColor" viewBox="0 0 20 20">
                      <path
                        fill-rule="evenodd"
                        d="M16.704 4.153a.75.75 0 01.143 1.052l-8 10.5a.75.75 0 01-1.127.075l-4.5-4.5a.75.75 0 011.06-1.06l3.894 3.893 7.48-9.817a.75.75 0 011.05-.143z"
                        clip-rule="evenodd"
                      />
                    </svg>
                    {{ $t('courses.dashboard.badgeCompleted') }}
                  </span>
                </div>
              </div>

              <!-- Content -->
              <div class="flex-1 p-5">
                <div class="flex items-start justify-between gap-4">
                  <div class="min-w-0 flex-1">
                    <div class="mb-1.5">
                      <span
                        class="inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide"
                        :class="isTutorial(enrollment.course_id)
                          ? 'bg-purple-500/10 text-purple-400'
                          : 'bg-primary/10 text-primary'"
                      >
                        {{ kindLabel(enrollment.course_id) }}
                      </span>
                    </div>
                    <h3 class="text-base font-semibold leading-tight">
                      {{ courseMap[enrollment.course_id]?.title ?? enrollment.course_id }}
                    </h3>
                    <p
                      v-if="courseMap[enrollment.course_id]?.description"
                      class="mt-1.5 text-sm text-muted-foreground line-clamp-2"
                    >
                      {{ courseMap[enrollment.course_id]?.description }}
                    </p>
                  </div>
                </div>

                <!-- Progress bar -->
                <div class="mt-4">
                  <div class="flex items-center justify-between text-xs mb-1.5">
                    <span class="text-muted-foreground">{{ $t('courses.dashboard.progress') }}</span>
                    <span
                      v-if="!progressLoading || isProgressReady(enrollment)"
                      class="font-medium"
                      :class="enrollment.completed_at ? 'text-green-400' : 'text-primary'"
                    >
                      {{ progressPercentFor(enrollment) }}%
                    </span>
                    <span v-else class="h-3 w-10 animate-pulse rounded bg-muted/40" />
                  </div>
                  <div class="h-1.5 w-full overflow-hidden rounded-full bg-muted/30">
                    <div
                      v-if="!progressLoading || isProgressReady(enrollment)"
                      class="h-full rounded-full transition-all duration-500"
                      :class="enrollment.completed_at ? 'bg-green-400' : 'bg-primary'"
                      :style="{ width: `${progressPercentFor(enrollment)}%` }"
                    />
                    <div v-else class="h-full w-2/5 animate-pulse rounded-full bg-muted/40" />
                  </div>
                </div>

                <!-- Meta info & action -->
                <div class="mt-4 flex flex-wrap items-center justify-between gap-3">
                  <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
                    <span>
                      {{ $t('courses.dashboard.started', { date: formatDate(enrollment.enrolled_at) }) }}
                    </span>
                    <span v-if="enrollment.updated_at && !enrollment.completed_at">
                      {{ $t('courses.dashboard.lastAccessed', { time: formatRelativeTime(enrollment.updated_at) }) }}
                    </span>
                    <span v-if="enrollment.completed_at">
                      {{ $t('courses.dashboard.completedOn', { date: formatDate(enrollment.completed_at) }) }}
                    </span>
                  </div>
                  <div class="flex flex-wrap items-center gap-2">
                    <!-- View earned credential(s) for completed content -->
                    <button
                      v-if="enrollment.completed_at"
                      type="button"
                      class="inline-flex items-center gap-1.5 rounded-lg bg-green-500/10 px-3 py-1.5 text-xs font-medium text-green-400 transition-colors hover:bg-green-500/20"
                      @click.stop.prevent="$router.push(credentialLinkFor(enrollment))"
                    >
                      <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" />
                      </svg>
                      {{ $t('courses.dashboard.viewCredential') }}
                    </button>
                    <span
                      class="inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium transition-colors"
                      :class="enrollment.completed_at
                        ? 'bg-muted/30 text-muted-foreground group-hover:bg-muted/50'
                        : 'bg-primary/10 text-primary group-hover:bg-primary/20'"
                    >
                      {{ enrollment.completed_at
                        ? (isTutorial(enrollment.course_id) ? $t('courses.dashboard.rewatchTutorial') : $t('courses.dashboard.reviewCourse'))
                        : $t('courses.dashboard.continueLearning') }}
                      <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M13.5 4.5L21 12m0 0l-7.5 7.5M21 12H3" />
                      </svg>
                    </span>
                  </div>
                </div>
              </div>
            </div>
          </router-link>
        </div>
      </template>
    </div>
  </div>
</template>
