<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'

const { t } = useI18n()
import { useAuth } from '@/composables/useAuth'
import { useP2P } from '@/composables/useP2P'
import { useContentSync } from '@/composables/useContentSync'
import { usePlatform } from '@/composables/usePlatform'
import { useSettings } from '@/composables/useSettings'
import { useGoals } from '@/composables/useGoals'
import { StatusBadge, AppButton, InfoTip } from '@/components/ui'
import { bloomFill } from '@/utils/bloom'
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
const { goals, pathFor } = useGoals()

const loading = ref(true)
const enrollments = ref<Enrollment[]>([])
const courses = ref<Course[]>([])
const enrolledCourseMap = ref<Record<string, Course>>({})

// ── Reputation + skill graph + goals (Option C cockpit) ──────────
const reputation = ref<FullReputationAssertion[]>([])
const usernameConflict = ref<{ username: string; winner_did: string } | null>(null)
const myGraph = ref<PublicSkillGraph | null>(null)
const goalPaths = ref<Record<string, LearningPath>>({})
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
  usernameConflict.value = await invoke<{ username: string; winner_did: string } | null>(
    'check_my_username_conflict',
  ).catch(() => null)
  const entries = await Promise.all(
    goals.value.map(async (t) => [t.id, await pathFor(t).catch(() => null)] as const),
  )
  const map: Record<string, LearningPath> = {}
  for (const [id, p] of entries) if (p) map[id] = p
  goalPaths.value = map
}

// Diagnostic log viewer (for iOS debugging). Dev-only — never shown in
// production/alpha builds.
const isDev = import.meta.env.DEV
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
  if (hour < 12) greeting.value = t('dashboard.home.greeting.morning')
  else if (hour < 18) greeting.value = t('dashboard.home.greeting.afternoon')
  else greeting.value = t('dashboard.home.greeting.evening')
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

// Open a content author's public instructor profile. Cards are router-links,
// so callers use @click.stop.prevent to suppress the card's own navigation.
function openInstructor(address: string | null | undefined) {
  if (address) router.push(`/u/${address}`)
}

