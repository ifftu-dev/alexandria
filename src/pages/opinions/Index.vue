<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, EmptyState, ProvenanceBadge } from '@/components/ui'
import { useDisplayNames } from '@/composables/useDisplayNames'
import type { OpinionRow, SubjectFieldInfo } from '@/types'

const { invoke } = useLocalApi()
const { displayName, ensureNames } = useDisplayNames()

const loading = ref(true)
const opinions = ref<OpinionRow[]>([])
const fields = ref<SubjectFieldInfo[]>([])

// Resolved thumbnail object URLs, keyed by thumbnail_cid. Revoked on unmount.
const thumbs = ref<Record<string, string>>({})

const selectedField = ref<string>(sessionStorage.getItem('opinions-field-filter') || '')

function setField(id: string) {
  selectedField.value = id
  sessionStorage.setItem('opinions-field-filter', id)
}

const fieldById = computed(() => {
  const m = new Map<string, SubjectFieldInfo>()
  for (const f of fields.value) m.set(f.id, f)
  return m
})

// Opinions grouped by subject field (filtered), newest-first within a group.
const grouped = computed(() => {
  const filtered = selectedField.value
    ? opinions.value.filter((o) => o.subject_field_id === selectedField.value)
    : opinions.value
  const groups = new Map<string, OpinionRow[]>()
  for (const op of filtered) {
    if (!groups.has(op.subject_field_id)) groups.set(op.subject_field_id, [])
    groups.get(op.subject_field_id)!.push(op)
  }
  return Array.from(groups.entries())
    .map(([id, ops]) => ({ id, name: fieldById.value.get(id)?.name || id, opinions: ops }))
    .sort((a, b) => a.name.localeCompare(b.name))
})

const counts = computed(() => {
  const m = new Map<string, number>()
  for (const o of opinions.value) m.set(o.subject_field_id, (m.get(o.subject_field_id) ?? 0) + 1)
  return m
})

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })
  } catch {
    return iso
  }
}

function durationLabel(seconds: number | null): string | null {
  if (!seconds) return null
  const m = Math.floor(seconds / 60)
  const s = Math.floor(seconds % 60)
  return `${m}:${String(s).padStart(2, '0')}`
}

// Best-effort thumbnail resolution — the CSP blocks `asset:` for <img>, so we
// pull the bytes through the backend and hand the tag a blob: URL. A missing
// or unresolvable thumbnail just falls back to the gradient placeholder.
async function resolveThumb(cid: string) {
  if (thumbs.value[cid]) return
  try {
    const bytes = await invoke<number[]>('content_resolve_bytes', { identifier: cid })
    const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)]))
    thumbs.value = { ...thumbs.value, [cid]: url }
  } catch {
    /* keep placeholder */
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
    void ensureNames(ops.map((o) => o.author_address))
    for (const o of ops) if (o.thumbnail_cid) void resolveThumb(o.thumbnail_cid)
  } catch (e) {
    console.error('Failed to load opinions:', e)
  } finally {
    loading.value = false
  }
})

onBeforeUnmount(() => {
  for (const url of Object.values(thumbs.value)) URL.revokeObjectURL(url)
})
</script>

