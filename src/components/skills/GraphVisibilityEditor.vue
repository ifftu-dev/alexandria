<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'

import { useLocalApi } from '@/composables/useLocalApi'
import { useSettings } from '@/composables/useSettings'
import { useGraphPrefs } from '@/composables/useGraphPrefs'
import { AppButton, AppBadge, AppSpinner } from '@/components/ui'
import type { PublicSkillGraph, PublicGraphNode } from '@/types'

const { invoke } = useLocalApi()
const { prefFor, setPublic, setTeaching, updateMany } = useGraphPrefs()

const loading = ref(true)
const graph = ref<PublicSkillGraph | null>(null)

const bloomColors: Record<string, string> = {
  remember: 'secondary',
  understand: 'primary',
  apply: 'accent',
  analyze: 'warning',
  evaluate: 'success',
  create: 'governance',
}

const nodes = computed<PublicGraphNode[]>(() => graph.value?.nodes ?? [])
const allIds = computed(() => nodes.value.map((n) => n.id))
const publicCount = computed(() => nodes.value.filter((n) => prefFor(n.id).public).length)

// Group by subject for a tidy editor.
const grouped = computed(() => {
  const map = new Map<string, PublicGraphNode[]>()
  for (const n of nodes.value) {
    const key = n.subject_name ?? 'Other'
    if (!map.has(key)) map.set(key, [])
    map.get(key)!.push(n)
  }
  return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]))
})

onMounted(async () => {
  await useSettings().initialize()
  try {
    graph.value = await invoke<PublicSkillGraph>('get_my_skill_graph')
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div class="card p-5">
    <div class="mb-4 flex flex-wrap items-center justify-between gap-2">
      <div>
        <h3 class="text-base font-semibold text-foreground">Visibility &amp; teaching</h3>
        <p class="mt-0.5 text-xs text-muted-foreground">
          Choose which earned skills are public, and mark the ones you teach.
        </p>
      </div>
      <div v-if="nodes.length" class="flex items-center gap-2">
        <span class="text-xs text-muted-foreground">{{ publicCount }} / {{ nodes.length }} public</span>
        <AppButton variant="ghost" size="xs" @click="updateMany(allIds, { public: true })">
          Show all
        </AppButton>
        <AppButton variant="ghost" size="xs" @click="updateMany(allIds, { public: false })">
          Hide all
        </AppButton>
      </div>
    </div>

    <div v-if="loading" class="flex justify-center py-8">
      <AppSpinner label="Loading your skills…" />
    </div>

    <p v-else-if="nodes.length === 0" class="py-6 text-sm text-muted-foreground">
      No earned skills yet — credentials you earn will appear here for you to publish.
    </p>

    <div v-else class="space-y-5">
      <div v-for="[subject, items] in grouped" :key="subject">
        <p class="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
          {{ subject }}
        </p>
        <ul class="divide-y divide-border rounded-lg border border-border">
          <li
            v-for="n in items"
            :key="n.id"
            class="flex items-center gap-3 px-3 py-2"
            :class="{ 'opacity-55': !prefFor(n.id).public }"
          >
            <span class="min-w-0 flex-1 truncate text-sm text-foreground">{{ n.name }}</span>
            <AppBadge :variant="(bloomColors[n.bloom_level] ?? 'primary') as any">
              {{ n.bloom_level }}
            </AppBadge>

            <!-- public toggle -->
            <button
              class="vis-toggle"
              :class="{ 'vis-toggle--on': prefFor(n.id).public }"
              :title="prefFor(n.id).public ? 'Public — click to hide' : 'Private — click to publish'"
              @click="setPublic(n.id, !prefFor(n.id).public)"
            >
              {{ prefFor(n.id).public ? '👁 Public' : '🔒 Private' }}
            </button>

            <!-- teaching toggle -->
            <button
              class="vis-toggle"
              :class="{ 'vis-toggle--teach': prefFor(n.id).teaching }"
              :disabled="!prefFor(n.id).public"
              :title="prefFor(n.id).teaching ? 'You teach this' : 'Mark as taught'"
              @click="setTeaching(n.id, !prefFor(n.id).teaching)"
            >
              {{ prefFor(n.id).teaching ? '★ Teaching' : '☆ Teach' }}
            </button>
          </li>
        </ul>
      </div>
    </div>
  </div>
</template>

<style scoped>
.vis-toggle {
  font-size: 0.7rem;
  font-weight: 500;
  padding: 0.2rem 0.55rem;
  border-radius: 999px;
  background: var(--app-muted);
  color: var(--app-muted-foreground);
  white-space: nowrap;
  transition:
    background 0.15s,
    color 0.15s;
}
.vis-toggle:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.vis-toggle--on {
  background: color-mix(in srgb, var(--app-success) 18%, transparent);
  color: var(--app-success);
}
.vis-toggle--teach {
  background: color-mix(in srgb, var(--app-primary) 85%, black);
  color: white;
}
</style>
