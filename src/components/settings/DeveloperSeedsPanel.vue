<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { useProfiles } from '@/composables/useProfiles'
import { AppButton } from '@/components/ui'
import type { SeedCatalog, SeedResource, SeedResult, SeedResetPlan } from '@/types'

const { invoke } = useLocalApi()
const { profiles, activeProfileId } = useProfiles()
const profileName = computed(() => profiles.value.find(p => p.id === activeProfileId.value)?.display_name ?? 'Current profile')
const catalog = ref<SeedCatalog>({ enabled: false, resources: [] })
const selected = ref<string[]>([])
const category = ref('All')
const search = ref('')
const review = ref<SeedResource[] | null>(null)
const results = ref<SeedResult[]>([])
const resetReview = ref<SeedResetPlan | null>(null)
const resetConfirmed = ref(false)
const busy = ref(false)
const error = ref('')
const scrollArea = ref<HTMLElement | null>(null)
const reviewHeading = ref<HTMLElement | null>(null)
const inReview = computed(() => review.value !== null || resetReview.value !== null)
const allVisibleSelected = computed(() => visible.value.length > 0 && visible.value.every(r => selected.value.includes(r.id)))
watch([review, resetReview, results], async () => {
  await nextTick()
  scrollArea.value?.scrollTo?.({ top: 0 })
  if (inReview.value) reviewHeading.value?.focus({ preventScroll: true })
})
const categories = computed(() => ['All', ...new Set(catalog.value.resources.map(r => r.category))])
const visible = computed(() => catalog.value.resources.filter(r => (category.value === 'All' || r.category === category.value) && `${r.title} ${r.description}`.toLowerCase().includes(search.value.toLowerCase())))
const additions = computed(() => review.value?.filter(r => !r.installed).length ?? 0)
const resultCounts = computed(() => ({ removed: results.value.filter(r => r.status === 'removed').length, added: results.value.filter(r => r.status === 'added').length, kept: results.value.filter(r => r.status === 'kept').length, failed: results.value.filter(r => r.status === 'failed').length }))
const title = (id: string) => catalog.value.resources.find(r => r.id === id)?.title ?? id
// Discussion and proposal seeds are local drafts the composer can open. The row id
// mirrors the backend: `draft:{key}` → `{key}`, `proposal:{key}` → `proposal-{key}`.
const draftKey = (id: string) => id.startsWith('draft:') ? id.slice(6) : id.startsWith('proposal:') ? `proposal-${id.slice(9)}` : null
async function load() { catalog.value = await invoke<SeedCatalog>('dev_seed_catalog') }
onMounted(async () => { try { await load() } catch (e) { error.value = String(e) } })
function toggleVisible() {
  const all = visible.value.every(r => selected.value.includes(r.id))
  const ids = new Set(selected.value)
  for (const r of visible.value) { if (all) ids.delete(r.id); else ids.add(r.id) }
  selected.value = [...ids]
}
async function prepare(everything = false) {
  busy.value = true; error.value = ''; results.value = []; resetReview.value = null
  if (everything) selected.value = catalog.value.resources.map(r => r.id)
  try { review.value = await invoke<SeedResource[]>('dev_seed_plan', { selected: selected.value }) }
  catch (e) { error.value = String(e) }
  finally { busy.value = false }
}
async function run() {
  busy.value = true; error.value = ''
  try {
    results.value = await invoke<SeedResult[]>('dev_seed_run', { selected: selected.value })
    review.value = null
    await load()
  } catch (e) { error.value = String(e) }
  finally { busy.value = false }
}
async function prepareReset(everything = false) {
  busy.value = true; error.value = ''; results.value = []; review.value = null; resetConfirmed.value = false
  if (everything) selected.value = catalog.value.resources.filter(r => r.installed).map(r => r.id)
  try { resetReview.value = await invoke<SeedResetPlan>('dev_seed_reset_plan', { selected: selected.value }) }
  catch (e) { error.value = String(e) }
  finally { busy.value = false }
}
async function reset() {
  if (!resetReview.value || !resetConfirmed.value) return
  busy.value = true; error.value = ''
  try {
    results.value = await invoke<SeedResult[]>('dev_seed_reset_run', { selected: selected.value, token: resetReview.value.token })
    resetReview.value = null; resetConfirmed.value = false
    await load()
  } catch (e) {
    error.value = String(e)
    resetReview.value = null; resetConfirmed.value = false
  } finally { busy.value = false }
}
</script>

