<script setup lang="ts">
/**
 * What this device sent of the learner's records, and to whom: the local
 * half of the access log. Reads by an organisation are known only to its
 * hosted service and are listed from there, beside this. The rows live in
 * `useRecordSendLog`, shared by every place this is shown.
 */
import { onMounted, type DeepReadonly } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRecordSendLog } from '@/composables/useRecordSendLog'
import type { RecordAccessEntry } from '@/types'

const { t, locale } = useI18n()
const { entries, error, refresh } = useRecordSendLog()

function when(iso: string): string {
  const parsed = new Date(iso)
  if (Number.isNaN(parsed.getTime())) return iso
  return new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short' }).format(parsed)
}

function line(e: DeepReadonly<RecordAccessEntry>): string {
  const values = { count: e.record_count, recipient: e.recipient_name, identifier: e.identifier, detail: e.detail ?? '' }
  if (e.action === 'void') {
    return e.accepted ? t('organisations.records.log.voided', values) : t('organisations.records.log.voidRefused', values)
  }
  return e.accepted ? t('organisations.records.log.sent', values) : t('organisations.records.log.refused', values)
}

onMounted(refresh)
</script>

<template>
  <div class="space-y-2" data-testid="record-send-log">
    <h3 class="text-sm font-medium text-foreground">{{ t('organisations.records.log.heading') }}</h3>
    <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>
    <p v-else-if="!entries.length" class="text-sm text-muted-foreground">{{ t('organisations.records.log.empty') }}</p>
    <ul v-else class="space-y-1">
      <li
        v-for="e in entries"
        :key="e.seq"
        class="break-words text-sm"
        :class="e.accepted ? 'text-foreground' : 'text-destructive'"
        data-testid="record-send-entry"
      >
        {{ line(e) }}
        <span class="text-xs text-muted-foreground"> · {{ when(e.at) }}</span>
      </li>
    </ul>
  </div>
</template>
