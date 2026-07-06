<script setup lang="ts">
// Unified instructor inbox: pending IRL-review submissions + classroom
// join requests, oldest first.
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppTabs, EmptyState } from '@/components/ui'
import type { InboxItem } from '@/types'

const { invoke } = useLocalApi()
const router = useRouter()

const items = ref<InboxItem[]>([])
const loading = ref(true)
const activeTab = ref('all')

const tabs = computed(() => [
  { key: 'all', label: 'All', count: items.value.length },
  { key: 'irl_submission', label: 'Submissions', count: items.value.filter(i => i.kind === 'irl_submission').length },
  { key: 'join_request', label: 'Join requests', count: items.value.filter(i => i.kind === 'join_request').length },
])

const visible = computed(() =>
  activeTab.value === 'all' ? items.value : items.value.filter(i => i.kind === activeTab.value),
)

onMounted(refresh)

async function refresh() {
  loading.value = true
  try {
    items.value = await invoke<InboxItem[]>('instructor_inbox')
  } finally {
    loading.value = false
  }
}

function open(item: InboxItem) {
  if (item.kind === 'irl_submission') {
    router.push(`/instructor/review/${item.target_id}`)
  } else {
    router.push(`/classrooms/${item.target_id}`)
  }
}

function iconFor(kind: InboxItem['kind']): string {
  return kind === 'irl_submission'
    ? 'M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z'
    : 'M18 9v3m0 0v3m0-3h3m-3 0h-3m-2-5a4 4 0 11-8 0 4 4 0 018 0zM3 20a6 6 0 0112 0v1H3v-1z'
}
</script>

<template>
  <div class="space-y-6">
    <div>
      <h1 class="text-2xl font-bold text-foreground">Inbox</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        Learner submissions awaiting your review, and pending classroom join requests.
      </p>
    </div>

    <AppTabs v-model="activeTab" :tabs="tabs" />

    <div v-if="loading" class="space-y-2">
      <div v-for="i in 3" :key="i" class="h-16 animate-pulse rounded-lg bg-muted-foreground/8" />
    </div>

    <EmptyState
      v-else-if="!visible.length"
      title="All caught up"
      description="Nothing is waiting for your review."
    />

    <div v-else class="space-y-2">
      <button
        v-for="item in visible"
        :key="`${item.kind}-${item.id}`"
        class="flex w-full items-center gap-3 rounded-xl border border-border bg-card p-4 text-left transition-colors hover:border-primary/50"
        @click="open(item)"
      >
        <span class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
          <svg class="h-4.5 w-4.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
            <path stroke-linecap="round" stroke-linejoin="round" :d="iconFor(item.kind)" />
          </svg>
        </span>
        <span class="min-w-0 flex-1">
          <span class="block truncate text-sm font-medium text-foreground">{{ item.title }}</span>
          <span v-if="item.subtitle" class="block truncate text-xs text-muted-foreground">{{ item.subtitle }}</span>
        </span>
        <span class="shrink-0 text-xs text-muted-foreground">{{ item.created_at.slice(0, 16) }}</span>
      </button>
    </div>
  </div>
</template>
