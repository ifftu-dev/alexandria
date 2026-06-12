<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'

import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppSpinner, EmptyState } from '@/components/ui'
import ProfileHeader from '@/components/profile/ProfileHeader.vue'
import type {
  FullReputationAssertion,
  Identity,
  PublicProfile,
  PublicSkillGraph,
} from '@/types'

const router = useRouter()
const { invoke } = useLocalApi()

const loading = ref(true)
const identity = ref<Identity | null>(null)
const did = ref<string | null>(null)
const graph = ref<PublicSkillGraph | null>(null)
const reputation = ref<FullReputationAssertion[]>([])

const profile = computed<PublicProfile | null>(() => {
  if (!identity.value || !did.value) return null
  return {
    did: did.value,
    username: identity.value.username,
    display_name: identity.value.display_name,
    bio: identity.value.bio,
    avatar_cid: identity.value.avatar_cid,
  }
})

const teachingImpact = computed(() =>
  Math.round(
    reputation.value.filter((a) => a.role === 'instructor').reduce((s, a) => s + a.score, 0),
  ),
)
const learningImpact = computed(() =>
  Math.round(
    reputation.value.filter((a) => a.role === 'learner').reduce((s, a) => s + a.score, 0),
  ),
)

onMounted(async () => {
  try {
    const [id, d, g, rep] = await Promise.all([
      invoke<Identity | null>('get_profile'),
      invoke<string | null>('get_local_did').catch(() => null),
      invoke<PublicSkillGraph>('get_my_skill_graph').catch(() => null),
      invoke<FullReputationAssertion[]>('get_reputation', { query: {} }).catch(() => []),
    ])
    identity.value = id
    did.value = d
    graph.value = g
    reputation.value = rep
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div>
    <div v-if="loading" class="flex justify-center py-16">
      <AppSpinner size="lg" label="Loading your profile…" />
    </div>

    <EmptyState
      v-else-if="!profile"
      icon="🔐"
      title="Profile unavailable"
      description="Unlock your profile to view it."
    />

    <div v-else class="space-y-6">
      <ProfileHeader :profile="profile" :is-own="true" :visibility="identity?.visibility ?? 'public'">
        <template #actions>
          <AppButton size="sm" variant="outline" @click="router.push('/settings/account')">
            ✏️ Edit profile
          </AppButton>
        </template>
      </ProfileHeader>

      <!-- This is what others see hint -->
      <p class="text-xs text-muted-foreground">
        {{
          identity?.visibility === 'private'
            ? 'Your profile is private — other users cannot fetch it, and your username is not discoverable.'
            : 'Your profile is public — anyone can view it by your username or DID.'
        }}
        Manage this in
        <button class="text-primary hover:underline" @click="router.push('/settings/account')">
          Settings → Account
        </button>.
      </p>

      <!-- Impact band -->
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
          <span class="stat-value">{{ graph?.nodes.length ?? 0 }}</span>
        </button>
        <button class="stat-card" @click="router.push('/skills')">
          <span class="stat-label">Public skills</span>
          <span class="stat-value">{{ graph?.nodes.filter((n) => n.public).length ?? 0 }}</span>
        </button>
      </div>

      <!-- Skill chips -->
      <div v-if="graph && graph.nodes.length > 0" class="card p-4">
        <div class="mb-2 flex items-center justify-between">
          <span class="text-sm font-semibold text-foreground">Your skill graph</span>
          <AppButton variant="ghost" size="xs" @click="router.push('/skills')">
            Manage visibility ▸
          </AppButton>
        </div>
        <div class="flex flex-wrap gap-2">
          <button
            v-for="n in graph.nodes"
            :key="n.id"
            class="skill-chip"
            :class="{ 'skill-chip--teaching': n.teaching, 'skill-chip--private': !n.public }"
            @click="router.push(`/skills/${n.id}`)"
          >
            {{ n.name }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.stat-card {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  padding: 0.75rem 0.9rem;
  border-radius: 0.75rem;
  background: var(--app-card);
  box-shadow: 0 1px 2px rgb(0 0 0 / 5%);
  text-align: left;
  transition: box-shadow 0.15s;
}
.stat-card:hover {
  box-shadow: 0 4px 12px rgb(0 0 0 / 8%);
}
.stat-label {
  font-size: 0.7rem;
  color: var(--app-muted-foreground);
}
.stat-value {
  font-size: 1.35rem;
  font-weight: 600;
  color: var(--app-foreground);
}
.skill-chip {
  font-size: 0.75rem;
  padding: 0.2rem 0.6rem;
  border-radius: 999px;
  background: var(--app-muted);
  color: var(--app-foreground);
}
.skill-chip--teaching {
  background: color-mix(in srgb, var(--app-primary) 85%, black);
  color: white;
}
.skill-chip--private {
  opacity: 0.55;
  border: 1px dashed var(--app-border);
}
</style>
