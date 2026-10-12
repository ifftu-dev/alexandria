<script setup lang="ts">
// What a panelist sees of the candidate's integrity check: whether the
// organisation asked, and each result the candidate's app sent. Their app
// reports it, so a flag is a reason to ask, not a verdict.
import { computed } from 'vue'
import type { PanelIntegrity, PanelIntegrityResult } from '@/composables/usePanel'

const props = defineProps<{ integrity: PanelIntegrity }>()

const SEVERITY_RANK = { critical: 0, warning: 1, info: 2 } as const

const latest = computed(() => props.integrity.results[props.integrity.results.length - 1] ?? null)

function flagKey(flag: string) {
  const camel = flag.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase())
  return `sentinel.flags.items.${camel}.description`
}

function minutes(r: PanelIntegrityResult): number | null {
  if (!r.startedAt || !r.endedAt) return null
  const ms = Date.parse(r.endedAt) - Date.parse(r.startedAt)
  return Number.isFinite(ms) && ms >= 0 ? Math.round(ms / 60000) : null
}

function sorted(r: PanelIntegrityResult) {
  return [...r.flags].sort((a, b) => SEVERITY_RANK[a.severity] - SEVERITY_RANK[b.severity] || b.count - a.count)
}
</script>

<template>
  <div v-if="integrity.requested" class="rounded-lg border border-border p-3" data-testid="panel-integrity">
    <div class="flex items-center justify-between gap-2">
      <p class="text-xs font-semibold text-foreground">{{ $t('interviews.panel.integrity.title') }}</p>
      <span
        class="rounded-full px-2 py-0.5 text-[0.65rem] font-medium"
        :class="!latest ? 'bg-muted text-muted-foreground' : latest.outcome === 'clear' ? 'bg-success/10 text-success' : 'bg-warning/15 text-warning'"
      >
        {{ !latest ? $t('interviews.panel.integrity.noResult') : latest.outcome === 'clear' ? $t('interviews.panel.integrity.clear') : $t('interviews.panel.integrity.flagged') }}
      </span>
    </div>
    <p v-if="!latest" class="mt-1 text-xs text-muted-foreground">{{ $t('interviews.panel.integrity.waiting') }}</p>
    <div v-for="(r, i) in integrity.results" :key="r.receivedAt" class="mt-2 text-xs text-muted-foreground" data-testid="panel-integrity-result">
      <p v-if="integrity.results.length > 1" class="font-medium text-foreground">{{ $t('interviews.panel.integrity.run', { n: i + 1, total: integrity.results.length }) }}</p>
      <p>
        <template v-if="r.score !== null">{{ $t('interviews.panel.integrity.score', { score: r.score }) }} · </template>
        {{ $t('interviews.panel.integrity.ran', { minutes: minutes(r) ?? '—' }) }} ·
        {{ r.camera ? $t('interviews.panel.integrity.cameraOn', { percent: r.cameraCoverage }) : $t('interviews.panel.integrity.cameraOff') }}
      </p>
      <ul v-if="r.flags.length" class="mt-1 space-y-0.5">
        <li v-for="f in sorted(r)" :key="f.flag" class="text-foreground">
          {{ $te(flagKey(f.flag)) ? $t(flagKey(f.flag)) : f.flag }}
          <span class="text-muted-foreground">× {{ f.count }} · {{ f.severity }}</span>
        </li>
      </ul>
      <p v-else class="mt-1">{{ $t('interviews.panel.integrity.noFlags') }}</p>
    </div>
    <p class="mt-2 text-[0.7rem] leading-relaxed text-muted-foreground">{{ $t('interviews.panel.integrity.caveat') }}</p>
  </div>
</template>
