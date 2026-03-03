<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAuth } from '@/composables/useAuth'
import { useP2P } from '@/composables/useP2P'
import { StatusBadge } from '@/components/ui'
import CourseCard from '@/components/course/CourseCard.vue'
import type { Course, Enrollment } from '@/types'

const { invoke } = useLocalApi()
const { displayName } = useAuth()
const { status: p2pStatus, start: startP2P, startPolling } = useP2P()

const loading = ref(true)
const enrollments = ref<Enrollment[]>([])
const courses = ref<Course[]>([])
const enrolledCourseMap = ref<Record<string, Course>>({})

// Time-based greeting
const greeting = ref('')
onMounted(() => {
  const hour = new Date().getHours()
  if (hour < 12) greeting.value = 'Good morning'
  else if (hour < 18) greeting.value = 'Good afternoon'
  else greeting.value = 'Good evening'
})

// Start P2P after a short delay so the Home page renders first.
onMounted(() => {
  setTimeout(() => {
    startP2P().catch(() => {})
    startPolling(15000)
  }, 2000)
})

const firstName = computed(() => {
  if (!displayName.value) return ''
  return displayName.value.split(' ')[0] || ''
})

// Recommended courses (non-enrolled)
const enrolledCourseIds = computed(() => new Set(enrollments.value.map(e => e.course_id)))
const recommendedCourses = computed(() =>
  courses.value.filter(c => !enrolledCourseIds.value.has(c.id))
)

