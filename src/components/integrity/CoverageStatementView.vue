<script setup lang="ts">
// A coverage statement as the app bundles it: what a session under one trust
// profile and verification rung checked, what it did not, the platform
// limits and how the signals were measured. The lines are the statement's
// own fixed text (its digest covers them), so they are quoted, not
// translated; the headings around them are localised.
import type { CoverageStatement } from '@/types'

defineProps<{
  statement: CoverageStatement | null
  /** Shown when the statement could not be resolved, so it can be looked up elsewhere. */
  digest?: string | null
}>()

const rate = (value: number) => value.toFixed(2)
</script>

<template>
  <div class="space-y-3 text-sm" data-testid="coverage-statement">
    <template v-if="statement">
      <div>
        <p class="text-xs font-medium uppercase tracking-wide text-muted-foreground">{{ $t('sentinel.trust.coverage.checked') }}</p>
        <ul class="mt-1 list-disc space-y-1 ps-5 text-foreground">
          <li v-for="line in statement.checked" :key="line">{{ line }}</li>
        </ul>
      </div>
      <div>
        <p class="text-xs font-medium uppercase tracking-wide text-muted-foreground">{{ $t('sentinel.trust.coverage.notChecked') }}</p>
        <ul class="mt-1 list-disc space-y-1 ps-5 text-foreground">
          <li v-for="line in statement.not_checked" :key="line">{{ line }}</li>
        </ul>
      </div>
      <div>
        <p class="text-xs font-medium uppercase tracking-wide text-muted-foreground">{{ $t('sentinel.trust.coverage.ceilings') }}</p>
        <ul class="mt-1 list-disc space-y-1 ps-5 text-muted-foreground">
          <li v-for="line in statement.platform_ceilings" :key="line">{{ line }}</li>
        </ul>
      </div>
      <div>
        <p class="text-xs font-medium uppercase tracking-wide text-muted-foreground">{{ $t('sentinel.trust.coverage.measurement') }}</p>
        <p class="mt-1 text-muted-foreground">{{ statement.measurement }}</p>
        <template v-if="statement.certification">
          <p class="mt-1 text-muted-foreground">
            {{ $t('sentinel.trust.coverage.certified', { version: statement.certification.version, date: statement.certification.date }) }}
          </p>
          <ul class="mt-1 space-y-0.5 text-xs text-muted-foreground">
            <li v-for="(rates, signal) in statement.certified_signals" :key="signal">
              {{ $t('sentinel.trust.coverage.rates', { signal, precision: rate(rates.precision), recall: rate(rates.recall) }) }}
            </li>
          </ul>
        </template>
      </div>
      <p class="text-xs text-muted-foreground">{{ $t('sentinel.trust.coverage.quoted') }}</p>
    </template>
    <template v-else>
      <p class="text-muted-foreground">{{ $t('sentinel.trust.coverage.unresolved') }}</p>
      <p v-if="digest" class="flex items-center justify-between gap-3 text-xs">
        <span class="text-muted-foreground">{{ $t('sentinel.trust.coverage.digest') }}</span>
        <span class="truncate font-mono text-muted-foreground">{{ digest }}</span>
      </p>
    </template>
  </div>
</template>
