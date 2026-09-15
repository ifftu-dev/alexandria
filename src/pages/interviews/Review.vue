<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useInterviews } from '@/composables/useInterviews'
import { AppAlert, AppBadge, AppButton, AppTextarea } from '@/components/ui'

const route = useRoute()
const router = useRouter()
const interviewId = computed(() => String(route.params.id))
const { activeInterview, loading, error, getInterview, generateSummary, saveReview } = useInterviews()

const activeTab = ref<'summary' | 'transcript' | 'privacy'>('summary')
const summary = ref('')
const conclusion = ref('')
const saved = ref(false)
const anonymizeExport = ref(true)
const includePrivateNotes = ref(false)

const bundle = computed(() => activeInterview.value)
const coveredCount = computed(() => bundle.value?.criteria.filter(item => item.status === 'covered').length ?? 0)
const partialCount = computed(() => bundle.value?.criteria.filter(item => item.status === 'partial').length ?? 0)

onMounted(async () => {
  const loaded = await getInterview(interviewId.value)
  if (!loaded) return
  summary.value = loaded.session.summary ?? await generateSummary(interviewId.value)
  conclusion.value = loaded.session.conclusion ?? ''
})

async function save() {
  await saveReview(interviewId.value, summary.value, conclusion.value)
  saved.value = true
  window.setTimeout(() => { saved.value = false }, 2000)
}

function formatTime(ms: number) {
  const total = Math.floor(ms / 1000)
  return `${String(Math.floor(total / 60)).padStart(2, '0')}:${String(total % 60).padStart(2, '0')}`
}

function exportJson() {
  if (!bundle.value) return
  const participantLabels = new Map(bundle.value.participants.map(item => [
    item.display_name,
    anonymizeExport.value ? item.pseudonym : item.display_name,
  ]))
  const payload = {
    schema_version: 1,
    exported_at: new Date().toISOString(),
    interview: {
      title: bundle.value.session.title,
      objective: bundle.value.session.objective,
      status: bundle.value.session.status,
      started_at: bundle.value.session.started_at,
      ended_at: bundle.value.session.ended_at,
      retention_expires_at: bundle.value.session.expires_at,
    },
    criteria: bundle.value.criteria.map(item => ({ label: item.label, status: item.status, notes: item.notes })),
    transcript: bundle.value.transcript.map(item => ({
      at_ms: item.start_ms,
      speaker: participantLabels.get(item.speaker_label) ?? item.speaker_label,
      text: item.text,
      source: item.source,
    })),
    summary: summary.value,
    conclusion: conclusion.value,
    private_notes: includePrivateNotes.value ? bundle.value.notes.map(item => item.text) : undefined,
  }
  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = `interview-${interviewId.value.slice(0, 8)}.json`
  anchor.click()
  URL.revokeObjectURL(url)
}
</script>

