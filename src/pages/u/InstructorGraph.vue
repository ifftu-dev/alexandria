<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import { useLocalApi } from '@/composables/useLocalApi'
import { useTargets } from '@/composables/useTargets'
import { AppButton, AppBadge, AppSpinner, AppAlert, EmptyState } from '@/components/ui'
import ProfileHeader from '@/components/profile/ProfileHeader.vue'
import SkillGraph from '@/components/skills/SkillGraph.vue'
import type { PublicProfile, PublicSkillGraph, SkillInfo, SkillGraphEdge } from '@/types'

const route = useRoute()
const router = useRouter()
const { invoke } = useLocalApi()
const { targets, addTarget, removeTarget } = useTargets()

// The route accepts a DID or a username (with or without a leading @).
// Mobile keyboards capitalize typed input, and the owner-match on the
// serving node is exact — normalize the did:key: scheme prefix only.
const param = computed(() => String(route.params.id ?? '').trim())
const isDid = computed(() => /^did:key:/i.test(param.value))
const lookupDid = computed(() =>
  isDid.value ? param.value.replace(/^did:key:/i, 'did:key:') : null,
)
const lookupUsername = computed(() =>
  isDid.value ? null : param.value.replace(/^@/, '').toLowerCase(),
)

const loading = ref(true)
const error = ref<string | null>(null)
const profile = ref<PublicProfile | null>(null)
const graph = ref<PublicSkillGraph | null>(null)
const adding = ref(false)

const name = computed(
  () => profile.value?.display_name || profile.value?.username || 'this user',
)

const existingTarget = computed(() =>
  targets.value.find((t) => t.source_did === profile.value?.did),
)

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
  profile.value = null
  graph.value = null
  try {
    // Resolve the profile first — by DID or username — then fetch the
    // skill graph for the resolved DID.
    profile.value = await invoke<PublicProfile>('fetch_user_profile', {
      did: lookupDid.value,
      username: lookupUsername.value,
    })
    graph.value = await invoke<PublicSkillGraph>('fetch_public_graph', {
      did: profile.value.did,
    })
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(param, load)

async function onTarget() {
  if (!profile.value || !graph.value || graph.value.nodes.length === 0) return
  adding.value = true
  try {
    await addTarget({
      label: `${name.value} · skill graph`,
      goalSkillIds: graph.value.nodes.map((n) => n.id),
      sourceDid: profile.value.did,
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

    <div v-if="loading" class="flex justify-center py-16">
      <AppSpinner size="lg" label="Fetching profile…" />
    </div>

    <AppAlert v-else-if="error" variant="error">
      Couldn't load this profile: {{ error }}
      <div class="mt-1 text-xs opacity-80">
        Profiles are fetched over P2P. The owner's node must be online and reachable, and their
        profile public.
      </div>
    </AppAlert>

    <div v-else-if="profile" class="space-y-6">
      <ProfileHeader :profile="profile">
        <template #actions>
          <AppButton
            v-if="!existingTarget"
            size="sm"
            :loading="adding"
            :disabled="!graph || graph.nodes.length === 0"
            @click="onTarget"
          >
            🎯 Target this graph
          </AppButton>
          <template v-else>
            <AppBadge variant="success">Targeted</AppBadge>
            <AppButton variant="outline" size="sm" @click="onUntarget">Remove</AppButton>
          </template>
        </template>
      </ProfileHeader>

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
      <div v-if="graph && graph.nodes.length > 0">
        <div class="mb-2 flex items-center justify-between">
          <h2 class="text-base font-semibold text-foreground">Public skill graph</h2>
          <span class="text-xs text-muted-foreground">
            {{ graph.nodes.length }} skills · {{ teachingNodes.length }} taught
          </span>
        </div>
        <SkillGraph :skills="skills" :edges="edges" @select="(id) => router.push(`/skills/${id}`)" />
      </div>
      <EmptyState
        v-else
        icon="📭"
        title="No public skills"
        :description="`${name} hasn't made any earned skills public yet.`"
      />
    </div>
  </div>
</template>

<style scoped>
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
