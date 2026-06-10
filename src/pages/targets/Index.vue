<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'

import { useTargets } from '@/composables/useTargets'
import { useSettings } from '@/composables/useSettings'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppInput, EmptyState, AppSpinner } from '@/components/ui'
import LearningPathView from '@/components/skills/LearningPathView.vue'
import type { LearningPath, Target } from '@/types'

const router = useRouter()
const { invoke } = useLocalApi()
const { targets, removeTarget, pathFor, combinedPath } = useTargets()

// Look up another user's public skill graph by DID.
const lookupDid = ref('')
const myDid = ref<string | null>(null)
function openInstructorGraph() {
  // Mobile keyboards auto-capitalize the first letter ("Did:key:…").
  // The DID scheme + method are case-insensitive per the DID spec, but
  // the base58 identifier is not — normalize only the prefix.
  const did = lookupDid.value.trim().replace(/^did:key:/i, 'did:key:')
  if (did) router.push(`/u/${encodeURIComponent(did)}`)
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
      targets.value.map(async (t) => [t.id, await pathFor(t).catch(() => null)] as const),
    )
    const map: Record<string, LearningPath> = {}
    for (const [id, p] of entries) if (p) map[id] = p
    paths.value = map
    combined.value = targets.value.length > 0 ? await combinedPath().catch(() => null) : null
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  await useSettings().initialize()
  myDid.value = await invoke<string | null>('get_local_did').catch(() => null)
  await loadAll()
})

function pct(p: LearningPath | undefined): number {
  if (!p || p.total === 0) return 0
  return Math.round((p.earned_count / p.total) * 100)
}

function nextStep(p: LearningPath | undefined): string | null {
  return p?.steps.find((s) => s.status === 'available')?.name ?? null
}

async function onRemove(t: Target) {
  await removeTarget(t.id)
  await loadAll()
}

const dash = computed(() => 2 * Math.PI * 20)
</script>

<template>
  <div>
    <div class="mb-6 flex items-end justify-between">
      <div>
        <h1 class="page-title">Your Targets</h1>
        <p class="mt-1 text-sm text-muted-foreground">
          Skill graphs you're working toward. Alexandria charts the path from what you've proven.
        </p>
      </div>
      <AppButton
        v-if="targets.length > 1"
        variant="outline"
        size="sm"
        @click="showCombined = !showCombined"
      >
        {{ showCombined ? 'Per target' : 'Combined path' }}
      </AppButton>
    </div>

    <!-- Instructor graph lookup -->
    <div class="card mb-6 p-4">
      <p class="mb-2 text-sm font-semibold text-foreground">View someone's skill graph</p>
      <div class="flex items-end gap-2">
        <div class="min-w-0 flex-1">
          <AppInput
            v-model="lookupDid"
            placeholder="did:key:…"
            data-testid="did-lookup-input"
            @keyup.enter="openInstructorGraph"
          />
        </div>
        <AppButton :disabled="!lookupDid.trim()" data-testid="did-lookup-go" @click="openInstructorGraph">
          View graph
        </AppButton>
      </div>
      <p v-if="myDid" class="mt-2 break-all text-[0.65rem] text-muted-foreground" data-testid="my-did">
        Your DID: {{ myDid }}
      </p>
    </div>

    <div v-if="loading" class="flex justify-center py-16">
      <AppSpinner size="lg" label="Charting your paths…" />
    </div>

    <EmptyState
      v-else-if="targets.length === 0"
      icon="🎯"
      title="No targets yet"
      description="Pick a skill graph to aim for — browse the taxonomy or an instructor's public graph and hit “Target this”."
    >
      <template #action>
        <AppButton class="mt-4" @click="router.push('/skills')">Browse skills</AppButton>
      </template>
    </EmptyState>

    <!-- Combined path -->
    <div v-else-if="showCombined && combined" class="card p-5">
      <div class="mb-4 flex items-center justify-between">
        <h2 class="text-base font-semibold text-foreground">Combined path</h2>
        <span class="text-xs text-muted-foreground">
          {{ combined.earned_count }} / {{ combined.total }} skills across {{ targets.length }} targets
        </span>
      </div>
      <LearningPathView :path="combined" />
    </div>

    <!-- Per-target cards -->
    <div v-else class="grid gap-5 sm:grid-cols-2 xl:grid-cols-3">
      <div v-for="t in targets" :key="t.id" class="card flex flex-col p-5">
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
              {{ paths[t.id]?.earned_count ?? 0 }} / {{ paths[t.id]?.total ?? t.goal_skill_ids.length }} skills
            </p>
            <p v-if="nextStep(paths[t.id])" class="mt-1 truncate text-xs text-primary">
              Next: {{ nextStep(paths[t.id]) }}
            </p>
            <p v-else class="mt-1 text-xs text-success">All prerequisites cleared 🎉</p>
          </div>
        </div>

        <div class="mt-4 flex items-center gap-2">
          <AppButton size="sm" @click="expanded = expanded === t.id ? null : t.id">
            {{ expanded === t.id ? 'Hide path' : 'View path' }}
          </AppButton>
          <button
            class="text-xs text-muted-foreground hover:text-destructive"
            @click="onRemove(t)"
          >
            Remove
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