<template>
  <section v-if="catalog.enabled" class="seed-panel" :aria-busy="busy">
    <div ref="scrollArea" class="seed-scroll overflow-y-auto">
      <header class="space-y-3">
        <div>
          <h2 class="text-lg font-semibold">Test data</h2>
          <p class="mt-1 text-sm text-muted-foreground">For <strong class="text-foreground">{{ profileName }}</strong> · {{ catalog.resources.length }} resources</p>
        </div>
        <template v-if="!inReview">
          <p class="text-sm leading-relaxed text-muted-foreground">Add missing resources. Keep existing work.</p>
          <div class="grid grid-cols-2 gap-2 md:flex md:flex-wrap">
            <AppButton variant="secondary" :disabled="busy" @click="prepare(true)">Seed everything</AppButton>
            <AppButton variant="outline" :disabled="busy || !catalog.resources.some(r => r.installed)" @click="prepareReset(true)">Reset all seeds</AppButton>
          </div>
        </template>
      </header>

      <p v-if="error" role="alert" class="rounded-xl border border-red-500/30 bg-red-500/5 p-3 text-sm text-red-500">{{ error }}</p>
      <div v-if="results.length" role="status" class="rounded-xl border border-border p-4 space-y-3">
        <p class="text-sm font-semibold">{{ resultCounts.added }} added · {{ resultCounts.kept }} kept · {{ resultCounts.failed }} failed<span v-if="resultCounts.removed"> · {{ resultCounts.removed }} removed</span></p>
        <details :open="resultCounts.failed > 0">
          <summary class="flex cursor-pointer items-center text-sm text-primary">View results</summary>
          <ul class="mt-2 divide-y divide-border text-sm">
            <li v-for="r in results" :key="r.id" class="py-3 space-y-1">
              <div class="flex items-start justify-between gap-3"><span>{{ title(r.id) }}</span><span class="shrink-0 text-muted-foreground">{{ r.status }}</span></div>
              <p v-if="r.error" :class="r.status === 'failed' ? 'text-red-500' : 'text-muted-foreground'">{{ r.error }}</p>
              <router-link v-if="draftKey(r.id) && (r.status === 'added' || r.status === 'kept')" :to="{ path: '/discussions/new', query: { seedDraft: draftKey(r.id) } }" class="inline-flex min-h-11 items-center text-primary underline">Open draft</router-link>
            </li>
          </ul>
        </details>
        <AppButton v-if="resultCounts.failed" variant="outline" :disabled="busy" @click="prepare()">Review and retry</AppButton>
      </div>

      <div v-if="review" class="space-y-4">
        <div>
          <h3 ref="reviewHeading" tabindex="-1" class="text-lg font-semibold outline-none">Review {{ review.length }} resources</h3>
          <p class="mt-1 text-sm text-muted-foreground">{{ additions }} to add · {{ review.length - additions }} already present</p>
        </div>
        <p class="rounded-xl bg-muted/40 p-3 text-sm leading-relaxed text-muted-foreground">Required dependencies are included. No credentials, attempts, votes, or public posts will be created. Discussion drafts stay local until you publish them.</p>
        <ul class="divide-y divide-border rounded-xl border border-border px-4 text-sm">
          <li v-for="r in review" :key="r.id" class="flex items-start justify-between gap-3 py-3">
            <div class="min-w-0"><span>{{ r.title }}</span><p v-if="!selected.includes(r.id)" class="mt-1 text-xs text-primary">required dependency</p></div>
            <span class="shrink-0 text-xs text-muted-foreground">{{ r.installed ? 'Keep existing' : 'Add' }}</span>
          </li>
        </ul>
      </div>
      <div v-else-if="resetReview" class="space-y-4">
        <div>
          <h3 ref="reviewHeading" tabindex="-1" class="text-lg font-semibold outline-none">Review seed reset</h3>
          <p class="mt-2 text-sm leading-relaxed text-muted-foreground">Remove these local seeds and the linked data listed below, including edits and progress. You can seed them again afterward.</p>
        </div>
        <details class="rounded-xl border border-border px-3 py-2 text-sm">
          <summary class="flex cursor-pointer items-center font-medium">What stays on your profile</summary>
          <p class="pb-2 leading-relaxed text-muted-foreground">Issued credentials, completion evidence, integrity history, learner goals, and published P2P copies remain. Cached media and plugin files are retained; resources still needed elsewhere are kept.</p>
        </details>
        <ul class="divide-y divide-border rounded-xl border border-border px-4">
          <li v-for="r in resetReview.resources" :key="r.id" class="py-3 text-sm">
            <div class="flex items-start justify-between gap-3"><span class="font-medium">{{ r.title }}</span><span :class="r.can_reset ? 'text-red-500' : 'text-muted-foreground'">{{ r.can_reset ? 'Remove' : 'Keep' }}</span></div>
            <p v-if="r.reason" class="mt-1 text-muted-foreground">{{ r.reason }}</p>
            <ul v-else class="mt-2 space-y-1 text-muted-foreground"><li v-for="effect in r.effects" :key="effect.label">{{ effect.count }} × {{ effect.label }}</li></ul>
          </li>
        </ul>
        <label class="flex cursor-pointer items-start gap-3 rounded-xl border border-red-500/30 bg-red-500/5 p-4 text-sm leading-relaxed">
          <input v-model="resetConfirmed" type="checkbox" :disabled="busy" class="seed-checkbox mt-0.5" />
          <span>I understand that the listed local data, including edits and progress, will be removed.</span>
        </label>
      </div>
      <template v-else>
        <div class="seed-filters space-y-3">
          <input v-model="search" type="search" placeholder="Search resources" aria-label="Search resources" class="input w-full" :disabled="busy" />
          <div class="flex items-center gap-3">
            <select v-model="category" aria-label="Resource category" class="input min-w-0 flex-1" :disabled="busy">
              <option v-for="c in categories" :key="c" :value="c">{{ c === 'All' ? 'All categories' : c }}</option>
            </select>
            <button :disabled="busy || !visible.length" class="min-h-11 shrink-0 text-sm font-medium text-primary disabled:opacity-50" @click="toggleVisible">{{ allVisibleSelected ? 'Deselect visible' : 'Select visible' }}</button>
          </div>
          <p class="text-xs text-muted-foreground">{{ visible.length }} resources<span v-if="search || category !== 'All'"> matching your filters</span></p>
        </div>
        <div v-if="visible.length" class="divide-y divide-border rounded-xl border border-border">
          <div v-for="r in visible" :key="r.id" class="relative">
            <label class="flex cursor-pointer items-start gap-3 p-4 hover:bg-muted/30">
              <input v-model="selected" type="checkbox" :value="r.id" :disabled="busy" class="seed-checkbox mt-0.5" />
              <span class="min-w-0 flex-1">
                <span class="block text-sm font-medium leading-relaxed">{{ r.title }}</span>
                <span class="mt-1 block text-xs text-muted-foreground">{{ r.category }}<span v-if="r.installed"> · Already present</span></span>
                <span class="mt-2 block text-sm leading-relaxed text-muted-foreground">{{ r.description }}</span>
              </span>
            </label>
            <router-link v-if="r.installed && draftKey(r.id)" :to="{ path: '/discussions/new', query: { seedDraft: draftKey(r.id) } }" class="ms-12 mb-2 inline-flex min-h-11 items-center px-2 text-sm text-primary underline">Open draft</router-link>
          </div>
        </div>
        <div v-else class="rounded-xl border border-dashed border-border p-6 text-center text-sm text-muted-foreground">
          <p>No resources match these filters.</p>
          <AppButton variant="ghost" class="mt-2" @click="search = ''; category = 'All'">Clear filters</AppButton>
        </div>
      </template>
    </div>

    <footer class="seed-footer">
      <template v-if="review">
        <AppButton class="w-full" :loading="busy" @click="run">Seed these resources</AppButton>
        <AppButton variant="ghost" class="w-full" :disabled="busy" @click="review = null">Back to selection</AppButton>
      </template>
      <template v-else-if="resetReview">
        <AppButton variant="danger" class="w-full" :loading="busy" :disabled="!resetConfirmed || !resetReview.resources.some(r => r.can_reset)" @click="reset">Remove these seeds</AppButton>
        <AppButton variant="ghost" class="w-full" :disabled="busy" @click="resetReview = null">Back to selection</AppButton>
      </template>
      <template v-else>
        <div class="flex min-h-8 items-center justify-between gap-2 text-sm">
          <span aria-live="polite">{{ selected.length }} selected<span class="text-muted-foreground"> across categories</span></span>
          <button v-if="selected.length" class="min-h-11 text-primary" :disabled="busy" @click="selected = []">Clear</button>
        </div>
        <div class="grid grid-cols-2 gap-2 md:flex md:flex-wrap">
          <AppButton variant="outline" :disabled="busy || !selected.length" @click="prepareReset()">Reset selected</AppButton>
          <AppButton :loading="busy" :disabled="!selected.length" @click="prepare()">Review selection</AppButton>
        </div>
      </template>
    </footer>
  </section>
  <p v-else-if="error" role="alert" class="py-4 text-red-500">{{ error }}</p>
