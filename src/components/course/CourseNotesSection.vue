<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { ConfirmDialog } from '@/components/ui'
import { useCourseNotes } from '@/composables/useCourseNotes'
import type { CourseNote } from '@/types'

/** The learner's notes on this course, the current lesson's first. Hidden
 * when there are none; the player leaves it out during assessments, and the
 * backend refuses the list then too. */
const props = defineProps<{ courseId: string; elementId: string | null }>()
const emit = defineEmits<{ open: [elementId: string] }>()
const { t } = useI18n()
const notes = useCourseNotes()

const items = ref<CourseNote[]>([])
const expanded = ref<string | null>(null)
const deleting = ref<string | null>(null)
const failed = ref(false)
let generation = 0

async function load() {
  const started = ++generation
  try {
    const next = await notes.list(props.courseId)
    if (started === generation) items.value = Array.isArray(next) ? next : []
  } catch {
    if (started === generation) items.value = []
  }
}
watch(() => [props.courseId, notes.revision.value], load, { immediate: true })

const here = computed(() => items.value.filter((n) => props.elementId && n.element_id === props.elementId))
const elsewhere = computed(() => items.value.filter((n) => !props.elementId || n.element_id !== props.elementId))

async function confirmDelete() {
  const id = deleting.value
  deleting.value = null
  if (!id) return
  failed.value = false
  try {
    await notes.remove(id)
  } catch {
    failed.value = true
  }
}
</script>

<template>
  <section v-if="items.length" class="space-y-2" :aria-label="t('learn.notes.title')">
    <h3 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{{ t('learn.notes.title') }} ({{ items.length }})</h3>
    <template v-for="group in [{ key: 'here', list: here }, { key: 'elsewhere', list: elsewhere }]" :key="group.key">
      <div v-if="group.list.length" class="space-y-1.5">
        <p class="text-[11px] text-muted-foreground">{{ group.key === 'here' ? t('learn.notes.thisLesson') : t('learn.notes.otherLessons') }}</p>
        <div v-for="note in group.list" :key="note.id" class="rounded-lg border border-border/60 bg-card/60 text-xs">
          <button type="button" class="w-full px-2.5 py-2 text-start" :aria-expanded="expanded === note.id" @click="expanded = expanded === note.id ? null : note.id">
            <span class="block truncate font-medium text-foreground">{{ note.title || t('learn.notes.untitled') }}</span>
            <span v-if="group.key === 'elsewhere' && note.element_title" class="block truncate text-[11px] text-muted-foreground">{{ note.element_title }}</span>
          </button>
          <div v-if="expanded === note.id" class="border-t border-border/60 px-2.5 py-2">
            <p class="whitespace-pre-wrap text-foreground">{{ note.body }}</p>
            <p v-if="note.origin === 'chat'" class="mt-1 text-[11px] text-muted-foreground">{{ t('learn.notes.fromChat') }}</p>
            <div class="mt-1.5 flex gap-2 text-[11px]">
              <button v-if="group.key === 'elsewhere' && note.element_id" type="button" class="text-primary hover:underline" @click="emit('open', note.element_id)">{{ t('learn.notes.openLesson') }}</button>
              <button type="button" class="text-muted-foreground hover:text-foreground" @click="deleting = note.id">{{ t('learn.notes.delete') }}</button>
            </div>
          </div>
        </div>
      </div>
    </template>
    <p v-if="failed" class="text-[11px] text-error">{{ t('learn.notes.deleteFailed') }}</p>
    <ConfirmDialog
      :open="deleting !== null"
      :title="t('learn.notes.deleteTitle')"
      :message="t('learn.notes.deleteBody')"
      :confirm-label="t('learn.notes.delete')"
      confirm-variant="danger"
      @confirm="confirmDelete"
      @cancel="deleting = null"
    />
  </section>
</template>
