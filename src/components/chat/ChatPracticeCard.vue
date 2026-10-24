<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppButton } from '@/components/ui'
import { chatErrorCode, useChat } from '@/composables/useChat'
import type { ChatPracticeView } from '@/types'

/**
 * One practice question. Answers are checked by Alexandria against the
 * stored question, never by a model. Hints come one at a time and showing
 * the answer is explicit; either marks later attempts as assisted.
 */
const props = defineProps<{ item: ChatPracticeView; latest: boolean }>()
const { t } = useI18n()
const chat = useChat()
const answer = ref('')
const choice = ref<number | null>(null)
const busy = ref(false)
const feedback = ref<'correct' | 'incorrect' | 'unreadable' | null>(null)

const done = computed(() => props.item.solved || props.item.revealed)
const assisted = computed(() => props.item.hints.length > 0 || props.item.revealed)
const canCheck = computed(() =>
  !props.item.solved && (props.item.kind === 'multiple_choice' ? choice.value !== null : !!answer.value.trim()),
)

async function check() {
  if (!canCheck.value) return
  busy.value = true
  feedback.value = null
  try {
    const given = props.item.kind === 'multiple_choice' ? String(choice.value) : answer.value
    const before = props.item.attempts.length
    await chat.practiceAttempt(props.item.id, given)
    const last = chat.detail.value?.practice.find((p) => p.id === props.item.id)
    const attempt = last?.attempts[last.attempts.length - 1]
    if (last && last.attempts.length > before && attempt) feedback.value = attempt.correct ? 'correct' : 'incorrect'
  } catch (e) {
    feedback.value = chatErrorCode(e) === 'invalid_input' ? 'unreadable' : null
    if (!feedback.value) chat.error.value = chatErrorCode(e)
  } finally {
    busy.value = false
  }
}

async function hint() {
  busy.value = true
  try {
    await chat.practiceHint(props.item.id)
  } catch (e) {
    chat.error.value = chatErrorCode(e)
  } finally {
    busy.value = false
  }
}

async function reveal() {
  busy.value = true
  try {
    await chat.practiceReveal(props.item.id)
  } catch (e) {
    chat.error.value = chatErrorCode(e)
  } finally {
    busy.value = false
  }
}

function another() {
  void chat.ask(t('chat.practice.anotherPrompt'))
}
</script>

<template>
  <div class="rounded-2xl border border-border bg-background p-3 text-sm">
    <div class="mb-2 flex flex-wrap gap-1.5 text-[10.5px]">
      <span class="chat-pill">{{ t('chat.practice.label') }}</span>
      <span v-if="item.skill" class="chat-pill">{{ item.skill }}</span>
      <span class="chat-pill">{{ t('chat.practice.checked') }}</span>
      <span v-if="item.solved && item.solved_independently" class="chat-pill chat-pill--good">{{ t('chat.practice.independent') }}</span>
      <span v-else-if="item.solved" class="chat-pill chat-pill--help">{{ t('chat.practice.assisted') }}</span>
      <span v-else-if="item.revealed" class="chat-pill chat-pill--help">{{ t('chat.practice.answerShown') }}</span>
      <span v-else-if="assisted" class="chat-pill chat-pill--help">{{ t('chat.practice.hintUsed') }}</span>
    </div>
    <p class="whitespace-pre-wrap font-medium leading-relaxed">{{ item.prompt }}</p>

    <fieldset v-if="item.kind === 'multiple_choice'" class="mt-2 space-y-1.5" :disabled="item.solved || busy">
      <legend class="sr-only">{{ t('chat.practice.answer') }}</legend>
      <label
        v-for="(option, index) in item.options"
        :key="index"
        class="flex cursor-pointer items-center gap-2 rounded-lg border px-2.5 py-1.5"
        :class="choice === index ? 'border-primary bg-primary/5' : 'border-border hover:bg-muted/50'"
      >
        <input v-model="choice" type="radio" class="accent-[var(--app-primary)]" :name="`practice-${item.id}`" :value="index">
        <span>{{ option }}</span>
      </label>
    </fieldset>
    <form v-else-if="!item.solved" class="mt-2 flex gap-1.5" @submit.prevent="check">
      <input
        v-model="answer"
        class="input flex-1 py-1.5 text-sm"
        :disabled="item.solved || busy"
        :inputmode="item.kind === 'numeric' ? 'decimal' : 'text'"
        :placeholder="item.kind === 'numeric' ? t('chat.practice.answerNumeric') : t('chat.practice.answer')"
        :aria-label="t('chat.practice.answer')"
        maxlength="200"
      >
    </form>

    <ul v-if="item.hints.length" class="mt-2 space-y-1 text-xs text-muted-foreground">
      <li v-for="(h, i) in item.hints" :key="i" class="rounded-md bg-muted/60 px-2 py-1">{{ h }}</li>
    </ul>

    <p
      v-if="feedback || item.solved"
      class="mt-2 rounded-lg px-2.5 py-1.5 text-xs"
      :class="item.solved ? 'bg-success/10' : feedback === 'unreadable' ? 'bg-muted' : 'bg-warning/10'"
      role="status"
    >
      {{ item.solved ? t('chat.practice.correct') : feedback === 'unreadable' ? t('chat.practice.unreadable') : t('chat.practice.incorrect') }}
    </p>
    <div v-if="done && (item.answer || item.explanation)" class="mt-2 rounded-lg border border-border px-2.5 py-1.5 text-xs">
      <p v-if="item.answer" class="font-medium">{{ t('chat.practice.theAnswer', { answer: item.answer }) }}</p>
      <p v-if="item.explanation" class="mt-0.5 text-muted-foreground">{{ item.explanation }}</p>
    </div>

    <div class="mt-2.5 flex flex-wrap items-center gap-1.5">
      <AppButton v-if="!item.solved" size="xs" type="button" :disabled="!canCheck" :loading="busy" @click="check">{{ t('chat.practice.check') }}</AppButton>
      <AppButton
        v-if="!item.solved && !item.revealed"
        variant="ghost"
        size="xs"
        type="button"
        :disabled="busy || item.hints.length >= item.hints_total"
        :title="item.hints_total ? '' : t('chat.practice.noHints')"
        @click="hint"
      >{{ item.hints_total ? t('chat.practice.hintCount', { used: item.hints.length, total: item.hints_total }) : t('chat.practice.hint') }}</AppButton>
      <AppButton v-if="!done" variant="ghost" size="xs" type="button" :disabled="busy" @click="reveal">{{ t('chat.practice.reveal') }}</AppButton>
      <span class="flex-1" />
      <span v-if="item.attempts.length" class="text-[11px] text-muted-foreground">{{ t('chat.practice.attempts', item.attempts.length) }}</span>
      <AppButton v-if="done && latest" variant="outline" size="xs" type="button" :disabled="chat.streaming.value || chat.preparing.value" @click="another">{{ t('chat.practice.another') }}</AppButton>
    </div>
  </div>
</template>

<style scoped>
.chat-pill { padding: 0.05rem 0.45rem; border-radius: 999px; background: var(--app-muted); color: var(--app-muted-foreground); }
.chat-pill--good { background: color-mix(in srgb, var(--app-success) 14%, transparent); color: color-mix(in srgb, var(--app-success) 65%, var(--app-foreground)); }
.chat-pill--help { background: color-mix(in srgb, var(--app-warning) 16%, transparent); color: color-mix(in srgb, var(--app-warning) 70%, var(--app-foreground)); }
</style>
