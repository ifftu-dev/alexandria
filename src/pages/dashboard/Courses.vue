<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import type { Enrollment, Course } from '@/types'

const { invoke } = useLocalApi()

const enrollments = ref<Enrollment[]>([])
const courseMap = ref<Record<string, Course>>({})
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

  if (seconds < 60) return 'just now'
  if (minutes < 60) return `${minutes}m ago`
  if (hours < 24) return `${hours}h ago`
  if (days < 7) return `${days}d ago`
  if (weeks < 5) return `${weeks}w ago`
  return `${months}mo ago`
}

onMounted(async () => {
  try {
    enrollments.value = await invoke<Enrollment[]>('list_enrollments')

    // Fetch course details for enrolled courses
    const courses = await invoke<Course[]>('list_courses')
    for (const c of courses) {
      courseMap.value[c.id] = c
    }
  } catch (e) {
    console.error('Failed to load enrollments:', e)
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div class="min-h-screen">
    <!-- Header -->
    <div class="py-8 px-4 sm:px-6 lg:px-8">
      <h1 class="text-3xl font-bold">My Courses</h1>
      <p class="mt-2 text-[rgb(var(--color-muted-foreground))]">
        Track your learning progress and continue where you left off.
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
            class="animate-pulse rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6"
          >
            <div class="h-3 w-24 rounded bg-[rgb(var(--color-muted-foreground)/0.15)] mb-3" />
            <div class="h-8 w-12 rounded bg-[rgb(var(--color-muted-foreground)/0.2)]" />
          </div>
        </div>

        <!-- Card skeletons -->
        <div class="space-y-4">
          <div
            v-for="i in 3"
            :key="i"
            class="animate-pulse overflow-hidden rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))]"
          >
            <div class="flex flex-col sm:flex-row">
              <div class="h-32 sm:h-auto sm:w-48 bg-[rgb(var(--color-muted-foreground)/0.1)]" />
              <div class="flex-1 p-5 space-y-3">
                <div class="h-4 w-48 rounded bg-[rgb(var(--color-muted-foreground)/0.15)]" />
                <div class="h-3 w-full rounded bg-[rgb(var(--color-muted-foreground)/0.1)]" />
                <div class="h-3 w-2/3 rounded bg-[rgb(var(--color-muted-foreground)/0.1)]" />
                <div class="h-2 w-full rounded-full bg-[rgb(var(--color-muted-foreground)/0.1)] mt-4" />
                <div class="flex gap-4 mt-3">
                  <div class="h-3 w-24 rounded bg-[rgb(var(--color-muted-foreground)/0.1)]" />
                  <div class="h-3 w-20 rounded bg-[rgb(var(--color-muted-foreground)/0.1)]" />
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
          <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
            <p class="text-sm text-[rgb(var(--color-muted-foreground))]">Total Enrolled</p>
            <p class="text-3xl font-bold mt-1">{{ total }}</p>
          </div>
          <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
            <p class="text-sm text-[rgb(var(--color-muted-foreground))]">In Progress</p>
            <p class="text-3xl font-bold text-yellow-400 mt-1">{{ inProgress }}</p>
          </div>
          <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
            <p class="text-sm text-[rgb(var(--color-muted-foreground))]">Completed</p>
            <p class="text-3xl font-bold text-green-400 mt-1">{{ completed }}</p>
          </div>
        </div>

        <!-- Filter toggle -->
        <div class="flex gap-2">
          <button
            class="rounded-lg px-4 py-2 text-sm font-medium transition-colors"
            :class="!showCompleted
              ? 'bg-[rgb(var(--color-primary))] text-white'
              : 'bg-[rgb(var(--color-muted)/0.3)] text-[rgb(var(--color-muted-foreground))] hover:bg-[rgb(var(--color-muted)/0.5)]'"
            @click="showCompleted = false"
          >
            In Progress
          </button>
          <button
            class="rounded-lg px-4 py-2 text-sm font-medium transition-colors"
            :class="showCompleted
              ? 'bg-[rgb(var(--color-primary))] text-white'
              : 'bg-[rgb(var(--color-muted)/0.3)] text-[rgb(var(--color-muted-foreground))] hover:bg-[rgb(var(--color-muted)/0.5)]'"
            @click="showCompleted = true"
          >
            All Courses
          </button>
        </div>

        <!-- Empty state -->
        <div
          v-if="filteredEnrollments.length === 0 && enrollments.length === 0"
          class="flex flex-col items-center justify-center py-20 text-center"
        >
          <div class="flex h-20 w-20 items-center justify-center rounded-full bg-[rgb(var(--color-primary)/0.1)] mb-6">
            <svg
              class="h-10 w-10 text-[rgb(var(--color-primary))]"
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
          <h3 class="text-lg font-semibold mb-2">No courses yet</h3>
          <p class="text-sm text-[rgb(var(--color-muted-foreground))] max-w-sm mb-6">
            Browse available courses and enroll to start your learning journey.
          </p>
          <AppButton @click="$router.push('/courses')">
            Browse Courses
          </AppButton>
        </div>

        <!-- Filtered empty (has enrollments but none match filter) -->
        <div
          v-else-if="filteredEnrollments.length === 0"
          class="flex flex-col items-center justify-center py-16 text-center"
        >
          <p class="text-sm text-[rgb(var(--color-muted-foreground))]">
            No in-progress courses. Switch to "All Courses" to see completed ones.
          </p>
        </div>

        <!-- Course cards -->
        <div v-else class="space-y-4">
          <router-link
            v-for="enrollment in filteredEnrollments"
            :key="enrollment.id"
            :to="enrollment.status === 'active' ? `/learn/${enrollment.course_id}` : `/courses/${enrollment.course_id}`"
            class="group block overflow-hidden rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] transition-all hover:border-[rgb(var(--color-primary)/0.5)]"
          >
            <div class="flex flex-col sm:flex-row">
              <!-- Thumbnail -->
              <div
                class="relative h-32 sm:h-auto sm:w-48 flex-shrink-0 overflow-hidden"
              >
                <div v-if="courseMap[enrollment.course_id]?.thumbnail_svg" class="w-full h-full" v-html="courseMap[enrollment.course_id]?.thumbnail_svg" />
                <div v-else class="w-full h-full bg-gradient-to-br from-[rgb(var(--color-primary)/0.3)] via-[rgb(var(--color-primary)/0.15)] to-[rgb(var(--color-card))] flex items-center justify-center">
                  <svg
                    class="h-10 w-10 text-[rgb(var(--color-primary)/0.4)]"
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
                    Completed
                  </span>
                </div>
              </div>

              <!-- Content -->
              <div class="flex-1 p-5">
                <div class="flex items-start justify-between gap-4">
                  <div class="min-w-0 flex-1">
                    <h3 class="text-base font-semibold leading-tight">
                      {{ courseMap[enrollment.course_id]?.title ?? enrollment.course_id }}
                    </h3>
                    <p
                      v-if="courseMap[enrollment.course_id]?.description"
                      class="mt-1.5 text-sm text-[rgb(var(--color-muted-foreground))] line-clamp-2"
                    >
                      {{ courseMap[enrollment.course_id]?.description }}
                    </p>
                  </div>
                </div>

                <!-- Progress bar -->
                <div class="mt-4">
                  <div class="flex items-center justify-between text-xs mb-1.5">
                    <span class="text-[rgb(var(--color-muted-foreground))]">Progress</span>
                    <span class="font-medium" :class="enrollment.completed_at ? 'text-green-400' : 'text-[rgb(var(--color-primary))]'">
                      {{ enrollment.completed_at ? '100' : '0' }}%
                    </span>
                  </div>
                  <div class="h-1.5 w-full overflow-hidden rounded-full bg-[rgb(var(--color-muted)/0.3)]">
                    <div
                      class="h-full rounded-full transition-all duration-500"
                      :class="enrollment.completed_at ? 'bg-green-400' : 'bg-[rgb(var(--color-primary))]'"
                      :style="{ width: enrollment.completed_at ? '100%' : '0%' }"
                    />
                  </div>
                </div>

                <!-- Meta info & action -->
                <div class="mt-4 flex flex-wrap items-center justify-between gap-3">
                  <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-[rgb(var(--color-muted-foreground))]">
                    <span>
                      Started {{ formatDate(enrollment.enrolled_at) }}
                    </span>
                    <span v-if="enrollment.updated_at && !enrollment.completed_at">
                      Last accessed {{ formatRelativeTime(enrollment.updated_at) }}
                    </span>
                    <span v-if="enrollment.completed_at">
                      Completed {{ formatDate(enrollment.completed_at) }}
                    </span>
                  </div>
                  <span
                    class="inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium transition-colors"
                    :class="enrollment.completed_at
                      ? 'bg-[rgb(var(--color-muted)/0.3)] text-[rgb(var(--color-muted-foreground))] group-hover:bg-[rgb(var(--color-muted)/0.5)]'
                      : 'bg-[rgb(var(--color-primary)/0.1)] text-[rgb(var(--color-primary))] group-hover:bg-[rgb(var(--color-primary)/0.2)]'"
                  >
                    {{ enrollment.completed_at ? 'Review Course' : 'Continue Learning' }}
                    <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M13.5 4.5L21 12m0 0l-7.5 7.5M21 12H3" />
                    </svg>
                  </span>
                </div>
              </div>
            </div>
          </router-link>
        </div>
      </template>
    </div>
  </div>
</template>
