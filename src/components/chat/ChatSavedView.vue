<script setup lang="ts">
import { onMounted, ref, type DeepReadonly } from 'vue'
import { useI18n } from 'vue-i18n'
import { ConfirmDialog } from '@/components/ui'
import { chatErrorCode, useChat } from '@/composables/useChat'
import { useCourseNotes } from '@/composables/useCourseNotes'
import type { ChatArtifact } from '@/types'

/** Saved notes and flashcard sets, with a one-card-at-a-time study mode. */
const { t } = useI18n()
const chat = useChat()
const open = ref<string | null>(null)
const studying = ref<string | null>(null)
const position = ref(0)
const revealed = ref(false)
const deleting = ref<string | null>(null)
const courseNotes = useCourseNotes()
/** Per note: copying to its course, copied, or why it could not be. */
const courseState = ref<Record<string, 'busy' | 'saved' | 'not_enrolled' | 'failed'>>({})

function citedLesson(item: DeepReadonly<ChatArtifact>) {
  return item.body.kind === 'note' ? item.sources.find((s) => s.kind === 'lesson') : undefined
}

/** Copies a saved note into the course of the lesson it cites. */
async function toCourse(item: DeepReadonly<ChatArtifact>) {
  courseState.value = { ...courseState.value, [item.id]: 'busy' }
  let next: 'saved' | 'not_enrolled' | 'failed' = 'saved'
  try {
    await courseNotes.saveFromChat(item.id)
  } catch (e) {
    next = chatErrorCode(e) === 'not_enrolled' ? 'not_enrolled' : 'failed'
  }
  courseState.value = { ...courseState.value, [item.id]: next }
}

onMounted(() => {
  void chat.refreshSaved().catch((e) => (chat.error.value = chatErrorCode(e)))
})

function cardsOf(item: DeepReadonly<ChatArtifact>) {
  return item.body.kind === 'flashcards' ? item.body.cards : []
}

function study(item: DeepReadonly<ChatArtifact>) {
  studying.value = studying.value === item.id ? null : item.id
  open.value = item.id
  position.value = 0
  revealed.value = false
}

function move(item: DeepReadonly<ChatArtifact>, delta: number) {
  const total = cardsOf(item).length
  position.value = Math.max(0, Math.min(total - 1, position.value + delta))
  revealed.value = false
}

async function confirmDelete() {
  const id = deleting.value
  deleting.value = null
  if (!id) return
  try {
    await chat.deleteArtifact(id)
  } catch (e) {
    chat.error.value = chatErrorCode(e)
  }
}

function formatDate(value: string) {
  try {
    return new Date(value).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
  } catch {
    return ''
  }
}
</script>

<template>
  <div class="min-h-0 flex-1 overflow-y-auto p-3">
    <p class="mb-3 text-xs text-muted-foreground">{{ t('chat.artifacts.intro') }}</p>
    <p v-if="!chat.savedArtifacts.value.length" class="px-2 py-6 text-center text-sm text-muted-foreground">{{ t('chat.artifacts.empty') }}</p>
    <ul class="space-y-2">
      <li v-for="item in chat.savedArtifacts.value" :key="item.id" class="rounded-2xl bg-muted/50">
        <div class="flex items-start gap-2 px-3 py-2">
          <button type="button" class="min-w-0 flex-1 text-start" :aria-expanded="open === item.id" @click="open = open === item.id ? null : item.id">
            <span class="block truncate text-sm font-medium">{{ item.title }}</span>
            <span class="block text-xs text-muted-foreground">
              {{ item.body.kind === 'note' ? t('chat.artifacts.untitledNote') : t('chat.artifacts.cards', cardsOf(item).length) }} · {{ formatDate(item.updated_at) }}
            </span>
          </button>
          <button
            v-if="item.body.kind === 'flashcards'"
            type="button"
            class="rounded-full border border-border px-2 py-0.5 text-xs font-medium hover:bg-muted"
            :aria-pressed="studying === item.id"
            @click="study(item)"
          >{{ t('chat.artifacts.study') }}</button>
          <button type="button" class="rounded-full px-2 py-0.5 text-xs text-muted-foreground hover:bg-muted hover:text-foreground" @click="deleting = item.id">{{ t('chat.artifacts.delete') }}</button>
        </div>

        <div v-if="studying === item.id && cardsOf(item).length" class="border-t border-border px-3 py-3 text-sm">
          <p class="text-xs text-muted-foreground">{{ t('chat.artifacts.cardPosition', { current: position + 1, total: cardsOf(item).length }) }}</p>
          <p class="mt-1 font-medium">{{ cardsOf(item)[position]?.front }}</p>
          <p v-if="revealed" class="mt-1 text-muted-foreground">{{ cardsOf(item)[position]?.back }}</p>
          <div class="mt-2 flex items-center gap-1.5">
            <button type="button" class="chat-saved-btn" :disabled="position === 0" :aria-label="t('chat.artifacts.previous')" @click="move(item, -1)">‹</button>
            <button v-if="!revealed" type="button" class="rounded-full bg-primary px-3 py-0.5 text-xs font-medium text-primary-foreground" @click="revealed = true">{{ t('chat.artifacts.showAnswer') }}</button>
            <button type="button" class="chat-saved-btn" :disabled="position >= cardsOf(item).length - 1" :aria-label="t('chat.artifacts.next')" @click="move(item, 1)">›</button>
          </div>
        </div>
        <div v-else-if="open === item.id" class="border-t border-border px-3 py-2 text-sm">
          <template v-if="item.body.kind === 'note'">
            <p class="whitespace-pre-wrap">{{ item.body.text }}</p>
            <div v-if="citedLesson(item)" class="mt-2 flex flex-wrap items-center gap-2 text-xs">
              <span v-if="courseState[item.id] === 'saved'" class="text-success">{{ t('chat.courseNotes.saved', { lesson: citedLesson(item)!.label }) }}</span>
              <button
                v-else
                type="button"
                class="rounded-full border border-border px-2 py-0.5 font-medium hover:bg-muted disabled:opacity-60"
                :disabled="courseState[item.id] === 'busy'"
                @click="toCourse(item)"
              >{{ t('chat.courseNotes.add', { lesson: citedLesson(item)!.label }) }}</button>
              <span v-if="courseState[item.id] === 'not_enrolled'" class="text-muted-foreground">{{ t('chat.courseNotes.notEnrolled') }}</span>
              <span v-else-if="courseState[item.id] === 'failed'" class="text-error">{{ t('chat.courseNotes.failed') }}</span>
            </div>
          </template>
          <dl v-else class="space-y-1.5">
            <div v-for="(card, i) in cardsOf(item)" :key="i">
              <dt class="font-medium">{{ card.front }}</dt>
              <dd class="text-muted-foreground">{{ card.back }}</dd>
            </div>
          </dl>
        </div>
      </li>
    </ul>
    <ConfirmDialog
      :open="deleting !== null"
      :title="t('chat.artifacts.deleteTitle')"
      :message="t('chat.artifacts.deleteBody')"
      :confirm-label="t('chat.artifacts.delete')"
      confirm-variant="danger"
      @confirm="confirmDelete"
      @cancel="deleting = null"
    />
  </div>
</template>

<style scoped>
.chat-saved-btn { display: grid; place-items: center; width: 1.5rem; height: 1.5rem; border-radius: 999px; color: var(--app-muted-foreground); }
.chat-saved-btn:hover:not(:disabled) { background: var(--app-muted); color: var(--app-foreground); }
.chat-saved-btn:disabled { opacity: 0.4; }
</style>