<template>
  <div>
    <div class="mb-6 flex items-center justify-between">
      <div>
        <h1 class="text-xl font-bold">{{ $t('opinions.index.title') }}</h1>
        <p class="text-sm text-muted-foreground">
          {{ $t('opinions.index.subtitle') }}
        </p>
      </div>
      <AppButton variant="primary" size="sm" @click="$router.push('/opinions/new')">
        <svg class="w-4 h-4 me-1" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 4v16m8-8H4" />
        </svg>
        {{ $t('opinions.index.post') }}
      </AppButton>
    </div>

    <!-- Subject field filter chips -->
    <div v-if="fields.length" class="mb-6 flex flex-wrap items-center gap-2">
      <button
        type="button"
        :class="[
          'text-xs font-medium px-3 py-1.5 rounded-full transition-colors',
          selectedField === '' ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground hover:bg-muted/70',
        ]"
        @click="setField('')"
      >
        {{ $t('opinions.index.filterAll') }} <span class="ms-1 opacity-70">{{ opinions.length }}</span>
      </button>
      <button
        v-for="f in fields"
        :key="f.id"
        type="button"
        :class="[
          'text-xs font-medium px-3 py-1.5 rounded-full transition-colors',
          selectedField === f.id ? 'bg-primary text-primary-foreground' : 'bg-muted text-muted-foreground hover:bg-muted/70',
        ]"
        @click="setField(f.id)"
      >
        {{ f.icon_emoji ? f.icon_emoji + ' ' : '' }}{{ f.name }}
        <span class="ms-1 opacity-70">{{ counts.get(f.id) ?? 0 }}</span>
      </button>
    </div>

    <!-- Loading skeleton grid -->
    <div v-if="loading" class="op-grid">
      <div v-for="i in 8" :key="i" class="animate-pulse">
        <div class="aspect-video rounded-xl bg-muted" />
        <div class="mt-2 h-4 w-3/4 rounded bg-muted" />
        <div class="mt-2 h-3 w-1/2 rounded bg-muted" />
      </div>
    </div>

    <!-- Empty state -->
    <EmptyState
      v-else-if="grouped.length === 0"
      :title="$t('opinions.index.emptyTitle')"
      :description="selectedField ? $t('opinions.index.emptyFieldDescription') : $t('opinions.index.emptyAllDescription')"
    />

    <!-- Grouped thumbnail grid -->
    <div v-else class="space-y-8">
      <section v-for="group in grouped" :key="group.id">
        <h2 class="mb-3 text-sm font-semibold uppercase tracking-wider text-muted-foreground">
          {{ group.name }}
          <span class="ms-1 text-xs font-normal opacity-60">
            · {{ $t('opinions.index.opinionCount', { count: group.opinions.length }, group.opinions.length) }}
          </span>
        </h2>

        <div class="op-grid">
          <router-link v-for="op in group.opinions" :key="op.id" :to="`/opinions/${op.id}`" class="op-card group">
            <!-- Thumbnail -->
            <div class="op-card__thumb">
              <img
                v-if="op.thumbnail_cid && thumbs[op.thumbnail_cid]"
                :src="thumbs[op.thumbnail_cid]"
                :alt="op.title"
                class="h-full w-full object-cover"
                loading="lazy"
              />
              <div v-else class="op-card__ph">
                <svg class="h-9 w-9 text-primary/60" fill="currentColor" viewBox="0 0 24 24"><path d="M8 5v14l11-7z" /></svg>
              </div>
              <span v-if="durationLabel(op.duration_seconds)" class="op-card__dur">
                {{ durationLabel(op.duration_seconds) }}
              </span>
            </div>

            <!-- Meta below -->
            <div class="mt-2.5 min-w-0">
              <div class="flex items-start gap-1.5">
                <h3 class="line-clamp-2 flex-1 text-sm font-semibold leading-snug text-foreground group-hover:text-primary">
                  {{ op.title }}
                </h3>
                <ProvenanceBadge :provenance="op.provenance" />
              </div>
              <div class="mt-1 truncate text-xs text-muted-foreground">
                {{ displayName(op.author_address) }}
              </div>
              <div class="text-xs text-muted-foreground/80">
                {{ formatDate(op.published_at) }}
              </div>
            </div>
          </router-link>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.op-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 1.25rem 1rem;
}
.op-card {
  display: block;
  text-decoration: none;
  color: inherit;
}
.op-card__thumb {
  position: relative;
  aspect-ratio: 16 / 9;
  border-radius: 0.75rem;
  overflow: hidden;
  background: color-mix(in srgb, var(--app-primary) 8%, var(--app-card));
  transition: box-shadow 0.15s ease, transform 0.15s ease;
}
.op-card:hover .op-card__thumb {
  transform: translateY(-2px);
  box-shadow: 0 12px 28px -14px rgb(0 0 0 / 0.4);
}
.op-card__ph {
  display: flex;
  height: 100%;
  width: 100%;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, color-mix(in srgb, var(--app-primary) 14%, transparent), transparent);
}
.op-card__dur {
  position: absolute;
  inset-inline-end: 0.375rem;
  inset-block-end: 0.375rem;
  padding: 0.05rem 0.35rem;
  border-radius: 0.25rem;
  background: rgb(0 0 0 / 0.72);
  color: #fff;
  font-size: 0.6875rem;
  font-variant-numeric: tabular-nums;
}
</style>
