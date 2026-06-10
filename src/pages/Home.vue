<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAuth } from '@/composables/useAuth'
import { useP2P } from '@/composables/useP2P'
import { useContentSync } from '@/composables/useContentSync'
import { usePlatform } from '@/composables/usePlatform'
import { useSettings } from '@/composables/useSettings'
import { useTargets } from '@/composables/useTargets'
import { StatusBadge, AppButton } from '@/components/ui'
import { sanitizeSvg } from '@/utils/sanitize'
import CourseCard from '@/components/course/CourseCard.vue'
import type {
  Course,
  Enrollment,
  FullReputationAssertion,
  LearningPath,
  PublicSkillGraph,
} from '@/types'

const { invoke } = useLocalApi()
const router = useRouter()
const { displayName } = useAuth()
const { status: p2pStatus, start: startP2P, startPolling } = useP2P()
const { startContentSync, completeContentSync, failContentSync } = useContentSync()
const { isMobilePlatform } = usePlatform()
const { targets, pathFor } = useTargets()

const loading = ref(true)
const enrollments = ref<Enrollment[]>([])
const courses = ref<Course[]>([])
const enrolledCourseMap = ref<Record<string, Course>>({})

// ── Reputation + skill graph + targets (Option C cockpit) ──────────
const reputation = ref<FullReputationAssertion[]>([])
const myGraph = ref<PublicSkillGraph | null>(null)
const targetPaths = ref<Record<string, LearningPath>>({})
const graphExpanded = ref(false)

const teachingImpact = computed(() =>
  Math.round(reputation.value.filter((a) => a.role === 'instructor').reduce((s, a) => s + a.score, 0)),
)
const learningImpact = computed(() =>
  Math.round(reputation.value.filter((a) => a.role === 'learner').reduce((s, a) => s + a.score, 0)),
)
const avgConfidence = computed(() => {
  if (reputation.value.length === 0) return 0
  return Math.round(
    (reputation.value.reduce((s, a) => s + a.confidence, 0) / reputation.value.length) * 100,
  )
})
const skillsProven = computed(() => myGraph.value?.nodes.length ?? 0)

const ringDash = 2 * Math.PI * 18
function pathPct(p: LearningPath | undefined): number {
  if (!p || p.total === 0) return 0
  return Math.round((p.earned_count / p.total) * 100)
}
function pathNext(p: LearningPath | undefined): string | null {
  return p?.steps.find((s) => s.status === 'available')?.name ?? null
}

async function loadCockpit() {
  await useSettings().initialize()
  const [rep, graph] = await Promise.all([
    invoke<FullReputationAssertion[]>('get_reputation', { query: {} }).catch(() => []),
    invoke<PublicSkillGraph>('get_my_skill_graph').catch(() => null),
  ])
  reputation.value = rep
  myGraph.value = graph
  const entries = await Promise.all(
    targets.value.map(async (t) => [t.id, await pathFor(t).catch(() => null)] as const),
  )
  const map: Record<string, LearningPath> = {}
  for (const [id, p] of entries) if (p) map[id] = p
  targetPaths.value = map
}

// Diagnostic log viewer (for iOS debugging)
const showDiag = ref(false)
const diagLog = ref<string | null>(null)
async function readDiagLog() {
  try {
    diagLog.value = await invoke<string>('read_diag_log')
  } catch (e) {
    diagLog.value = `ERROR: ${e}`
  }
  showDiag.value = true
}

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

// Load the reputation / skill-graph / targets cockpit (non-blocking).
onMounted(() => {
  loadCockpit().catch(() => {})
})

const firstName = computed(() => {
  if (!displayName.value) return ''
  return displayName.value.split(' ')[0] || ''
})

// Split non-enrolled courses into tutorials and full courses
const enrolledCourseIds = computed(() => new Set(enrollments.value.map(e => e.course_id)))
const recommendedCourses = computed(() =>
  courses.value.filter(c => !enrolledCourseIds.value.has(c.id) && c.kind !== 'tutorial')
)
const tutorials = computed(() =>
  courses.value.filter(c => c.kind === 'tutorial')
)

