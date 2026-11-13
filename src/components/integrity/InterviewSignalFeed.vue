<script setup lang="ts">
// The panel's live view of the candidate's interview signals: when each
// answer started against the candidate's own earlier answers, and the cues
// just before it. The candidate's app reports these, so each line is a
// reason to ask, never a verdict.
//
// On a panel seat (an interview held in Alexandria) the panel can also mark
// an unscripted live segment done; the candidate's check records it as a
// live verification.
import { AppButton } from '@/components/ui'
import type { InterviewAnswerSignal } from '@/types'
import type { AnswerConcern } from '@/utils/sentinel/interviewFeed'

export interface SignalFeedEntry {
  key: string
  /** When it arrived, ms since the epoch. */
  at: number
  sender: string
  /** An answer's timing, or null for a second voice on its own. */
  answer: InterviewAnswerSignal | null
  concerns: AnswerConcern[]
}

defineProps<{
  entries: SignalFeedEntry[]
  /** A panel seat: offer the live-segment mark. */
  live?: boolean
  /** When the panel last marked a live segment done, ms since the epoch. */
  liveMarkedAt?: number | null
}>()
defineEmits<{ mark: []; live: [] }>()

const seconds = (ms: number) => (ms / 1000).toFixed(1)
const clock = (at: number) => new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
</script>

<template>
  <section data-testid="interview-signal-feed">
    <div class="mb-2 flex items-center justify-between gap-2">
      <h3 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{{ $t('interviews.signals.title') }}</h3>
      <AppButton size="xs" variant="outline" data-testid="mark-question" @click="$emit('mark')">{{ $t('interviews.signals.markQuestion') }}</AppButton>
    </div>
    <p class="mb-2 text-[0.68rem] leading-relaxed text-muted-foreground">{{ $t('interviews.signals.hint') }}</p>
    <div v-if="live" class="mb-2 rounded-lg border border-border bg-card px-2.5 py-2">
      <div class="flex items-center justify-between gap-2">
        <p class="text-[0.68rem] leading-relaxed text-muted-foreground">{{ $t('sentinel.trust.live.hint') }}</p>
        <AppButton size="xs" variant="outline" data-testid="mark-live-segment" @click="$emit('live')">{{ $t('sentinel.trust.live.mark') }}</AppButton>
      </div>
      <p v-if="liveMarkedAt" class="mt-1 text-[0.62rem] text-success">{{ $t('sentinel.trust.live.marked', { time: clock(liveMarkedAt) }) }}</p>
    </div>
    <ul class="space-y-1.5">
      <li
        v-for="entry in entries"
        :key="entry.key"
        class="rounded-lg border px-2.5 py-2 text-xs"
        :class="entry.concerns.length ? 'border-warning/40 bg-warning/5' : 'border-border bg-card'"
      >
        <div class="flex items-center justify-between gap-2">
          <span class="truncate font-medium text-foreground">{{ entry.sender }}</span>
          <span class="font-mono text-[0.62rem] text-muted-foreground">{{ clock(entry.at) }}</span>
        </div>
        <p v-if="entry.answer" class="mt-1 text-foreground">
          {{ $t('interviews.signals.answer', { latency: $t('interviews.signals.seconds', { value: seconds(entry.answer.latency_ms) }) }) }}
          <span class="text-muted-foreground">
            · {{ entry.answer.baseline_ms === null
              ? $t('interviews.signals.noBaseline')
              : $t('interviews.signals.baseline', { baseline: $t('interviews.signals.seconds', { value: seconds(entry.answer.baseline_ms) }) }) }}
          </span>
        </p>
        <p v-else class="mt-1 text-foreground">{{ $t('interviews.signals.secondVoice') }}</p>
        <ul v-if="entry.answer && entry.concerns.length" class="mt-1 flex flex-wrap gap-1">
          <li v-for="concern in entry.concerns" :key="concern" class="rounded-full bg-warning/15 px-2 py-0.5 text-[0.62rem] font-medium text-warning">
            {{ $t(`interviews.signals.concern.${concern}`) }}
          </li>
        </ul>
      </li>
      <li v-if="entries.length === 0" class="rounded-lg border border-dashed border-border p-3 text-xs leading-relaxed text-muted-foreground">
        {{ $t('interviews.signals.empty') }}
      </li>
    </ul>
  </section>
</template>
