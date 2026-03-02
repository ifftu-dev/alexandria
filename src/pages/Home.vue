<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAuth } from '@/composables/useAuth'
import { useP2P } from '@/composables/useP2P'
import { StatusBadge } from '@/components/ui'
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
// The delay lets the initial DB queries (list_courses, list_enrollments)
// settle before we hit the keystore/p2p locks.
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
    <div class="mb-8">
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

      <!-- Recommended skeleton -->
      <div class="mb-4 h-5 w-48 animate-pulse rounded bg-muted" />
      <div class="grid gap-5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
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
        <h2 class="text-base font-semibold text-foreground mb-4">Continue Learning</h2>
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
                <div class="absolute bottom-0 left-0 right-0 h-1 bg-muted/50">
                  <div class="h-full bg-primary" style="width: 0%" />
                </div>
              </div>
              <div class="p-3.5">
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
            {{ enrollments.length > 0 ? 'Explore More Courses' : 'Available Courses' }}
          </h2>
          <span v-if="recommendedCourses.length > 0" class="text-xs text-muted-foreground">
            {{ recommendedCourses.length }} course{{ recommendedCourses.length !== 1 ? 's' : '' }}
          </span>
        </div>

        <!-- Empty state -->
        <div
          v-if="courses.length === 0"
          class="rounded-xl bg-card border border-border p-12 text-center"
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

        <!-- Course grid -->
        <div v-else class="grid gap-5 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          <router-link
            v-for="course in (recommendedCourses.length > 0 ? recommendedCourses : courses)"
            :key="course.id"
            :to="`/courses/${course.id}`"
            class="group"
          >
            <div class="card card-interactive overflow-hidden h-full flex flex-col">
              <!-- Thumbnail -->
              <div class="aspect-[16/9] overflow-hidden">
                <div v-if="course.thumbnail_svg" class="w-full h-full" v-html="course.thumbnail_svg" />
                <div v-else class="w-full h-full bg-gradient-to-br from-primary/10 to-accent/5 flex items-center justify-center">
                  <svg class="w-8 h-8 text-primary/30" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
                  </svg>
                </div>
              </div>
              <div class="p-4 flex-1 flex flex-col">
                <!-- Tags -->
                <div v-if="course.tags?.length" class="flex flex-wrap gap-1.5 mb-2">
                  <span
                    v-for="tag in course.tags.slice(0, 3)"
                    :key="tag"
                    class="text-[10px] font-medium px-1.5 py-0.5 rounded bg-muted text-muted-foreground"
                  >
                    {{ tag }}
                  </span>
                </div>
                <!-- Title -->
                <h3 class="text-sm font-medium text-foreground line-clamp-2 group-hover:text-primary transition-colors">
                  {{ course.title }}
                </h3>
                <!-- Description -->
                <p v-if="course.description" class="text-xs text-muted-foreground line-clamp-2 mt-1 flex-1">
                  {{ course.description }}
                </p>
                <!-- Footer -->
                <div class="flex items-center gap-2 mt-3 pt-3 border-t border-border/50">
                  <div v-if="course.author_name" class="flex items-center gap-1.5 flex-1 min-w-0">
                    <div class="w-4 h-4 rounded-full bg-gradient-to-br from-primary to-accent flex items-center justify-center shrink-0">
                      <span class="text-[7px] font-bold text-white">{{ course.author_name.charAt(0) }}</span>
                    </div>
                    <span class="text-[10px] text-muted-foreground truncate">{{ course.author_name }}</span>
                  </div>
                  <StatusBadge :status="course.status" />
                  <span class="text-[10px] text-muted-foreground">v{{ course.version }}</span>
                </div>
              </div>
            </div>
          </router-link>
        </div>
      </section>

      <!-- Quick Actions -->
      <section class="mt-10">
        <h2 class="text-base font-semibold mb-4">Quick Actions</h2>
        <div class="grid grid-cols-2 lg:grid-cols-4 gap-4">
          <router-link
            to="/courses"
            class="card card-interactive p-4 flex items-center gap-3"
          >
            <div class="w-9 h-9 rounded-lg bg-primary/10 flex items-center justify-center shrink-0">
              <svg class="w-4.5 h-4.5 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
              </svg>
            </div>
            <div>
              <div class="text-sm font-medium">Browse Courses</div>
              <div class="text-xs text-muted-foreground">Explore the catalog</div>
            </div>
          </router-link>

          <router-link
            to="/skills"
            class="card card-interactive p-4 flex items-center gap-3"
          >
            <div class="w-9 h-9 rounded-lg bg-success/10 flex items-center justify-center shrink-0">
              <svg class="w-4.5 h-4.5 text-success" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M9 12l2 2 4-4M7.835 4.697a3.42 3.42 0 001.946-.806 3.42 3.42 0 014.438 0 3.42 3.42 0 001.946.806 3.42 3.42 0 013.138 3.138 3.42 3.42 0 00.806 1.946 3.42 3.42 0 010 4.438 3.42 3.42 0 00-.806 1.946 3.42 3.42 0 01-3.138 3.138 3.42 3.42 0 00-1.946.806 3.42 3.42 0 01-4.438 0 3.42 3.42 0 00-1.946-.806 3.42 3.42 0 01-3.138-3.138 3.42 3.42 0 00-.806-1.946 3.42 3.42 0 010-4.438 3.42 3.42 0 00.806-1.946 3.42 3.42 0 013.138-3.138z" />
              </svg>
            </div>
            <div>
              <div class="text-sm font-medium">Skills</div>
              <div class="text-xs text-muted-foreground">Taxonomy & proofs</div>
            </div>
          </router-link>

          <router-link
            to="/governance"
            class="card card-interactive p-4 flex items-center gap-3"
          >
            <div class="w-9 h-9 rounded-lg bg-governance/10 flex items-center justify-center shrink-0">
              <svg class="w-4.5 h-4.5 text-governance" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M3 6l3 1m0 0l-3 9a5.002 5.002 0 006.001 0M6 7l3 9M6 7l6-2m6 2l3-1m-3 1l-3 9a5.002 5.002 0 006.001 0M18 7l3 9m-3-9l-6-2m0-2v2m0 16V5m0 16H9m3 0h3" />
              </svg>
            </div>
            <div>
              <div class="text-sm font-medium">Governance</div>
              <div class="text-xs text-muted-foreground">DAOs & proposals</div>
            </div>
          </router-link>

          <router-link
            to="/dashboard/settings"
            class="card card-interactive p-4 flex items-center gap-3"
          >
            <div class="w-9 h-9 rounded-lg bg-muted flex items-center justify-center shrink-0">
              <svg class="w-4.5 h-4.5 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
              </svg>
            </div>
            <div>
              <div class="text-sm font-medium">Settings</div>
              <div class="text-xs text-muted-foreground">Profile & node config</div>
            </div>
          </router-link>
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.home-greeting {
  font-family: 'Libre Baskerville', Georgia, serif;
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