onMounted(async () => {
  try {
    const syncStartedAt = performance.now()
    startContentSync()

    const coursesBeforeSync = await invoke<Course[]>('list_courses').catch(() => [])
    const beforeCount = coursesBeforeSync.length

    const bootstrapped = await invoke<number>('bootstrap_public_catalog').catch((e) => {
      console.warn('Public catalog bootstrap skipped on Home:', e)
      return 0
    })

    const hydrated = await invoke<number>('hydrate_catalog_courses', { limit: 200 }).catch((e) => {
      console.warn('Catalog hydration skipped on Home:', e)
      return 0
    })

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

    completeContentSync({
      bootstrapped,
      hydrated,
      beforeCourses: beforeCount,
      afterCourses: allCourses.length,
      durationMs: Math.round(performance.now() - syncStartedAt),
    })
  } catch (e) {
    console.error('Failed to load home data:', e)
    failContentSync(String(e))
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

    <!-- ═══ Reputation stat band ═══ -->
    <section class="mb-6">
      <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <button class="stat-card" @click="router.push('/dashboard/reputation')">
          <span class="stat-label">Teaching impact</span>
          <span class="stat-value">{{ teachingImpact }}</span>
        </button>
        <button class="stat-card" @click="router.push('/dashboard/reputation')">
          <span class="stat-label">Learning impact</span>
          <span class="stat-value">{{ learningImpact }}</span>
        </button>
        <button class="stat-card" @click="router.push('/skills')">
          <span class="stat-label">Skills proven</span>
          <span class="stat-value">{{ skillsProven }}</span>
        </button>
        <button class="stat-card" @click="router.push('/dashboard/reputation')">
          <span class="stat-label">Confidence</span>
          <span class="stat-value">{{ avgConfidence }}%</span>
        </button>
      </div>
    </section>

    <!-- ═══ Targets rail ═══ -->
    <section class="mb-8">
      <div class="mb-3 flex items-center justify-between">
        <div class="flex items-center gap-2">
          <h2 class="text-base font-semibold text-foreground">Your targets</h2>
          <span v-if="targets.length" class="text-xs text-muted-foreground">{{ targets.length }}</span>
        </div>
        <button class="sb-view-all text-xs text-primary hover:underline" @click="router.push('/targets')">
          View all
        </button>
      </div>

      <div class="-mx-4 flex gap-4 overflow-x-auto px-4 pb-2 scrollbar-thin sm:mx-0 sm:px-0">
        <!-- target cards -->
        <button
          v-for="t in targets"
          :key="t.id"
          class="target-card group"
          @click="router.push('/targets')"
        >
          <svg width="46" height="46" viewBox="0 0 46 46" class="shrink-0">
            <circle cx="23" cy="23" r="18" fill="none" stroke="var(--app-border)" stroke-width="4" />
            <circle
              cx="23" cy="23" r="18" fill="none" stroke="var(--app-primary)" stroke-width="4"
              stroke-linecap="round" :stroke-dasharray="ringDash"
              :stroke-dashoffset="ringDash * (1 - pathPct(targetPaths[t.id]) / 100)"
              transform="rotate(-90 23 23)" class="transition-all duration-500"
            />
            <text x="23" y="23" text-anchor="middle" dominant-baseline="central"
              font-size="11" font-weight="600" fill="var(--app-foreground)">
              {{ pathPct(targetPaths[t.id]) }}%
            </text>
          </svg>
          <div class="min-w-0 text-left">
            <p class="truncate text-sm font-medium text-foreground group-hover:text-primary">
              {{ t.label }}
            </p>
            <p v-if="pathNext(targetPaths[t.id])" class="mt-0.5 truncate text-xs text-muted-foreground">
              Next: {{ pathNext(targetPaths[t.id]) }}
            </p>
            <p v-else class="mt-0.5 truncate text-xs text-success">Prereqs cleared 🎉</p>
          </div>
        </button>

        <!-- add target -->
        <button class="target-card target-card--add" @click="router.push('/skills')">
          <span class="text-2xl leading-none text-muted-foreground">+</span>
          <span class="text-sm font-medium text-muted-foreground">Add a target</span>
        </button>
      </div>
    </section>

    <!-- ═══ Skill graph summary (collapsible) ═══ -->
    <section class="mb-8">
      <div class="card overflow-hidden">
        <button
          class="flex w-full items-center justify-between p-4 text-left"
          @click="graphExpanded = !graphExpanded"
        >
          <div class="flex items-center gap-2">
            <span class="text-base font-semibold text-foreground">Your skill graph</span>
            <span class="text-xs text-muted-foreground">
              {{ skillsProven }} skill{{ skillsProven === 1 ? '' : 's' }} proven
            </span>
          </div>
          <span class="text-xs text-muted-foreground">{{ graphExpanded ? 'Hide ▴' : 'Expand ▾' }}</span>
        </button>
        <div v-if="graphExpanded" class="border-t border-border p-4">
          <div v-if="skillsProven === 0" class="text-sm text-muted-foreground">
            No proven skills yet. Earn credentials by completing courses, then they'll appear here.
          </div>
          <div v-else class="flex flex-wrap gap-2">
            <button
              v-for="n in myGraph?.nodes ?? []"
              :key="n.id"
              class="graph-chip"
              :class="{ 'graph-chip--teaching': n.teaching, 'graph-chip--private': !n.public }"
              @click="router.push(`/skills/${n.id}`)"
            >
              {{ n.name }}
            </button>
          </div>
          <div class="mt-4">
            <AppButton variant="outline" size="sm" @click="router.push('/skills')">
              Manage visibility & teaching
            </AppButton>
          </div>
        </div>
      </div>
    </section>

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

      <!-- Recommended skeleton (shadow only, no border) -->
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
        <div class="-mx-4 flex gap-5 snap-x snap-mandatory overflow-x-auto px-4 pb-2 scrollbar-thin sm:mx-0 sm:px-0">
          <router-link
            v-for="enrollment in enrollments"
            :key="enrollment.id"
            :to="`/learn/${enrollment.course_id}`"
            class="w-64 shrink-0 snap-start group"
          >
            <div class="card card-interactive overflow-hidden">
              <!-- Thumbnail -->
              <div class="relative aspect-[16/9] overflow-hidden">
                <div v-if="enrolledCourseMap[enrollment.course_id]?.thumbnail_svg" class="w-full h-full" v-html="sanitizeSvg(enrolledCourseMap[enrollment.course_id]?.thumbnail_svg ?? '')" />
                <div v-else class="w-full h-full bg-gradient-to-br from-primary/15 to-accent/8 flex items-center justify-center">
                  <svg class="w-8 h-8 text-primary/40" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
                  </svg>
                </div>
                <!-- Content-type pill -->
                <div class="absolute top-2 left-2">
                  <span v-if="enrolledCourseMap[enrollment.course_id]?.kind === 'tutorial'" class="inline-flex items-center gap-1 rounded-full bg-[color-mix(in_srgb,var(--app-primary)_85%,black)] px-2 py-0.5 text-[10px] font-semibold text-white shadow">
                    <svg class="h-3 w-3" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
                    Tutorial
                  </span>
                  <span v-else class="inline-flex items-center gap-1 rounded-full bg-[color-mix(in_srgb,var(--app-success)_80%,black)] px-2 py-0.5 text-[10px] font-semibold text-white shadow">
                    <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
                    </svg>
                    Course
                  </span>
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

      <!-- Quick Tutorials -->
      <section v-if="tutorials.length > 0" class="mb-10">
        <div class="flex items-center justify-between mb-4">
          <div class="flex items-center gap-2">
            <svg class="h-5 w-5 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z" />
              <path stroke-linecap="round" stroke-linejoin="round" d="M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <h2 class="text-base font-semibold text-foreground">Quick Tutorials</h2>
          </div>
          <span class="text-xs text-muted-foreground">
            {{ tutorials.length }} tutorial{{ tutorials.length !== 1 ? 's' : '' }}
          </span>
        </div>
        <div class="-mx-4 flex gap-5 snap-x snap-mandatory overflow-x-auto px-4 pb-2 scrollbar-thin sm:mx-0 sm:px-0">
          <router-link
            v-for="tut in tutorials"
            :key="tut.id"
            :to="`/learn/${tut.id}`"
            class="w-72 shrink-0 snap-start group"
          >
            <div class="card card-interactive overflow-hidden rounded-xl border border-primary/15">
              <!-- Thumbnail -->
              <div class="relative aspect-[2/1] overflow-hidden bg-gradient-to-br from-primary/20 via-accent/10 to-primary/5">
                <div v-if="tut.thumbnail_svg" class="w-full h-full" v-html="sanitizeSvg(tut.thumbnail_svg)" />
                <div v-else class="w-full h-full flex items-center justify-center">
                  <svg class="w-12 h-12 text-primary/30" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664z" />
                    <path stroke-linecap="round" stroke-linejoin="round" d="M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                  </svg>
                </div>
                <!-- Play badge -->
                <div class="absolute top-2 left-2 flex items-center gap-1 rounded-full bg-primary/90 px-2 py-0.5 text-[10px] font-semibold text-white shadow">
                  <svg class="h-3 w-3" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
                  Tutorial
                </div>
              </div>
              <div class="p-3">
                <h3 class="text-sm font-medium text-foreground truncate group-hover:text-primary transition-colors">
                  {{ tut.title }}
                </h3>
                <p v-if="tut.description" class="mt-0.5 text-xs text-muted-foreground line-clamp-1">
                  {{ tut.description }}
                </p>
              </div>
            </div>
          </router-link>
        </div>
      </section>

      <!-- Courses -->
      <section>
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-base font-semibold text-foreground">
            {{ enrollments.length > 0 ? 'Recommended For You' : 'Courses' }}
          </h2>
          <span v-if="recommendedCourses.length > 0" class="text-xs text-muted-foreground">
            {{ recommendedCourses.length }} course{{ recommendedCourses.length !== 1 ? 's' : '' }}
          </span>
        </div>

        <!-- Empty state (shadow, no border) -->
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

        <!-- Course grid (gap-6, 4 columns) -->
        <div v-else class="grid gap-6 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          <CourseCard
            v-for="course in (recommendedCourses.length > 0 ? recommendedCourses : courses.filter(c => c.kind !== 'tutorial'))"
            :key="course.id"
            :course="course"
          />
        </div>
      </section>
    </template>

    <!-- Floating diagnostic button (mobile only, for iOS freeze debugging) -->
    <button
      v-if="isMobilePlatform"
      class="fixed bottom-20 right-3 z-50 flex h-8 w-8 items-center justify-center rounded-full bg-destructive/80 text-white shadow-lg text-xs font-bold"
      @click="readDiagLog"
      title="Read diag.log"
    >
      D
    </button>

    <!-- Diagnostic overlay -->
    <Teleport to="body">
      <div v-if="showDiag" class="fixed inset-0 z-[100] bg-black/80 p-4 overflow-y-auto" @click.self="showDiag = false">
        <div class="bg-card rounded-xl p-4 max-w-lg mx-auto mt-12">
          <div class="flex items-center justify-between mb-2">
            <h3 class="text-sm font-semibold text-foreground">diag.log</h3>
            <button class="text-xs text-muted-foreground" @click="showDiag = false">Close</button>
          </div>
          <pre class="text-[0.55rem] text-muted-foreground whitespace-pre-wrap leading-tight max-h-[70vh] overflow-y-auto">{{ diagLog ?? 'Loading...' }}</pre>
        </div>
      </div>
    </Teleport>
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

/* Reputation stat band */
.stat-card {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  padding: 0.75rem 0.9rem;
  border-radius: 0.75rem;
  background: var(--app-card);
  box-shadow: 0 1px 2px rgb(0 0 0 / 5%);
  text-align: left;
  transition:
    box-shadow 0.15s,
    transform 0.15s;
}
.stat-card:hover {
  box-shadow: 0 4px 12px rgb(0 0 0 / 8%);
  transform: translateY(-1px);
}
.stat-label {
  font-size: 0.7rem;
  color: var(--app-muted-foreground);
}
.stat-value {
  font-size: 1.35rem;
  font-weight: 600;
  color: var(--app-foreground);
  line-height: 1.1;
}

/* Targets rail */
.target-card {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  width: 16rem;
  flex-shrink: 0;
  padding: 0.85rem;
  border-radius: 0.85rem;
  background: var(--app-card);
  box-shadow: 0 1px 2px rgb(0 0 0 / 5%);
  transition:
    box-shadow 0.15s,
    transform 0.15s;
}
.target-card:hover {
  box-shadow: 0 4px 12px rgb(0 0 0 / 8%);
  transform: translateY(-1px);
}
.target-card--add {
  justify-content: center;
  border: 1px dashed var(--app-border);
  background: transparent;
  box-shadow: none;
}

/* Skill graph chips */
.graph-chip {
  font-size: 0.75rem;
  padding: 0.2rem 0.6rem;
  border-radius: 999px;
  background: var(--app-muted);
  color: var(--app-foreground);
  transition: background 0.15s;
}
.graph-chip:hover {
  background: color-mix(in srgb, var(--app-primary) 16%, var(--app-muted));
}
.graph-chip--teaching {
  background: color-mix(in srgb, var(--app-primary) 85%, black);
  color: white;
}
.graph-chip--private {
  opacity: 0.55;
  border: 1px dashed var(--app-border);
}

</style>
