<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, EmptyState, AppBadge, ProvenanceBadge } from '@/components/ui'
import type { OpinionRow, SubjectFieldInfo } from '@/types'

const { invoke } = useLocalApi()

const loading = ref(true)
const opinions = ref<OpinionRow[]>([])
const fields = ref<SubjectFieldInfo[]>([])

// Current subject-field filter. Empty string = all fields.
const selectedField = ref<string>(
  sessionStorage.getItem('opinions-field-filter') || '',
)

function setField(id: string) {
  selectedField.value = id
  sessionStorage.setItem('opinions-field-filter', id)
}

// Map subject_field_id → friendly name for header rendering
const fieldById = computed(() => {
  const m = new Map<string, SubjectFieldInfo>()
  for (const f of fields.value) m.set(f.id, f)
  return m
})

// Opinions grouped by subject field (only the ones in the current
// filter, or all if no filter).
const grouped = computed(() => {
  const filtered = selectedField.value
    ? opinions.value.filter((o) => o.subject_field_id === selectedField.value)
    : opinions.value
  const groups = new Map<string, OpinionRow[]>()
  for (const op of filtered) {
    const key = op.subject_field_id
    if (!groups.has(key)) groups.set(key, [])
    groups.get(key)!.push(op)
  }
  // Sort groups alphabetically by field name; opinions within each group
  // already come back newest-first from the backend.
  return Array.from(groups.entries())
    .map(([id, ops]) => ({ id, name: fieldById.value.get(id)?.name || id, opinions: ops }))
    .sort((a, b) => a.name.localeCompare(b.name))
})

const counts = computed(() => {
  const m = new Map<string, number>()
  for (const o of opinions.value) {
    m.set(o.subject_field_id, (m.get(o.subject_field_id) ?? 0) + 1)
  }
  return m
})

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
      year: 'numeric',
    })
  } catch {
    return iso
  }
}

onMounted(async () => {
  try {
    const [ops, f] = await Promise.all([
      invoke<OpinionRow[]>('list_opinions', {
        subjectFieldId: null,
        authorAddress: null,
        includeWithdrawn: false,
        limit: 200,
      }),
      invoke<SubjectFieldInfo[]>('list_subject_fields', {}),
    ])
    opinions.value = ops
    fields.value = f
  } catch (e) {
    console.error('Failed to load opinions:', e)
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div>
    <div class="mb-6 flex items-center justify-between">
      <div>
        <h1 class="text-xl font-bold">Field Commentary</h1>
        <p class="text-sm text-muted-foreground">
          Opinions from educators credentialed in each field. Chronological, not ranked.
        </p>
      </div>
      <AppButton variant="primary" size="sm" @click="$router.push('/opinions/new')">
        <svg class="w-4 h-4 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 4v16m8-8H4" />
        </svg>
        Post Opinion
      </AppButton>
    </div>

    <!-- Subject field filter chips -->
    <div v-if="fields.length" class="mb-6 flex flex-wrap items-center gap-2">
      <button
        type="button"
        :class="[
          'text-xs font-medium px-3 py-1.5 rounded-full transition-colors',
          selectedField === ''
            ? 'bg-primary text-primary-foreground'
            : 'bg-muted text-muted-foreground hover:bg-muted/70',
        ]"
        @click="setField('')"
      >
        All <span class="ml-1 opacity-70">{{ opinions.length }}</span>
      </button>
      <button
        v-for="f in fields"
        :key="f.id"
        type="button"
        :class="[
          'text-xs font-medium px-3 py-1.5 rounded-full transition-colors',
          selectedField === f.id
            ? 'bg-primary text-primary-foreground'
            : 'bg-muted text-muted-foreground hover:bg-muted/70',
        ]"
        @click="setField(f.id)"
      >
        {{ f.icon_emoji ? f.icon_emoji + ' ' : '' }}{{ f.name }}
        <span class="ml-1 opacity-70">{{ counts.get(f.id) ?? 0 }}</span>
      </button>
    </div>

    <!-- Loading -->
    <div v-if="loading" class="space-y-4">
      <div v-for="i in 3" :key="i" class="animate-pulse rounded-xl bg-card p-4 shadow-sm">
        <div class="h-5 w-40 rounded bg-muted mb-3" />
        <div class="h-4 w-3/4 rounded bg-muted mb-2" />
        <div class="h-4 w-1/2 rounded bg-muted" />
      </div>
    </div>

    <!-- Empty state -->
    <EmptyState
      v-else-if="grouped.length === 0"
      title="No opinions yet"
      :description="
        selectedField
          ? 'No opinions in this subject field yet.'
          : 'Educators credentialed in a subject can post opinion videos here. Be the first.'
      "
    />

    <!-- Grouped list -->
    <div v-else class="space-y-8">
      <section v-for="group in grouped" :key="group.id">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground mb-3">
          {{ group.name }}
          <span class="ml-1 text-xs font-normal opacity-60">
            · {{ group.opinions.length }} {{ group.opinions.length === 1 ? 'opinion' : 'opinions' }}
          </span>
        </h2>

        <div class="space-y-2">
          <router-link
            v-for="op in group.opinions"
            :key="op.id"
            :to="`/opinions/${op.id}`"
            class="op-row group"
          >
            <div class="op-row__thumb">
              <svg class="h-6 w-6 text-primary/70" fill="currentColor" viewBox="0 0 24 24">
                <path d="M8 5v14l11-7z" />
              </svg>
            </div>
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2 mb-1">
                <span class="truncate text-sm font-semibold text-foreground group-hover:text-primary">
                  {{ op.title }}
                </span>
                <AppBadge v-if="op.duration_seconds" variant="secondary">
                  {{ Math.round(op.duration_seconds / 60) }} min
                </AppBadge>
                <ProvenanceBadge :provenance="op.provenance" />
              </div>
              <p v-if="op.summary" class="line-clamp-2 text-xs text-muted-foreground">
                {{ op.summary }}
              </p>
              <div class="mt-1 flex items-center gap-2 text-[11px] text-muted-foreground">
                <code class="truncate max-w-[14rem]">{{ op.author_address }}</code>
                <span>·</span>
                <span>{{ formatDate(op.published_at) }}</span>
              </div>
            </div>
          </router-link>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.op-row {
  display: flex;
  align-items: flex-start;
  gap: 0.75rem;
  padding: 0.75rem 1rem;
  border-radius: 0.5rem;
  background: var(--app-card);
  box-shadow: 0 1px 3px rgb(0 0 0 / 0.04);
  text-decoration: none;
  color: inherit;
  transition: transform 0.15s ease, box-shadow 0.15s ease;
}
.op-row:hover {
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgb(0 0 0 / 0.08);
}
.op-row__thumb {
  flex-shrink: 0;
  width: 2.25rem;
  height: 2.25rem;
  border-radius: 0.5rem;
  background: color-mix(in srgb, var(--app-primary) 10%, transparent);
  display: flex;
  align-items: center;
  justify-content: center;
}
</style>
