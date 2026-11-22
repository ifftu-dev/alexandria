<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppBadge } from '@/components/ui'
import type { MessageKey } from '@/i18n/keys'
import type { LearningRecordEntry } from '@/types'

const props = defineProps<{ entry: LearningRecordEntry, selected: boolean }>()
defineEmits<{ toggle: [], delete: [] }>()

const { t, locale } = useI18n()

const VERBS: Record<string, MessageKey> = {
  registered: 'dashboard.activityLog.verbs.registered',
  launched: 'dashboard.activityLog.verbs.launched',
  progressed: 'dashboard.activityLog.verbs.progressed',
  completed: 'dashboard.activityLog.verbs.completed',
  passed: 'dashboard.activityLog.verbs.passed',
  failed: 'dashboard.activityLog.verbs.failed',
  scored: 'dashboard.activityLog.verbs.scored',
  responded: 'dashboard.activityLog.verbs.responded',
  voided: 'dashboard.activityLog.verbs.voided',
  satisfied: 'dashboard.activityLog.verbs.satisfied',
  abandoned: 'dashboard.activityLog.verbs.abandoned',
}

const verb = computed(() => {
  const key = VERBS[props.entry.verb]
  return key ? t(key) : props.entry.verb
})

const course = computed(() => props.entry.course_title
  ?? t('dashboard.activityLog.row.unknownCourse', { id: props.entry.course_id.slice(0, 8) }))

/** The element when the record is about one, else the course. */
const subject = computed(() => props.entry.element_title ?? course.value)

const percent = (value: number) => new Intl.NumberFormat(locale.value, { style: 'percent', maximumFractionDigits: 0 }).format(value)

const when = computed(() => new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short' })
  .format(new Date(props.entry.recorded_at)))
</script>

<template>
  <li class="flex items-start gap-3 px-4 py-3">
    <input
      type="checkbox"
      class="mt-1 h-4 w-4 shrink-0"
      :checked="selected"
      :aria-label="t('dashboard.activityLog.row.select', { what: `${verb} ${subject}` })"
      @change="$emit('toggle')"
    >
    <div class="min-w-0 flex-1">
      <p class="text-sm text-foreground">
        <span class="font-medium">{{ verb }}</span>
        <span class="text-muted-foreground">·</span>
        <span>{{ subject }}</span>
      </p>
      <p class="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
        <span v-if="entry.element_title">{{ course }}</span>
        <time :datetime="entry.recorded_at">{{ when }}</time>
        <span v-if="entry.percent !== null">{{ t('dashboard.activityLog.row.percent', { percent: percent(entry.percent / 100) }) }}</span>
        <span v-if="entry.score !== null">{{ t('dashboard.activityLog.row.score', { score: percent(entry.score) }) }}</span>
        <AppBadge v-if="entry.kind === 'interaction'">{{ t('dashboard.activityLog.row.response') }}</AppBadge>
        <AppBadge v-if="entry.summary">{{ t('dashboard.activityLog.row.summary') }}</AppBadge>
      </p>
      <!-- Per-record extras another part of the app adds, such as send status. -->
      <slot name="status" :entry="entry" />
    </div>
    <button
      type="button"
      class="btn btn-ghost btn-xs shrink-0"
      :aria-label="t('dashboard.activityLog.row.delete')"
      :title="t('dashboard.activityLog.row.delete')"
      @click="$emit('delete')"
    >
      <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <path stroke-linecap="round" stroke-linejoin="round" d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.76-2.164-1.789-2.201a51.964 51.964 0 00-3.32 0c-1.03.037-1.79 1.022-1.79 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
      </svg>
    </button>
  </li>
</template>
