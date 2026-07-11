<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'

import { useLocalApi } from '@/composables/useLocalApi'
import { useSettings } from '@/composables/useSettings'
import { useGraphPrefs } from '@/composables/useGraphPrefs'
import { AppButton, AppBadge, AppSpinner } from '@/components/ui'
import type { PublicSkillGraph, PublicGraphNode } from '@/types'
import { bloomBadge } from '@/utils/bloom'

const { invoke } = useLocalApi()
const { prefFor, setPublic, setTeaching, updateMany } = useGraphPrefs()

const loading = ref(true)
const graph = ref<PublicSkillGraph | null>(null)

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
        <h3 class="text-base font-semibold text-foreground">{{ $t('skills.visibility.title') }}</h3>
        <p class="mt-0.5 text-xs text-muted-foreground">
          {{ $t('skills.visibility.subtitle') }}
        </p>
      </div>
      <div v-if="nodes.length" class="flex items-center gap-2">
        <span class="text-xs text-muted-foreground">{{ $t('skills.visibility.publicCount', { public: publicCount, total: nodes.length }) }}</span>
        <AppButton variant="ghost" size="xs" @click="updateMany(allIds, { public: true })">
          {{ $t('skills.visibility.showAll') }}
        </AppButton>
        <AppButton variant="ghost" size="xs" @click="updateMany(allIds, { public: false })">
          {{ $t('skills.visibility.hideAll') }}
        </AppButton>
      </div>
    </div>

    <div v-if="loading" class="flex justify-center py-8">
      <AppSpinner :label="$t('skills.visibility.loading')" />
    </div>

    <p v-else-if="nodes.length === 0" class="py-6 text-sm text-muted-foreground">
      {{ $t('skills.visibility.empty') }}
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
            <AppBadge :variant="bloomBadge(n.bloom_level)">
              {{ n.bloom_level }}
            </AppBadge>

            <!-- public toggle -->
            <button
              class="vis-toggle"
              :class="{ 'vis-toggle--on': prefFor(n.id).public }"
              :title="prefFor(n.id).public ? $t('skills.visibility.publicOnTitle') : $t('skills.visibility.publicOffTitle')"
              @click="setPublic(n.id, !prefFor(n.id).public)"
            >
              {{ prefFor(n.id).public ? $t('skills.visibility.publicOn') : $t('skills.visibility.publicOff') }}
            </button>

            <!-- teaching toggle -->
            <button
              class="vis-toggle"
              :class="{ 'vis-toggle--teach': prefFor(n.id).teaching }"
              :disabled="!prefFor(n.id).public"
              :title="prefFor(n.id).teaching ? $t('skills.visibility.teachOnTitle') : $t('skills.visibility.teachOffTitle')"
              @click="setTeaching(n.id, !prefFor(n.id).teaching)"
            >
              {{ prefFor(n.id).teaching ? $t('skills.visibility.teachOn') : $t('skills.visibility.teachOff') }}
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
