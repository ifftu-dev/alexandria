<script setup lang="ts">
import { computed, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import type { LearningRecordUsage } from '@/types'

const props = defineProps<{ usage: LearningRecordUsage }>()

const { t, locale } = useI18n()
const headingId = useId()

/** Binary units, as the limit is defined: 16 MiB reads "16 MB". Unit names come from the catalogue. */
function size(bytes: number): string {
  const number = (value: number) => new Intl.NumberFormat(locale.value, { maximumFractionDigits: 1 }).format(value)
  if (bytes < 1024) return t('dashboard.activityLog.size.bytes', { size: number(bytes) })
  if (bytes < 1024 * 1024) return t('dashboard.activityLog.size.kilobytes', { size: number(bytes / 1024) })
  return t('dashboard.activityLog.size.megabytes', { size: number(bytes / (1024 * 1024)) })
}

const fraction = computed(() => props.usage.limit_bytes > 0
  ? Math.min(1, props.usage.bytes / props.usage.limit_bytes)
  : 0)
</script>

<template>
  <section class="card p-5" :aria-labelledby="headingId">
    <div class="flex flex-wrap items-baseline justify-between gap-2">
      <h2 :id="headingId" class="text-sm font-semibold text-foreground">{{ t('dashboard.activityLog.size.label') }}</h2>
      <p class="text-sm text-muted-foreground">
        {{ t('dashboard.activityLog.size.records', { count: usage.records }, usage.records) }}
        ·
        {{ t('dashboard.activityLog.size.used', { used: size(usage.bytes), limit: size(usage.limit_bytes) }) }}
      </p>
    </div>
    <div
      role="progressbar"
      class="mt-3 h-2 w-full overflow-hidden rounded-full bg-muted"
      :aria-label="t('dashboard.activityLog.size.label')"
      :aria-valuenow="usage.bytes"
      aria-valuemin="0"
      :aria-valuemax="usage.limit_bytes"
      :aria-valuetext="t('dashboard.activityLog.size.used', { used: size(usage.bytes), limit: size(usage.limit_bytes) })"
    >
      <div class="h-full rounded-full bg-primary" :style="{ width: `${fraction * 100}%` }" />
    </div>
    <p v-if="usage.interaction_records > 0" class="mt-3 text-xs text-muted-foreground">
      {{ t('dashboard.activityLog.size.interactions', { count: usage.interaction_records }) }}
    </p>
    <p class="mt-1 text-xs text-muted-foreground">{{ t('dashboard.activityLog.size.explain') }}</p>
  </section>
</template>
