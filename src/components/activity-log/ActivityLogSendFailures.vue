<script setup lang="ts">
import { computed, useId, type DeepReadonly } from 'vue'
import { useI18n } from 'vue-i18n'
import type { SendFailure } from '@/types'

/**
 * How many records a recipient (a record destination or a share) refused at
 * their last try. Each refused record also says so in its own row.
 */
const props = defineProps<{ failures: DeepReadonly<SendFailure[]> }>()

const { t } = useI18n()
const headingId = useId()

const records = computed(() => new Set(props.failures.map(failure => failure.record_id)).size)
</script>

<template>
  <section class="card border-destructive/40 p-5" :aria-labelledby="headingId">
    <h2 :id="headingId" class="text-sm font-semibold text-foreground">{{ t('dashboard.activityLog.sendFailures.label') }}</h2>
    <p class="mt-1 text-sm text-muted-foreground">
      {{ t('dashboard.activityLog.sendFailures.summary', { count: records }, records) }}
    </p>
  </section>
</template>