// Single highest-value next action, surfaced as the dashboard hero. Priority:
// resume an in-progress course → set a first target → start a first course →
// browse. Keeps the landing screen action-oriented rather than passive.
interface HeroAction {
  eyebrow: string
  title: string
  cta: string
  to: string
}
const heroAction = computed<HeroAction>(() => {
  const enrolled = enrollments.value[0]
  if (enrolled) {
    return {
      eyebrow: t('dashboard.home.hero.resumeEyebrow'),
      title: enrolledCourseMap.value[enrolled.course_id]?.title ?? t('dashboard.home.hero.resumeTitleFallback'),
      cta: t('dashboard.home.hero.resumeCta'),
      to: `/learn/${enrolled.course_id}`,
    }
  }
  if (goals.value.length === 0) {
    return {
      eyebrow: t('dashboard.home.hero.startEyebrow'),
      title: t('dashboard.home.hero.startTitle'),
      cta: t('dashboard.home.hero.startCta'),
      to: '/skills',
    }
  }
  const next = recommendedCourses.value[0]
  if (next) {
    return {
      eyebrow: t('dashboard.home.hero.recommendedEyebrow'),
      title: next.title,
      cta: t('dashboard.home.hero.recommendedCta'),
      to: `/courses/${next.id}`,
    }
  }
  return {
    eyebrow: t('dashboard.home.hero.exploreEyebrow'),
    title: t('dashboard.home.hero.exploreTitle'),
    cta: t('dashboard.home.hero.exploreCta'),
    to: '/courses',
  }
})

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
        {{ p2pStatus?.is_running ? t('network.appOnline') : p2pStatus != null ? t('network.appOffline') : t('network.appStarting') }}
      </p>
    </div>

    <!-- Username conflict banner (deterministic registry loser) -->
    <div
      v-if="usernameConflict"
      class="mb-6 rounded-xl border border-warning/40 bg-warning/10 px-4 py-3 text-sm"
    >
      <span class="font-semibold text-foreground">{{ $t('dashboard.home.conflict.held', { name: usernameConflict.username }) }}</span>
      <span class="text-muted-foreground">
        {{ $t('dashboard.home.conflict.explain') }}
      </span>
      <button class="ms-1 font-medium text-primary hover:underline" @click="router.push('/settings/account')">
        {{ $t('dashboard.home.conflict.pick') }}
      </button>
    </div>

    <!-- ═══ Hero: single highest-value next action ═══ -->
    <button
      v-if="!loading"
      class="home-hero group mb-6"
      @click="router.push(heroAction.to)"
    >
      <div class="min-w-0 text-start">
        <p class="home-hero-eyebrow">{{ heroAction.eyebrow }}</p>
        <p class="home-hero-title">{{ heroAction.title }}</p>
      </div>
      <span class="home-hero-cta">
        {{ heroAction.cta }}
        <svg class="h-4 w-4 transition-transform group-hover:translate-x-0.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M5 12h14m0 0l-6-6m6 6l-6 6" />
        </svg>
      </span>
    </button>

    <!-- ═══ Reputation stat band ═══ -->
    <section class="mb-6">
      <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <div class="relative">
          <button class="stat-card w-full" @click="router.push('/reputation')">
            <span class="stat-head">
              <span class="stat-icon stat-icon--teaching">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9"><path stroke-linecap="round" stroke-linejoin="round" d="M12 14l9-5-9-5-9 5 9 5z"/><path stroke-linecap="round" stroke-linejoin="round" d="M12 14l6.16-3.42A12 12 0 0112 21a12 12 0 01-6.16-10.42L12 14z"/></svg>
              </span>
              <span class="stat-label">{{ $t('dashboard.home.stats.teaching') }}</span>
            </span>
            <span class="stat-value">{{ teachingImpact }}</span>
          </button>
          <InfoTip
            class="absolute end-2 top-2"
            :label="$t('dashboard.home.stats.teachingTipLabel')"
            :text="$t('dashboard.home.stats.teachingTipText')"
          />
        </div>
        <div class="relative">
          <button class="stat-card w-full" @click="router.push('/reputation')">
            <span class="stat-head">
              <span class="stat-icon stat-icon--learning">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9"><path stroke-linecap="round" stroke-linejoin="round" d="M12 6.5C10.5 5.5 8.5 5 6.5 5 5.5 5 4.7 5.1 4 5.3v12.4c.7-.2 1.5-.3 2.5-.3 2 0 4 .5 5.5 1.5m0-13.4c1.5-1 3.5-1.5 5.5-1.5 1 0 1.8.1 2.5.3v12.4c-.7-.2-1.5-.3-2.5-.3-2 0-4 .5-5.5 1.5m0-13.4V19.9"/></svg>
              </span>
              <span class="stat-label">{{ $t('dashboard.home.stats.learning') }}</span>
            </span>
            <span class="stat-value">{{ learningImpact }}</span>
          </button>
          <InfoTip
            class="absolute end-2 top-2"
            :label="$t('dashboard.home.stats.learningTipLabel')"
            :text="$t('dashboard.home.stats.learningTipText')"
          />
        </div>
        <div class="relative">
          <button class="stat-card w-full" @click="router.push('/skills')">
            <span class="stat-head">
              <span class="stat-icon stat-icon--skills">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9"><path stroke-linecap="round" stroke-linejoin="round" d="M9 12.5l2 2 4-4.5M12 3l2.09 1.26 2.43-.1.99 2.22 1.99 1.4-.55 2.37.55 2.37-1.99 1.4-.99 2.22-2.43-.1L12 21l-2.09-1.26-2.43.1-.99-2.22-1.99-1.4.55-2.37L4.5 10l1.99-1.4.99-2.22 2.43.1L12 3z"/></svg>
              </span>
              <span class="stat-label">{{ $t('dashboard.home.stats.skills') }}</span>
            </span>
            <span class="stat-value">{{ skillsProven }}</span>
          </button>
          <InfoTip
            class="absolute end-2 top-2"
            :label="$t('dashboard.home.stats.skillsTipLabel')"
            :text="$t('dashboard.home.stats.skillsTipText')"
          />
        </div>
        <div class="relative">
          <button class="stat-card w-full" @click="router.push('/reputation')">
            <span class="stat-head">
              <span class="stat-icon stat-icon--confidence">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9"><path stroke-linecap="round" stroke-linejoin="round" d="M12 3l7 3v5c0 4.5-3 8.5-7 10-4-1.5-7-5.5-7-10V6l7-3z"/><path stroke-linecap="round" stroke-linejoin="round" d="M9.5 12l1.8 1.8 3.2-3.6"/></svg>
              </span>
              <span class="stat-label">{{ $t('dashboard.home.stats.confidence') }}</span>
            </span>
            <span class="stat-value">{{ avgConfidence }}%</span>
          </button>
          <InfoTip
            class="absolute end-2 top-2"
            :label="$t('dashboard.home.stats.confidenceTipLabel')"
            :text="$t('dashboard.home.stats.confidenceTipText')"
          />
        </div>
      </div>
    </section>

    <!-- ═══ Goals rail ═══ -->
    <section class="mb-8">
      <div class="mb-3 flex items-center justify-between">
        <div class="flex items-center gap-2">
          <h2 class="text-base font-semibold text-foreground">{{ $t('dashboard.home.goals.title') }}</h2>
          <span v-if="goals.length" class="text-xs text-muted-foreground">{{ goals.length }}</span>
        </div>
        <button class="sb-view-all text-xs text-primary hover:underline" @click="router.push('/goals')">
          {{ $t('dashboard.home.goals.viewAll') }}
        </button>
      </div>

      <div class="-mx-4 flex gap-4 overflow-x-auto px-4 pb-2 scrollbar-thin sm:mx-0 sm:px-0">
        <!-- goal cards -->
        <button
          v-for="t in goals"
          :key="t.id"
          class="goal-card group"
          @click="router.push('/goals')"
        >
          <svg width="46" height="46" viewBox="0 0 46 46" class="shrink-0">
            <circle cx="23" cy="23" r="18" fill="none" stroke="var(--app-border)" stroke-width="4" />
            <circle
              cx="23" cy="23" r="18" fill="none" stroke="var(--app-primary)" stroke-width="4"
              stroke-linecap="round" :stroke-dasharray="ringDash"
              :stroke-dashoffset="ringDash * (1 - pathPct(goalPaths[t.id]) / 100)"
              transform="rotate(-90 23 23)" class="transition-all duration-500"
            />
            <text x="23" y="23" text-anchor="middle" dominant-baseline="central"
              font-size="11" font-weight="600" fill="var(--app-foreground)">
              {{ pathPct(goalPaths[t.id]) }}%
            </text>
          </svg>
          <div class="min-w-0 text-start">
            <p class="truncate text-sm font-medium text-foreground group-hover:text-primary">
              {{ t.label }}
            </p>
            <p v-if="pathNext(goalPaths[t.id])" class="mt-0.5 truncate text-xs text-muted-foreground">
              {{ $t('dashboard.home.goals.next', { name: pathNext(goalPaths[t.id]) }) }}
            </p>
            <p v-else class="mt-0.5 truncate text-xs text-success">{{ $t('dashboard.home.goals.ready') }}</p>
          </div>
        </button>

        <!-- add goal -->
        <button class="goal-card goal-card--add" @click="router.push('/skills')">
          <span class="text-2xl leading-none text-muted-foreground">+</span>
          <span class="text-sm font-medium text-muted-foreground">{{ $t('dashboard.home.goals.add') }}</span>
        </button>

        <!-- trailing gap: WebKit drops a scroll container's right padding -->
        <div class="w-4 shrink-0 sm:hidden" aria-hidden="true" />
      </div>
    </section>

    <!-- ═══ Skill graph summary (collapsible) ═══ -->
    <section class="mb-8">
      <div class="card overflow-hidden">
        <button
          class="flex w-full items-center justify-between p-4 text-start"
          @click="graphExpanded = !graphExpanded"
        >
          <div class="flex items-center gap-2">
            <span class="text-base font-semibold text-foreground">{{ $t('dashboard.home.graph.title') }}</span>
            <span class="text-xs text-muted-foreground">
              {{ $t('dashboard.home.graph.proven', { count: skillsProven }, skillsProven) }}
            </span>
          </div>
          <span class="text-xs text-muted-foreground">{{ graphExpanded ? $t('dashboard.home.graph.hide') : $t('dashboard.home.graph.expand') }}</span>
        </button>
        <div v-if="graphExpanded" class="border-t border-border p-4">
          <div v-if="skillsProven === 0" class="text-sm text-muted-foreground">
            {{ $t('dashboard.home.graph.empty') }}
          </div>
          <div v-else class="flex flex-wrap gap-2">
            <button
              v-for="n in myGraph?.nodes ?? []"
              :key="n.id"
              class="graph-chip"
              :class="{ 'graph-chip--teaching': n.teaching, 'graph-chip--private': !n.public }"
              :title="$t('dashboard.home.graph.levelTitle', { level: n.bloom_level })"
              @click="router.push(`/skills/${n.id}`)"
            >
              <span class="graph-chip-dot" :style="{ background: bloomFill(n.bloom_level) }" />
              {{ n.name }}
            </button>
          </div>
          <div class="mt-4">
            <AppButton variant="outline" size="sm" @click="router.push('/skills')">
              {{ $t('dashboard.home.graph.manage') }}
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
          <h2 class="text-base font-semibold text-foreground">{{ $t('dashboard.home.continue') }}</h2>
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
                <div class="absolute top-2 start-2">
                  <span v-if="enrolledCourseMap[enrollment.course_id]?.kind === 'tutorial'" class="inline-flex items-center gap-1 rounded-full bg-[color-mix(in_srgb,var(--app-primary)_85%,black)] px-2 py-0.5 text-[10px] font-semibold text-white shadow">
                    <svg class="h-3 w-3" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
                    {{ $t('dashboard.home.pill.tutorial') }}
                  </span>
                  <span v-else class="inline-flex items-center gap-1 rounded-full bg-[color-mix(in_srgb,var(--app-success)_80%,black)] px-2 py-0.5 text-[10px] font-semibold text-white shadow">
                    <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
                    </svg>
                    {{ $t('dashboard.home.pill.course') }}
                  </span>
                </div>
                <!-- Progress bar overlay at bottom -->
                <div class="absolute bottom-0 start-0 end-0 h-1.5 bg-black/30">
                  <div class="h-full bg-primary" style="width: 0%" />
                </div>
              </div>
              <div class="p-4">
                <h3 class="text-sm font-medium text-foreground truncate group-hover:text-primary transition-colors">
                  {{ enrolledCourseMap[enrollment.course_id]?.title ?? $t('common.actions.loading') }}
                </h3>
                <div class="flex items-center gap-2 mt-1.5">
                  <StatusBadge :status="enrollment.status" />
                  <span class="text-xs text-muted-foreground">
                    {{ $t('dashboard.home.enrolledOn', { date: new Date(enrollment.enrolled_at).toLocaleDateString() }) }}
                  </span>
                </div>
              </div>
            </div>
          </router-link>

          <!-- trailing gap: WebKit drops a scroll container's right padding -->
          <div class="w-4 shrink-0 sm:hidden" aria-hidden="true" />
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
            <h2 class="text-base font-semibold text-foreground">{{ $t('dashboard.home.tutorials.title') }}</h2>
          </div>
          <span class="text-xs text-muted-foreground">
            {{ $t('dashboard.home.tutorials.count', { count: tutorials.length }, tutorials.length) }}
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
                <div class="absolute top-2 start-2 flex items-center gap-1 rounded-full bg-primary/90 px-2 py-0.5 text-[10px] font-semibold text-white shadow">
                  <svg class="h-3 w-3" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
                  {{ $t('dashboard.home.pill.tutorial') }}
                </div>
              </div>
              <div class="p-3">
                <h3 class="text-sm font-medium text-foreground truncate group-hover:text-primary transition-colors">
                  {{ tut.title }}
                </h3>
                <p v-if="tut.description" class="mt-0.5 text-xs text-muted-foreground line-clamp-1">
                  {{ tut.description }}
                </p>
                <button
                  v-if="tut.author_address"
                  type="button"
                  class="mt-1 block max-w-full truncate text-start text-xs text-muted-foreground hover:text-primary hover:underline"
                  @click.stop.prevent="openInstructor(tut.author_address)"
                >
                  {{ tut.author_name || tut.author_address.slice(0, 16) + '…' }}
                </button>
              </div>
            </div>
          </router-link>

          <!-- trailing gap: WebKit drops a scroll container's right padding -->
          <div class="w-4 shrink-0 sm:hidden" aria-hidden="true" />
        </div>
      </section>

      <!-- Courses -->
      <section>
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-base font-semibold text-foreground">
            {{ enrollments.length > 0 ? $t('dashboard.home.courses.recommendedTitle') : $t('dashboard.home.courses.title') }}
          </h2>
          <span v-if="recommendedCourses.length > 0" class="text-xs text-muted-foreground">
            {{ $t('dashboard.home.courses.count', { count: recommendedCourses.length }, recommendedCourses.length) }}
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
          <p class="text-sm font-medium text-foreground">{{ $t('dashboard.home.courses.emptyTitle') }}</p>
          <p class="mt-1 text-xs text-muted-foreground">
            {{ $t('dashboard.home.courses.emptyBody') }}
          </p>
          <router-link
            to="/instructor/courses/new"
            class="inline-flex items-center mt-4 px-4 py-2 text-sm font-medium rounded-lg bg-primary text-white hover:bg-primary-hover transition-colors"
          >
            {{ $t('dashboard.home.courses.create') }}
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
      v-if="isMobilePlatform && isDev"
      class="fixed bottom-20 end-3 z-50 flex h-8 w-8 items-center justify-center rounded-full bg-destructive/80 text-white shadow-lg text-xs font-bold"
      @click="readDiagLog"
      :title="$t('dashboard.home.diag.read')"
    >
      D
    </button>

    <!-- Diagnostic overlay -->
    <Teleport to="body">
      <div v-if="showDiag" class="fixed inset-0 z-[100] bg-black/80 p-4 overflow-y-auto" @click.self="showDiag = false">
        <div class="bg-card rounded-xl p-4 max-w-lg mx-auto mt-12">
          <div class="flex items-center justify-between mb-2">
            <h3 class="text-sm font-semibold text-foreground">diag.log</h3>
            <button class="text-xs text-muted-foreground" @click="showDiag = false">{{ $t('common.actions.close') }}</button>
          </div>
          <pre class="text-[0.55rem] text-muted-foreground whitespace-pre-wrap leading-tight max-h-[70vh] overflow-y-auto">{{ diagLog ?? $t('common.actions.loading') }}</pre>
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

