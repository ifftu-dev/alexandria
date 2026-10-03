<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
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
const categories = computed(() => ['All', ...new Set(catalog.value.resources.map(r => r.category))])
const visible = computed(() => catalog.value.resources.filter(r => (category.value === 'All' || r.category === category.value) && `${r.title} ${r.description}`.toLowerCase().includes(search.value.toLowerCase())))
const additions = computed(() => review.value?.filter(r => !r.installed).length ?? 0)
const resultCounts = computed(() => ({ removed: results.value.filter(r => r.status === 'removed').length, added: results.value.filter(r => r.status === 'added').length, kept: results.value.filter(r => r.status === 'kept').length, failed: results.value.filter(r => r.status === 'failed').length }))
const title = (id: string) => catalog.value.resources.find(r => r.id === id)?.title ?? id
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
  <section v-if="catalog.enabled" class="space-y-6">
    <div class="flex flex-wrap items-start justify-between gap-4">
      <div><p class="text-xs font-semibold uppercase tracking-wider text-primary">Developer · Test data</p><h2 class="mt-2 text-xl font-semibold">Build your testing workspace</h2><p class="mt-2 text-sm text-muted-foreground">Active profile: <strong class="text-foreground">{{ profileName }}</strong></p></div>
      <div class="flex flex-wrap gap-2"><AppButton variant="secondary" :disabled="busy" @click="prepare(true)">Seed everything</AppButton><AppButton variant="outline" :disabled="busy || !catalog.resources.some(r => r.installed)" @click="prepareReset(true)">Reset all seeds</AppButton></div>
    </div>
    <div class="rounded-xl border border-border bg-muted/30 p-4 text-sm text-muted-foreground">Seed missing resources or review a reset. Seeding preserves existing work and includes required dependencies. Discussion prompts stay local until you publish them with an accepted topic credential.</div>
    <p v-if="error" role="alert" class="text-sm text-red-500">{{ error }}</p>
    <div v-if="results.length" role="status" class="rounded-xl border border-border p-4 space-y-3">
      <h3 class="font-semibold">{{ resultCounts.added }} added · {{ resultCounts.kept }} kept · {{ resultCounts.failed }} failed<span v-if="resultCounts.removed"> · {{ resultCounts.removed }} removed</span></h3>
      <ul class="space-y-2 text-sm"><li v-for="r in results" :key="r.id" class="flex flex-wrap gap-2"><span>{{ title(r.id) }}</span><span class="text-muted-foreground">{{ r.status }}</span><span v-if="r.error" :class="r.status === 'failed' ? 'text-red-500' : 'text-muted-foreground'">{{ r.error }}</span><router-link v-if="r.id.startsWith('draft:') && (r.status === 'added' || r.status === 'kept')" :to="{ path: '/discussions/new', query: { seedDraft: r.id.slice(6) } }" class="text-primary underline">Open draft</router-link></li></ul>
      <AppButton v-if="resultCounts.failed" size="sm" :disabled="busy" @click="prepare()">Review and retry</AppButton>
    </div>
    <div v-if="review" class="rounded-xl border border-primary/30 bg-primary/5 p-5 space-y-4">
      <h3 class="font-semibold">Review {{ review.length }} resources</h3><p class="text-sm text-muted-foreground">{{ additions }} to add · {{ review.length - additions }} already present. No credentials, attempts, votes, or public posts will be created.</p>
      <ul class="max-h-80 overflow-y-auto space-y-2 pr-3 text-sm"><li v-for="r in review" :key="r.id" class="flex flex-wrap justify-between gap-2"><span>{{ r.title }} <span v-if="!selected.includes(r.id)" class="text-primary">· required dependency</span></span><span class="text-muted-foreground">{{ r.installed ? 'Keep existing' : 'Add' }}</span></li></ul>
      <div class="flex flex-wrap gap-3"><AppButton :disabled="busy" @click="run">{{ busy ? 'Seeding…' : 'Seed these resources' }}</AppButton><AppButton variant="secondary" :disabled="busy" @click="review = null">Back to selection</AppButton></div>
    </div>
    <div v-else-if="resetReview" class="rounded-xl border border-red-500/30 bg-red-500/5 p-5 space-y-4">
      <h3 class="font-semibold">Review seed reset</h3>
      <p class="text-sm text-muted-foreground">Remove selected local seeds, including the edits and linked data listed below. You can seed them again afterward. Issued credentials, completion evidence, integrity history, learner goals, and published P2P copies remain. Cached media and plugin files are retained; resources still needed elsewhere are kept.</p>
      <ul class="max-h-96 overflow-y-auto space-y-4 pr-3"><li v-for="r in resetReview.resources" :key="r.id" class="text-sm"><div class="font-medium">{{ r.title }} · {{ r.can_reset ? 'Remove' : 'Keep' }}</div><p v-if="r.reason" class="mt-1 text-muted-foreground">{{ r.reason }}</p><ul v-else class="mt-1 space-y-1 text-muted-foreground"><li v-for="effect in r.effects" :key="effect.label">{{ effect.count }} × {{ effect.label }}</li></ul></li></ul>
      <label class="flex items-start gap-2 text-sm"><input v-model="resetConfirmed" type="checkbox" :disabled="busy" class="mt-1" />I understand that the listed local data, including edits and progress, will be removed.</label>
      <div class="flex flex-wrap gap-3"><AppButton variant="danger" :disabled="busy || !resetConfirmed || !resetReview.resources.some(r => r.can_reset)" @click="reset">{{ busy ? 'Resetting…' : 'Remove these seeds' }}</AppButton><AppButton variant="secondary" :disabled="busy" @click="resetReview = null">Back to selection</AppButton></div>
    </div>
    <template v-else>
      <div class="flex flex-wrap gap-2" aria-label="Resource categories"><button v-for="c in categories" :key="c" class="rounded-lg px-3 py-2 text-sm" :class="category === c ? 'bg-primary/10 text-primary font-semibold' : 'bg-muted/40 text-muted-foreground'" :aria-pressed="category === c" @click="category = c">{{ c }}</button></div>
      <div class="flex flex-wrap items-center gap-3"><input v-model="search" type="search" placeholder="Search resources" aria-label="Search resources" class="input min-w-0 flex-1" /><button :disabled="busy || !visible.length" class="text-sm text-primary" @click="toggleVisible">{{ visible.length && visible.every(r => selected.includes(r.id)) ? 'Deselect visible' : 'Select visible' }}</button></div>
      <div class="divide-y divide-border rounded-xl border border-border overflow-hidden"><label v-for="r in visible" :key="r.id" class="flex items-start gap-3 p-4 cursor-pointer hover:bg-muted/30"><input v-model="selected" type="checkbox" :value="r.id" :disabled="busy" class="mt-1 accent-primary" /><span class="min-w-0 flex-1"><span class="flex flex-wrap items-center justify-between gap-2"><span class="font-medium text-sm">{{ r.title }}</span><span class="text-xs text-muted-foreground">{{ r.installed ? 'Already present' : r.category }}</span></span><span class="mt-1 block text-xs leading-relaxed text-muted-foreground">{{ r.description }}</span><router-link v-if="r.installed && r.id.startsWith('draft:')" :to="{ path: '/discussions/new', query: { seedDraft: r.id.slice(6) } }" class="mt-2 block text-xs text-primary underline" @click.stop>Open draft</router-link></span></label></div>
      <div class="sticky bottom-0 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-border bg-card p-4 shadow-sm"><span class="text-sm">{{ selected.length }} selected across all categories</span><div class="flex flex-wrap gap-2"><AppButton size="sm" variant="outline" :disabled="busy || !selected.length" @click="prepareReset()">Reset selected</AppButton><AppButton size="sm" :disabled="busy || !selected.length" @click="prepare()">{{ busy ? 'Preparing…' : 'Review selection' }}</AppButton></div></div>
    </template>
  </section>
  <p v-else-if="error" role="alert" class="text-red-500">{{ error }}</p>
</template>
