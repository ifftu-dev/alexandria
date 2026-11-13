<script setup lang="ts">
// A sponsor's review queue for one role: finished sessions ranked by the
// backend, most suspicious first by fusion posterior (the combined weight of
// measured signals). Each raised signal shows the rates it was certified at,
// or is marked advisory: no measured error rate, no weight.
import { AppBadge, AppButton } from '@/components/ui'
import type { RoleSessionReview } from '@/types'

defineProps<{ reviews: RoleSessionReview[] }>()
defineEmits<{ use: [review: RoleSessionReview] }>()

const fixed = (value: number) => value.toFixed(2)
</script>

<template>
  <ul class="space-y-2">
    <li
      v-for="r in reviews"
      :key="r.integrity_session_id"
      class="rounded-md border border-border/70 p-2.5 text-xs"
      data-testid="session-review"
      :data-session="r.integrity_session_id"
    >
      <div class="flex flex-wrap items-center gap-2">
        <span class="font-mono text-muted-foreground">{{ r.subject_did.slice(0, 20) }}…</span>
        <AppBadge :variant="r.review.status === 'completed' || r.review.status === 'active' ? 'secondary' : 'warning'">{{ r.review.status }}</AppBadge>
        <span class="font-medium text-foreground">{{ $t('dashboard.sponsor.review.weight', { value: fixed(r.review.posterior_llr) }) }}</span>
        <span v-if="r.review.threshold !== null" class="text-muted-foreground">{{ $t('dashboard.sponsor.review.threshold', { value: fixed(r.review.threshold) }) }}</span>
        <span v-if="r.review.trust_profile" class="text-muted-foreground">{{ $t(`sentinel.trust.profile.${r.review.trust_profile}`) }}</span>
        <span class="text-muted-foreground">{{ $t(`sentinel.trust.rung.${r.review.rung}`) }}</span>
        <AppButton class="ms-auto" variant="ghost" size="sm" @click="$emit('use', r)">{{ $t('dashboard.sponsor.issue.attemptUse') }}</AppButton>
      </div>
      <p v-if="r.review.signals.length === 0" class="mt-1 text-muted-foreground">{{ $t('dashboard.sponsor.review.noSignals') }}</p>
      <ul v-else class="mt-1.5 space-y-1">
        <li v-for="s in r.review.signals" :key="s.signal" class="flex flex-wrap items-center gap-2">
          <span class="font-mono text-foreground">{{ s.signal }}</span>
          <span class="text-muted-foreground">{{ $t('dashboard.sponsor.review.windows', { count: s.windows }) }}</span>
          <span v-if="s.certified" class="text-muted-foreground">
            {{ $t('dashboard.sponsor.review.rates', { precision: fixed(s.certified.precision), recall: fixed(s.certified.recall) }) }}
          </span>
          <AppBadge v-if="s.advisory" variant="secondary">{{ $t('dashboard.sponsor.review.advisory') }}</AppBadge>
        </li>
      </ul>
    </li>
  </ul>
</template>