onMounted(async () => {
  try {
    const [allCourses, allEnrollments] = await Promise.all([
      invoke<Course[]>('list_courses').catch(() => []),
      invoke<Enrollment[]>('list_enrollments').catch(() => []),
    ])
    courses.value = allCourses
    enrollments.value = allEnrollments

    // Build map of enrolled course IDs to course objects
    for (const enrollment of allEnrollments) {
      const course = allCourses.find(c => c.id === enrollment.course_id)
      if (course) {
        enrolledCourseMap.value[enrollment.course_id] = course
      }
    }
  } catch (e) {
    console.error('Failed to load home data:', e)
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div>
    <!-- Greeting -->
    <div class="mb-6">
      <h1 class="home-greeting">
        {{ greeting }}{{ firstName ? `, ${firstName}` : '' }}
      </h1>
      <p class="mt-1 text-sm text-muted-foreground">
        Your decentralized learning node is {{ p2pStatus?.is_running ? 'online' : p2pStatus != null ? 'offline' : 'starting up' }}.
      </p>
    </div>

    <!-- Loading skeleton -->
    <div v-if="loading">
      <!-- Enrolled skeleton -->
      <div class="mb-10">
        <div class="mb-4 h-5 w-40 animate-pulse rounded bg-muted" />
        <div class="flex gap-4 overflow-hidden">
          <div v-for="i in 3" :key="i" class="w-64 shrink-0 animate-pulse rounded-xl bg-card shadow-sm">
            <div class="aspect-[16/9] bg-muted rounded-t-xl" />
            <div class="p-4">
              <div class="h-4 w-3/4 rounded bg-muted mb-2" />
              <div class="h-3 w-1/2 rounded bg-muted" />
            </div>
          </div>
        </div>
      </div>

      <!-- Recommended skeleton (Mark 2 style: shadow only, no border) -->
      <div class="mb-4 h-5 w-48 animate-pulse rounded bg-muted" />
      <div class="grid gap-6 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
        <div v-for="i in 8" :key="i" class="animate-pulse overflow-hidden rounded-xl bg-card shadow-sm">
          <div class="aspect-[16/9] bg-muted" />
          <div class="p-4 space-y-2">
            <div class="flex gap-2">
              <div class="h-4 w-16 rounded bg-muted" />
              <div class="h-4 w-12 rounded bg-muted" />
            </div>
            <div class="h-5 w-4/5 rounded bg-muted" />
            <div class="h-4 w-full rounded bg-muted" />
            <div class="mt-2 flex items-center gap-2">
              <div class="h-5 w-5 rounded-full bg-muted" />
              <div class="h-3 w-20 rounded bg-muted" />
            </div>
          </div>
        </div>
      </div>
    </div>

    <template v-else>
      <!-- Continue Learning -->
      <section v-if="enrollments.length > 0" class="mb-10">
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-base font-semibold text-foreground">Continue Learning</h2>
        </div>
        <div class="flex gap-4 overflow-x-auto pb-2 scrollbar-thin">
          <router-link
            v-for="enrollment in enrollments"
            :key="enrollment.id"
            :to="`/learn/${enrollment.course_id}`"
            class="w-64 shrink-0 group"
          >
            <div class="card card-interactive overflow-hidden">
              <!-- Thumbnail -->
              <div class="relative aspect-[16/9] overflow-hidden">
                <div v-if="enrolledCourseMap[enrollment.course_id]?.thumbnail_svg" class="w-full h-full" v-html="enrolledCourseMap[enrollment.course_id]?.thumbnail_svg" />
                <div v-else class="w-full h-full bg-gradient-to-br from-primary/15 to-accent/8 flex items-center justify-center">
                  <svg class="w-8 h-8 text-primary/40" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
                  </svg>
                </div>
                <!-- Progress bar overlay at bottom -->
                <div class="absolute bottom-0 left-0 right-0 h-1.5 bg-black/30">
                  <div class="h-full bg-primary" style="width: 0%" />
                </div>
              </div>
              <div class="p-4">
                <h3 class="text-sm font-medium text-foreground truncate group-hover:text-primary transition-colors">
                  {{ enrolledCourseMap[enrollment.course_id]?.title ?? 'Loading...' }}
                </h3>
                <div class="flex items-center gap-2 mt-1.5">
                  <StatusBadge :status="enrollment.status" />
                  <span class="text-xs text-muted-foreground">
                    Enrolled {{ new Date(enrollment.enrolled_at).toLocaleDateString() }}
                  </span>
                </div>
              </div>
            </div>
          </router-link>
        </div>
      </section>

      <!-- Recommended Courses -->
      <section>
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-base font-semibold text-foreground">
            {{ enrollments.length > 0 ? 'Recommended For You' : 'Available Courses' }}
          </h2>
          <span v-if="recommendedCourses.length > 0" class="text-xs text-muted-foreground">
            {{ recommendedCourses.length }} course{{ recommendedCourses.length !== 1 ? 's' : '' }}
          </span>
        </div>

        <!-- Empty state (Mark 2 style: shadow, no border) -->
        <div
          v-if="courses.length === 0"
          class="rounded-xl bg-card p-12 text-center shadow-sm"
        >
          <svg class="mx-auto mb-3 h-10 w-10 text-muted-foreground/30" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
          </svg>
          <p class="text-sm font-medium text-foreground">No courses yet</p>
          <p class="mt-1 text-xs text-muted-foreground">
            Create your first course or discover them from peers.
          </p>
          <router-link
            to="/instructor/courses/new"
            class="inline-flex items-center mt-4 px-4 py-2 text-sm font-medium rounded-lg bg-primary text-white hover:bg-primary-hover transition-colors"
          >
            Create Course
          </router-link>
        </div>

        <!-- Course grid (Mark 2 style: gap-6, 4 columns) -->
        <div v-else class="grid gap-6 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          <CourseCard
            v-for="course in (recommendedCourses.length > 0 ? recommendedCourses : courses)"
            :key="course.id"
            :course="course"
          />
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.home-greeting {
  font-family: 'Libre Baskerville', 'DM Serif Display', Georgia, serif;
  font-size: 1.5rem;
  font-weight: 400;
  line-height: 1.3;
  color: var(--app-foreground);
  letter-spacing: -0.01em;
}

@media (min-width: 640px) {
  .home-greeting {
    font-size: 1.75rem;
  }
}
</style>