</template>

<style scoped>
.seed-panel { display: flex; flex: 1; flex-direction: column; min-height: 0; min-width: 0; }
.seed-scroll { flex: 1; min-height: 0; overscroll-behavior: contain; padding-block: 1rem; padding-inline: 1px; }
.seed-scroll > * + * { margin-top: 1.25rem; }
.seed-footer { flex-shrink: 0; border-top: 1px solid var(--app-border); background: var(--app-background); padding-block: 0.5rem 0.75rem; }
.seed-scroll h3:focus { box-shadow: none; }
@media (min-width: 768px) {
  .seed-filters { display: grid; grid-template-columns: minmax(0, 1fr) minmax(16rem, 1fr); align-items: center; column-gap: 1rem; }
  .seed-filters > * { margin-top: 0; }
  .seed-filters > p { grid-column: 1 / -1; margin-top: 0.75rem; }
  .seed-footer > .btn { width: auto; margin-inline-end: 0.5rem; }
}
.seed-checkbox { width: 20px; height: 20px; flex-shrink: 0; accent-color: var(--app-primary); }
.seed-panel :deep(.btn), .seed-panel input:not([type="checkbox"]), .seed-panel select, .seed-panel summary { min-height: 44px; }
.seed-panel :deep(.btn) { white-space: normal; }
.seed-panel :is(button, input, select, summary):focus-visible { outline: 2px solid var(--app-primary); outline-offset: 2px; }
@media (max-width: 767px) { .seed-panel input:not([type="checkbox"]), .seed-panel select { font-size: 16px; } }
@media (max-height: 500px) { .seed-footer { padding-block: 0.25rem; } .seed-footer > .btn { width: auto; } }
</style>
