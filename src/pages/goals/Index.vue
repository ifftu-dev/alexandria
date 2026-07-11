<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'

import { useGoals } from '@/composables/useGoals'
import { useSettings } from '@/composables/useSettings'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppInput, EmptyState, AppSpinner } from '@/components/ui'
import LearningPathView from '@/components/skills/LearningPathView.vue'
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
const expanded = ref<string | null>(null)
const showCombined = ref(false)

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
      <AppButton
        v-if="goals.length > 1"
        variant="outline"
        size="sm"
        @click="showCombined = !showCombined"
      >
        {{ showCombined ? $t('goals.index.perGoal') : $t('goals.index.combinedPath') }}
      </AppButton>
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
        <AppButton class="mt-4" @click="router.push('/skills')">{{ $t('goals.index.browseSkills') }}</AppButton>
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
          <AppButton size="sm" @click="expanded = expanded === t.id ? null : t.id">
            {{ expanded === t.id ? $t('goals.index.hidePath') : $t('goals.index.viewPath') }}
          </AppButton>
          <button
            class="text-xs text-muted-foreground hover:text-destructive"
            @click="onRemove(t)"
          >
            {{ $t('common.actions.remove') }}
          </button>
        </div>

        <div v-if="expanded === t.id && paths[t.id]" class="mt-4 border-t border-border pt-4">
          <LearningPathView :path="paths[t.id]!" />
        </div>
      </div>
    </div>
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
