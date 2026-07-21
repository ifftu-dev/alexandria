<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'

import { useGoals } from '@/composables/useGoals'
import { useSettings } from '@/composables/useSettings'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppInput, AppModal, EmptyState, AppSpinner } from '@/components/ui'
import LearningPathView from '@/components/skills/LearningPathView.vue'
import GoalPicker from '@/components/goals/GoalPicker.vue'
import SkillBootstrapPanel from '@/components/skills/SkillBootstrapPanel.vue'
import type { LearningPath, Goal } from '@/types'

const router = useRouter()
const { invoke } = useLocalApi()
const { goals, removeGoal, pathFor, combinedPath } = useGoals()

// Look up another user by @username (or, as a power-user fallback, DID).
const lookupQuery = ref('')
const myUsername = ref<string | null>(null)
function openUserProfile() {
  // Mobile keyboards auto-capitalize typed input. Usernames are
  // lowercase by definition; for DIDs only the scheme prefix is
  // case-insensitive — normalize accordingly.
  let q = lookupQuery.value.trim()
  if (/^did:key:/i.test(q)) {
    q = q.replace(/^did:key:/i, 'did:key:')
  } else {
    q = q.replace(/^@/, '').toLowerCase()
  }
  if (q) router.push(`/u/${encodeURIComponent(q)}`)
}

const loading = ref(true)
const paths = ref<Record<string, LearningPath>>({})
const combined = ref<LearningPath | null>(null)
// Set of expanded goal ids — each card opens/closes independently.
const expanded = ref<Set<string>>(new Set())
const showCombined = ref(false)

function isExpanded(id: string): boolean {
  return expanded.value.has(id)
}
function toggleExpand(id: string): void {
  if (expanded.value.has(id)) expanded.value.delete(id)
  else expanded.value.add(id)
}

async function loadAll() {
  loading.value = true
  try {
    const entries = await Promise.all(
      goals.value.map(async (t) => [t.id, await pathFor(t).catch(() => null)] as const),
    )
    const map: Record<string, LearningPath> = {}
    for (const [id, p] of entries) if (p) map[id] = p
    paths.value = map
    combined.value = goals.value.length > 0 ? await combinedPath().catch(() => null) : null
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  await useSettings().initialize()
  const me = await invoke<{ username: string | null } | null>('get_profile').catch(() => null)
  myUsername.value = me?.username ?? null
  await loadAll()
})

function pct(p: LearningPath | undefined): number {
  if (!p || p.total === 0) return 0
  return Math.round((p.earned_count / p.total) * 100)
}

function nextStep(p: LearningPath | undefined): string | null {
  return p?.steps.find((s) => s.status === 'available')?.name ?? null
}

async function onRemove(t: Goal) {
  await removeGoal(t.id)
  await loadAll()
}

// ---- Add a goal (same flow as onboarding) --------------------------------
// Opens a modal that reuses the onboarding GoalPicker, then offers to add or
// change skills via the resume/transcript bootstrap — those are recorded as
// self-asserted (self-made) claims, exactly as during onboarding.
const showAddGoal = ref(false)
const addStep = ref<'goal' | 'skills'>('goal')
const skillsClaimed = ref(0)

function openAddGoal() {
  addStep.value = 'goal'
  skillsClaimed.value = 0
  showAddGoal.value = true
}

async function onGoalAdded() {
  // A goal was set — refresh paths, then ask about skills.
  await loadAll()
  addStep.value = 'skills'
}

function onSkillsClaimed(n: number) {
  skillsClaimed.value += n
}

async function closeAddGoal() {
  showAddGoal.value = false
  addStep.value = 'goal'
  await loadAll()
}

const dash = computed(() => 2 * Math.PI * 20)
</script>

