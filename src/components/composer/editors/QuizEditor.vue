<script setup lang="ts">
// Structured quiz/MCQ builder. Persists the same content_inline JSON
// shape the player's QuizEngine consumes:
//   { questions: [{ id, question, options[], correct_index }] }
// Multi-MCQ uses `correct_indices` instead of `correct_index`.
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import type { Element } from '@/types'

const props = defineProps<{ element: Element }>()
const emit = defineEmits<{ updated: [Element] }>()

const { invoke } = useLocalApi()
const { t } = useI18n()

interface Question {
  id: string
  question: string
  options: string[]
  correct_index?: number
  correct_indices?: number[]
}

const multi = () => props.element.element_type === 'objective_multi_mcq'

function parseQuestions(): Question[] {
  try {
    const parsed = JSON.parse(props.element.content_inline ?? '')
    if (Array.isArray(parsed?.questions)) return parsed.questions
  } catch { /* fall through to empty */ }
  return []
}

const questions = ref<Question[]>(parseQuestions())
const dirty = ref(false)
const saving = ref(false)
const error = ref('')

watch(() => props.element.id, () => {
  questions.value = parseQuestions()
  dirty.value = false
})

function addQuestion() {
  const q: Question = {
    id: `q${questions.value.length + 1}_${Date.now().toString(36)}`,
    question: '',
    options: ['', ''],
  }
  if (multi()) q.correct_indices = []
  else q.correct_index = 0
  questions.value.push(q)
  dirty.value = true
}

function removeQuestion(i: number) {
  questions.value.splice(i, 1)
  dirty.value = true
}

function addOption(q: Question) {
  q.options.push('')
  dirty.value = true
}

function removeOption(q: Question, i: number) {
  q.options.splice(i, 1)
  if (q.correct_index !== undefined && q.correct_index >= q.options.length) q.correct_index = 0
  if (q.correct_indices) q.correct_indices = q.correct_indices.filter(x => x < q.options.length)
  dirty.value = true
}

function toggleCorrect(q: Question, i: number) {
  if (multi()) {
    const set = new Set(q.correct_indices ?? [])
    if (set.has(i)) set.delete(i)
    else set.add(i)
    q.correct_indices = [...set].sort((a, b) => a - b)
  } else {
    q.correct_index = i
  }
  dirty.value = true
}

function isCorrect(q: Question, i: number): boolean {
  return multi() ? (q.correct_indices ?? []).includes(i) : q.correct_index === i
}

function validate(): string | null {
  if (!questions.value.length) return t('instructor.editors.quiz.errNeedQuestion')
  for (const [i, q] of questions.value.entries()) {
    if (!q.question.trim()) return t('instructor.editors.quiz.errNoPrompt', { number: i + 1 })
    if (q.options.filter(o => o.trim()).length < 2) return t('instructor.editors.quiz.errTooFewOptions', { number: i + 1 })
    if (multi() && !(q.correct_indices ?? []).length) return t('instructor.editors.quiz.errNoCorrect', { number: i + 1 })
  }
  return null
}

async function save() {
  const problem = validate()
  if (problem) {
    error.value = problem
    return
  }
  saving.value = true
  error.value = ''
  try {
    const updated = await invoke<Element>('update_element', {
      elementId: props.element.id,
      req: { content_inline: JSON.stringify({ questions: questions.value }, null, 2) },
    })
    emit('updated', updated)
    dirty.value = false
  } catch (e) {
    error.value = String(e)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center justify-between">
      <h3 class="text-sm font-semibold text-foreground">
        {{ $t('instructor.editors.quiz.questionsHeading') }}
        <span class="ml-1 text-xs font-normal text-muted-foreground">
          ({{ element.element_type === 'objective_multi_mcq' ? $t('instructor.editors.quiz.multiHint') : $t('instructor.editors.quiz.singleHint') }})
        </span>
      </h3>
      <div class="flex gap-2">
        <AppButton variant="ghost" size="xs" @click="addQuestion">{{ $t('instructor.editors.quiz.addQuestion') }}</AppButton>
        <AppButton v-if="dirty" size="xs" :loading="saving" @click="save">{{ $t('instructor.editors.quiz.saveQuiz') }}</AppButton>
      </div>
    </div>

    <p v-if="!questions.length" class="text-xs text-muted-foreground">
      {{ $t('instructor.editors.quiz.empty') }}
    </p>

    <div
      v-for="(q, qi) in questions"
      :key="q.id"
      class="rounded-lg border border-border bg-muted/10 p-4 space-y-3"
    >
      <div class="flex items-start gap-2">
        <span class="mt-2 text-xs font-mono text-muted-foreground">{{ qi + 1 }}.</span>
        <textarea
          v-model="q.question"
          rows="2"
          class="flex-1 rounded-md border border-border bg-background px-3 py-2 text-sm"
          :placeholder="$t('instructor.editors.quiz.questionPlaceholder')"
          @input="dirty = true"
        />
        <button
          type="button"
          class="rounded-md p-2 text-muted-foreground hover:bg-muted hover:text-foreground"
          :title="$t('instructor.editors.quiz.removeQuestion')"
          @click="removeQuestion(qi)"
        >
          <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>

      <div class="space-y-1.5 pl-6">
        <div v-for="(_, oi) in q.options" :key="oi" class="flex items-center gap-2">
          <button
            type="button"
            class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full border transition-colors"
            :class="isCorrect(q, oi) ? 'border-success bg-success text-white' : 'border-border text-transparent hover:border-success/60'"
            :title="isCorrect(q, oi) ? $t('instructor.editors.quiz.correctAnswer') : $t('instructor.editors.quiz.markCorrect')"
            @click="toggleCorrect(q, oi)"
          >
            <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="3">
              <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
            </svg>
          </button>
          <input
            v-model="q.options[oi]"
            type="text"
            class="flex-1 rounded-md border border-border bg-background px-3 py-1.5 text-sm"
            :placeholder="$t('instructor.editors.quiz.optionPlaceholder', { number: oi + 1 })"
            @input="dirty = true"
          >
          <button
            type="button"
            class="rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-30"
            :disabled="q.options.length <= 2"
            :title="$t('instructor.editors.quiz.removeOption')"
            @click="removeOption(q, oi)"
          >
            <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
        <button
          type="button"
          class="text-xs text-primary hover:underline"
          @click="addOption(q)"
        >
          {{ $t('instructor.editors.quiz.addOption') }}
        </button>
      </div>
    </div>

    <p v-if="error" class="text-sm text-error">{{ error }}</p>
  </div>
</template>