<template>
  <div v-if="bundle" class="mx-auto max-w-6xl space-y-5 pb-10">
    <header class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
      <div class="flex items-start gap-3">
        <button class="mt-0.5 rounded-lg p-2 text-muted-foreground hover:bg-muted hover:text-foreground" :title="$t('interviews.review.back')" @click="router.push('/interviews')">
          <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" /></svg>
        </button>
        <div>
          <div class="flex flex-wrap items-center gap-2"><AppBadge variant="secondary">{{ $t('interviews.review.label') }}</AppBadge><span class="text-xs text-muted-foreground">{{ $t('interviews.review.localDraft') }}</span></div>
          <h1 class="mt-2 text-2xl font-semibold tracking-tight text-foreground">{{ bundle.session.title }}</h1>
          <p class="mt-1 text-sm text-muted-foreground">{{ $t('interviews.review.subtitle') }}</p>
        </div>
      </div>
      <div class="flex gap-2"><AppButton variant="outline" @click="activeTab = 'privacy'">{{ $t('interviews.review.exportPrivacy') }}</AppButton><AppButton :loading="loading" @click="save">{{ saved ? $t('interviews.review.saved') : $t('interviews.review.save') }}</AppButton></div>
    </header>

    <AppAlert v-if="error" variant="error">{{ error }}</AppAlert>

    <section class="grid gap-3 sm:grid-cols-4">
      <div class="card p-4"><p class="text-xs text-muted-foreground">{{ $t('interviews.review.segments') }}</p><p class="mt-2 text-xl font-semibold tabular-nums">{{ bundle.transcript.length }}</p></div>
      <div class="card p-4"><p class="text-xs text-muted-foreground">{{ $t('interviews.review.criteriaCovered') }}</p><p class="mt-2 text-xl font-semibold tabular-nums">{{ coveredCount }}/{{ bundle.criteria.length }}</p></div>
      <div class="card p-4"><p class="text-xs text-muted-foreground">{{ $t('interviews.review.partialEvidence') }}</p><p class="mt-2 text-xl font-semibold tabular-nums">{{ partialCount }}</p></div>
      <div class="card border-primary/20 bg-primary/[0.035] p-4"><p class="text-xs text-primary">{{ $t('interviews.review.deletesLocally') }}</p><p class="mt-2 text-sm font-semibold">{{ new Date(bundle.session.expires_at).toLocaleDateString() }}</p></div>
    </section>

    <nav class="flex gap-1 border-b border-border" :aria-label="$t('interviews.review.sectionsLabel')">
      <button v-for="tab in (['summary', 'transcript', 'privacy'] as const)" :key="tab" class="border-b-2 px-4 py-2.5 text-sm font-medium capitalize transition-colors" :class="activeTab === tab ? 'border-primary text-primary' : 'border-transparent text-muted-foreground hover:text-foreground'" @click="activeTab = tab">{{ $t(`interviews.review.tabs.${tab}`) }}</button>
    </nav>

    <div v-if="activeTab === 'summary'" class="grid gap-5 lg:grid-cols-[minmax(0,1fr)_22rem]">
      <section class="card p-5">
        <div class="flex items-center justify-between gap-3"><div><h2 class="text-sm font-semibold text-foreground">{{ $t('interviews.review.summaryTitle') }}</h2><p class="mt-1 text-xs text-muted-foreground">{{ $t('interviews.review.summaryHint') }}</p></div><AppButton size="xs" variant="outline" @click="generateSummary(interviewId).then(value => { summary = value })">{{ $t('interviews.review.regenerate') }}</AppButton></div>
        <div class="mt-4"><AppTextarea v-model="summary" :rows="18" /></div>
        <div class="mt-5"><AppTextarea v-model="conclusion" :label="$t('interviews.review.conclusionLabel')" :placeholder="$t('interviews.review.conclusionPlaceholder')" :rows="5" /></div>
      </section>

      <aside class="space-y-4">
        <div class="card p-4">
          <h2 class="text-sm font-semibold text-foreground">{{ $t('interviews.review.coverage') }}</h2>
          <div class="mt-3 space-y-2">
            <div v-for="criterion in bundle.criteria" :key="criterion.id" class="flex items-center gap-2 rounded-lg border border-border px-3 py-2">
              <span class="h-2 w-2 rounded-full" :class="criterion.status === 'covered' ? 'bg-success' : criterion.status === 'partial' ? 'bg-warning' : 'bg-border'" />
              <span class="min-w-0 flex-1 truncate text-xs text-foreground">{{ criterion.label }}</span>
              <span class="text-[0.62rem] capitalize text-muted-foreground">{{ criterion.status.replace('_', ' ') }}</span>
            </div>
          </div>
        </div>
        <div class="card p-4"><h2 class="text-sm font-semibold text-foreground">{{ $t('interviews.review.discipline') }}</h2><ul class="mt-3 space-y-2 text-xs leading-relaxed text-muted-foreground"><li>• {{ $t('interviews.review.disciplineTimestamp') }}</li><li>• {{ $t('interviews.review.disciplineSentinel') }}</li><li>• {{ $t('interviews.review.disciplinePii') }}</li><li>• {{ $t('interviews.review.disciplineSharing') }}</li></ul></div>
      </aside>
    </div>

    <section v-else-if="activeTab === 'transcript'" class="card overflow-hidden">
      <div class="border-b border-border p-4"><h2 class="text-sm font-semibold text-foreground">{{ $t('interviews.review.transcriptTitle') }}</h2><p class="mt-1 text-xs text-muted-foreground">{{ $t('interviews.review.transcriptHint', { count: bundle.transcript.length }) }}</p></div>
      <div class="divide-y divide-border">
        <article v-for="segment in bundle.transcript" :key="segment.id" class="grid gap-2 p-4 sm:grid-cols-[5rem_10rem_minmax(0,1fr)]">
          <span class="font-mono text-xs text-muted-foreground">{{ formatTime(segment.start_ms) }}</span>
          <div><p class="text-xs font-semibold text-primary">{{ segment.speaker_label }}</p><p class="mt-0.5 text-[0.62rem] uppercase tracking-wide text-muted-foreground">{{ segment.source.replace('_', ' ') }}</p></div>
          <p class="text-sm leading-relaxed text-foreground">{{ segment.text }}</p>
        </article>
        <p v-if="bundle.transcript.length === 0" class="p-8 text-center text-sm text-muted-foreground">{{ $t('interviews.review.transcriptEmpty') }}</p>
      </div>
    </section>

    <section v-else class="grid gap-5 lg:grid-cols-[minmax(0,1fr)_22rem]">
      <div class="card p-5">
        <h2 class="text-sm font-semibold text-foreground">{{ $t('interviews.review.exportTitle') }}</h2>
        <p class="mt-2 text-sm leading-relaxed text-muted-foreground">{{ $t('interviews.review.exportHint') }}</p>
        <div class="mt-5 space-y-3">
          <label class="flex items-start gap-3 rounded-xl border border-border p-4"><input v-model="anonymizeExport" type="checkbox" class="mt-0.5 accent-primary" /><span><strong class="block text-sm text-foreground">{{ $t('interviews.review.pseudonyms') }}</strong><small class="mt-1 block text-xs text-muted-foreground">{{ $t('interviews.review.pseudonymsHint') }}</small></span></label>
          <label class="flex items-start gap-3 rounded-xl border border-border p-4"><input v-model="includePrivateNotes" type="checkbox" class="mt-0.5 accent-primary" /><span><strong class="block text-sm text-foreground">{{ $t('interviews.review.includeNotes') }}</strong><small class="mt-1 block text-xs text-muted-foreground">{{ $t('interviews.review.includeNotesHint') }}</small></span></label>
        </div>
        <AppButton class="mt-5" @click="exportJson">{{ $t('interviews.review.exportJson') }}</AppButton>
      </div>
      <aside class="card p-5"><h2 class="text-sm font-semibold text-foreground">{{ $t('interviews.review.retention') }}</h2><p class="mt-2 text-sm text-muted-foreground">{{ $t('interviews.review.retentionHint', { count: bundle.session.retention_days }) }}</p><div class="mt-4 rounded-lg bg-muted p-3"><p class="text-[0.65rem] uppercase tracking-wide text-muted-foreground">{{ $t('interviews.review.expiry') }}</p><p class="mt-1 font-mono text-xs text-foreground">{{ bundle.session.expires_at }}</p></div><p class="mt-4 text-xs leading-relaxed text-muted-foreground">{{ $t('interviews.review.expirySweep') }}</p></aside>
    </section>
  </div>
  <div v-else class="grid min-h-80 place-items-center"><p class="text-sm text-muted-foreground">{{ loading ? $t('interviews.review.loading') : $t('interviews.common.notFound') }}</p></div>
</template>