<template>
  <div>
    <div class="mb-6 flex items-end justify-between">
      <div>
        <h1 class="page-title">{{ $t('goals.index.title') }}</h1>
        <p class="mt-1 text-sm text-muted-foreground">
          {{ $t('goals.index.subtitle') }}
        </p>
      </div>
      <div class="flex items-center gap-2">
        <AppButton
          v-if="goals.length > 1"
          variant="outline"
          size="sm"
          @click="showCombined = !showCombined"
        >
          {{ showCombined ? $t('goals.index.perGoal') : $t('goals.index.combinedPath') }}
        </AppButton>
        <AppButton size="sm" data-testid="add-goal" @click="openAddGoal">
          {{ $t('goals.index.addGoal') }}
        </AppButton>
      </div>
    </div>

    <!-- User lookup -->
    <div class="card mb-6 p-4">
      <p class="mb-2 text-sm font-semibold text-foreground">{{ $t('goals.index.findSomeone') }}</p>
      <div class="flex items-end gap-2">
        <div class="min-w-0 flex-1">
          <AppInput
            v-model="lookupQuery"
            :placeholder="$t('goals.index.usernamePlaceholder')"
            data-testid="user-lookup-input"
            @keyup.enter="openUserProfile"
          />
        </div>
        <AppButton :disabled="!lookupQuery.trim()" data-testid="user-lookup-go" @click="openUserProfile">
          {{ $t('goals.index.viewProfile') }}
        </AppButton>
      </div>
      <p v-if="myUsername" class="mt-2 text-[0.65rem] text-muted-foreground" data-testid="my-username">
        {{ $t('goals.index.yourHandle', { name: myUsername }) }}
      </p>
    </div>

    <div v-if="loading" class="flex justify-center py-16">
      <AppSpinner size="lg" :label="$t('goals.index.chartingPaths')" />
    </div>

    <EmptyState
      v-else-if="goals.length === 0"
      icon="🎯"
      :title="$t('goals.index.emptyTitle')"
      :description="$t('goals.index.emptyDescription')"
    >
      <template #action>
        <div class="mt-4 flex flex-wrap justify-center gap-2">
          <AppButton data-testid="add-goal-empty" @click="openAddGoal">{{ $t('goals.index.addGoal') }}</AppButton>
          <AppButton variant="outline" @click="router.push('/skills')">{{ $t('goals.index.browseSkills') }}</AppButton>
        </div>
      </template>
    </EmptyState>

    <!-- Combined path -->
    <div v-else-if="showCombined && combined" class="card p-5">
      <div class="mb-4 flex items-center justify-between">
        <h2 class="text-base font-semibold text-foreground">{{ $t('goals.index.combinedPath') }}</h2>
        <span class="text-xs text-muted-foreground">
          {{ $t('goals.index.skillsAcrossGoals', { earned: combined.earned_count, total: combined.total, goals: goals.length }) }}
        </span>
      </div>
      <LearningPathView :path="combined" />
    </div>

    <!-- Per-goal cards -->
    <div v-else class="grid gap-5 sm:grid-cols-2 xl:grid-cols-3">
      <div v-for="t in goals" :key="t.id" class="card flex flex-col p-5">
        <div class="flex items-start gap-4">
          <!-- progress ring -->
          <svg width="52" height="52" viewBox="0 0 52 52" class="shrink-0">
            <circle cx="26" cy="26" r="20" fill="none" stroke="var(--app-border)" stroke-width="5" />
            <circle
              cx="26" cy="26" r="20" fill="none"
              stroke="var(--app-primary)" stroke-width="5" stroke-linecap="round"
              :stroke-dasharray="dash"
              :stroke-dashoffset="dash * (1 - pct(paths[t.id]) / 100)"
              transform="rotate(-90 26 26)"
              class="transition-all duration-500"
            />
            <text x="26" y="26" text-anchor="middle" dominant-baseline="central"
              font-size="12" font-weight="600" fill="var(--app-foreground)">
              {{ pct(paths[t.id]) }}%
            </text>
          </svg>
          <div class="min-w-0 flex-1">
            <h3 class="truncate text-sm font-semibold text-foreground">{{ t.label }}</h3>
            <p class="mt-0.5 text-xs text-muted-foreground">
              {{ $t('goals.index.skillsCount', { earned: paths[t.id]?.earned_count ?? 0, total: paths[t.id]?.total ?? t.goal_skill_ids.length }) }}
            </p>
            <p v-if="nextStep(paths[t.id])" class="mt-1 truncate text-xs text-primary">
              {{ $t('goals.index.nextStep', { name: nextStep(paths[t.id]) }) }}
            </p>
            <p v-else class="mt-1 text-xs text-success">{{ $t('goals.index.allCleared') }}</p>
          </div>
        </div>

        <div class="mt-4 flex items-center gap-2">
          <AppButton size="sm" @click="toggleExpand(t.id)">
            {{ isExpanded(t.id) ? $t('goals.index.hidePath') : $t('goals.index.viewPath') }}
          </AppButton>
          <button
            class="text-xs text-muted-foreground hover:text-destructive"
            @click="onRemove(t)"
          >
            {{ $t('common.actions.remove') }}
          </button>
        </div>

        <div v-if="isExpanded(t.id) && paths[t.id]" class="mt-4 border-t border-border pt-4">
          <LearningPathView :path="paths[t.id]!" />
        </div>
      </div>
    </div>

    <!-- Add-a-goal modal — same flow as onboarding -->
    <AppModal
      :open="showAddGoal"
      :title="addStep === 'goal' ? $t('goals.index.addGoalTitle') : $t('goals.index.updateSkillsTitle')"
      max-width="34rem"
      @close="closeAddGoal"
    >
      <!-- Step 1: pick a goal (curated templates or a job description) -->
      <div v-if="addStep === 'goal'">
        <p class="mb-4 text-sm text-muted-foreground">{{ $t('goals.index.addGoalIntro') }}</p>
        <GoalPicker @added="onGoalAdded" />
      </div>

      <!-- Step 2: optionally add/change skills (self-made claims) -->
      <div v-else>
        <p class="mb-1 text-sm text-muted-foreground">{{ $t('goals.index.updateSkillsIntro') }}</p>
        <p class="mb-4 text-xs text-muted-foreground/80">{{ $t('goals.index.updateSkillsSelfMade') }}</p>
        <SkillBootstrapPanel @claimed="onSkillsClaimed" />
        <p v-if="skillsClaimed > 0" class="mt-3 text-xs text-success">
          {{ $t('goals.index.skillsClaimed', { count: skillsClaimed }, skillsClaimed) }}
        </p>
        <div class="mt-5 flex justify-end">
          <AppButton data-testid="add-goal-done" @click="closeAddGoal">{{ $t('goals.index.done') }}</AppButton>
        </div>
      </div>
    </AppModal>
  </div>
</template>

<style scoped>
.page-title {
  font-family: 'Libre Baskerville', 'DM Serif Display', Georgia, serif;
  font-size: 1.6rem;
  font-weight: 400;
  letter-spacing: -0.01em;
  color: var(--app-foreground);
}
</style>
