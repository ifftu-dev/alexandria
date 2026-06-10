<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { useLocalApi } from '@/composables/useLocalApi'
import { useDisplayNames, shortDid } from '@/composables/useDisplayNames'
import { useTargets } from '@/composables/useTargets'
import { AppButton, AppBadge, AppSpinner, AppAlert, EmptyState } from '@/components/ui'
import SkillGraph from '@/components/skills/SkillGraph.vue'
import type { PublicSkillGraph, SkillInfo, SkillGraphEdge } from '@/types'

const route = useRoute()
const router = useRouter()
const { invoke } = useLocalApi()
const { displayName, ensureNames } = useDisplayNames()
const { targets, addTarget, removeTarget } = useTargets()

// Normalize the scheme prefix — mobile keyboards capitalize pasted/typed
// DIDs ("Did:key:…") and the owner-match on the serving node is exact.
const did = computed(() => String(route.params.did ?? '').replace(/^did:key:/i, 'did:key:'))

const loading = ref(true)
const error = ref<string | null>(null)
const graph = ref<PublicSkillGraph | null>(null)
const adding = ref(false)

const name = computed(() => displayName(did.value) || shortDid(did.value))

const existingTarget = computed(() => targets.value.find((t) => t.source_did === did.value))

const teachingNodes = computed(() => graph.value?.nodes.filter((n) => n.teaching) ?? [])

// Adapt the public graph into the shapes SkillGraph.vue expects.
const skills = computed<SkillInfo[]>(
  () =>
    graph.value?.nodes.map((n) => ({
      id: n.id,
      name: n.name,
      description: null,
      subject_id: null,
      subject_name: n.subject_name,
      subject_field_id: null,
      subject_field_name: null,
      bloom_level: n.bloom_level,
      prerequisite_count: 0,
      dependent_count: 0,
      created_at: null,
    })) ?? [],
)

const edges = computed<SkillGraphEdge[]>(() => {
  const byId = new Map(graph.value?.nodes.map((n) => [n.id, n]) ?? [])
  return (
    graph.value?.edges.map((e) => {
      const sk = byId.get(e.skill_id)
      const pr = byId.get(e.prerequisite_id)
      return {
        skill_id: e.skill_id,
        skill_name: sk?.name ?? e.skill_id,
        skill_bloom: sk?.bloom_level ?? 'apply',
        prerequisite_id: e.prerequisite_id,
        prerequisite_name: pr?.name ?? e.prerequisite_id,
        prerequisite_bloom: pr?.bloom_level ?? 'apply',
      }
    }) ?? []
  )
})

async function load() {
  loading.value = true
  error.value = null
  graph.value = null
  try {
    await ensureNames([did.value])
    graph.value = await invoke<PublicSkillGraph>('fetch_public_graph', { did: did.value })
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(did, load)

async function onTarget() {
  if (!graph.value || graph.value.nodes.length === 0) return
  adding.value = true
  try {
    await addTarget({
      label: `${name.value} · skill graph`,
      goalSkillIds: graph.value.nodes.map((n) => n.id),
      sourceDid: did.value,
    })
    router.push('/targets')
  } finally {
    adding.value = false
  }
}

async function onUntarget() {
  if (existingTarget.value) await removeTarget(existingTarget.value.id)
}
</script>

<template>
  <div>
    <button class="mb-4 text-xs text-muted-foreground hover:text-foreground" @click="router.back()">
      ‹ back
    </button>

    <div class="mb-6 flex flex-wrap items-start justify-between gap-4">
      <div>
        <h1 class="page-title">{{ name }}</h1>
        <p class="mt-1 text-xs text-muted-foreground break-all">{{ did }}</p>
      </div>
      <div class="flex items-center gap-2">
        <AppButton
          v-if="!existingTarget"
          :loading="adding"
          :disabled="!graph || graph.nodes.length === 0"
          @click="onTarget"
        >
          🎯 Target this graph
        </AppButton>
        <template v-else>
          <AppBadge variant="success">Targeted</AppBadge>
          <AppButton variant="outline" size="sm" @click="onUntarget">Remove target</AppButton>
        </template>
      </div>
    </div>

    <div v-if="loading" class="flex justify-center py-16">
      <AppSpinner size="lg" label="Fetching public graph…" />
    </div>

    <AppAlert v-else-if="error" variant="error">
      Couldn't fetch this graph: {{ error }}
      <div class="mt-1 text-xs opacity-80">
        Remote graphs are fetched over P2P from connected peers. The owner's node must be online and
        reachable.
      </div>
    </AppAlert>

    <EmptyState
      v-else-if="!graph || graph.nodes.length === 0"
      icon="📭"
      title="No public skills"
      description="This person hasn't made any earned skills public yet."
    />

    <div v-else class="space-y-6">
      <!-- Teaching highlight -->
      <div v-if="teachingNodes.length > 0" class="card p-4">
        <div class="mb-2 flex items-center gap-2">
          <span class="text-sm font-semibold text-foreground">Teaches</span>
          <span class="text-xs text-muted-foreground">— opted to instruct these</span>
        </div>
        <div class="flex flex-wrap gap-2">
          <button
            v-for="n in teachingNodes"
            :key="n.id"
            class="teach-pill"
            @click="router.push(`/skills/${n.id}`)"
          >
            {{ n.name }}
          </button>
        </div>
      </div>

      <!-- Full public DAG -->
      <div>
        <div class="mb-2 flex items-center justify-between">
          <h2 class="text-base font-semibold text-foreground">Public skill graph</h2>
          <span class="text-xs text-muted-foreground">
            {{ graph.nodes.length }} skills · {{ teachingNodes.length }} taught
          </span>
        </div>
        <SkillGraph :skills="skills" :edges="edges" @select="(id) => router.push(`/skills/${id}`)" />
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
.teach-pill {
  font-size: 0.75rem;
  font-weight: 500;
  padding: 0.2rem 0.6rem;
  border-radius: 999px;
  color: white;
  background: color-mix(in srgb, var(--app-primary) 85%, black);
  transition: opacity 0.15s;
}
.teach-pill:hover {
  opacity: 0.85;
}
</style>