/* Hero: highest-value next action */
.home-hero {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  width: 100%;
  padding: 1.1rem 1.25rem;
  border-radius: 1rem;
  text-align: left;
  color: white;
  background:
    radial-gradient(120% 140% at 0% 0%, color-mix(in srgb, var(--app-accent) 65%, transparent), transparent 60%),
    linear-gradient(120deg, color-mix(in srgb, var(--app-primary) 92%, black), color-mix(in srgb, var(--app-primary) 70%, black));
  box-shadow: 0 8px 24px -10px color-mix(in srgb, var(--app-primary) 60%, transparent);
  transition: box-shadow 0.15s, transform 0.15s;
}
.home-hero:hover {
  transform: translateY(-1px);
  box-shadow: 0 12px 30px -10px color-mix(in srgb, var(--app-primary) 70%, transparent);
}
.home-hero-eyebrow {
  font-size: 0.7rem;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: color-mix(in srgb, white 80%, transparent);
}
.home-hero-title {
  margin-top: 0.15rem;
  font-size: 1.1rem;
  font-weight: 600;
  line-height: 1.25;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.home-hero-cta {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  flex-shrink: 0;
  padding: 0.5rem 0.9rem;
  border-radius: 0.625rem;
  font-size: 0.85rem;
  font-weight: 600;
  background: rgb(255 255 255 / 0.16);
}

/* Reputation stat band */
.stat-card {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
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
.stat-head {
  display: flex;
  align-items: center;
  gap: 0.45rem;
}
.stat-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 1.5rem;
  height: 1.5rem;
  flex-shrink: 0;
  border-radius: 0.5rem;
}
.stat-icon svg {
  width: 0.9rem;
  height: 0.9rem;
}
.stat-icon--teaching {
  background: color-mix(in srgb, var(--app-primary) 15%, transparent);
  color: var(--app-primary);
}
.stat-icon--learning {
  background: color-mix(in srgb, var(--app-accent) 15%, transparent);
  color: var(--app-accent);
}
.stat-icon--skills {
  background: color-mix(in srgb, var(--app-success) 15%, transparent);
  color: var(--app-success);
}
.stat-icon--confidence {
  background: color-mix(in srgb, var(--app-governance) 15%, transparent);
  color: var(--app-governance);
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

/* Goals rail */
.goal-card {
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
.goal-card:hover {
  box-shadow: 0 4px 12px rgb(0 0 0 / 8%);
  transform: translateY(-1px);
}
.goal-card--add {
  justify-content: center;
  border: 1px dashed var(--app-border);
  background: transparent;
  box-shadow: none;
}

/* Skill graph chips */
.graph-chip {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 0.75rem;
  padding: 0.2rem 0.6rem;
  border-radius: 999px;
  background: var(--app-muted);
  color: var(--app-foreground);
  transition: background 0.15s;
}
.graph-chip-dot {
  width: 0.5rem;
  height: 0.5rem;
  border-radius: 9999px;
  flex-shrink: 0;
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
