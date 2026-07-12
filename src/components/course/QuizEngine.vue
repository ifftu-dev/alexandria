<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppSpinner, AppAlert } from '@/components/ui'
import type { QuizDefinition, QuizResult, ElementSubmissionRecord } from '@/types'

const props = defineProps<{
  contentCid: string | null
  contentInline?: string | null
  elementId: string
  /** Element type ('quiz' | 'assessment') — recorded with the submission. */
  elementType?: string
  /** Enrollment the response is recorded under. Null while browsing
   *  unenrolled; persistence is skipped in that case. */
  enrollmentId?: string | null
  /** Course completed → review-only: the prior response is shown but the
   *  quiz can't be re-submitted. */
  readOnly?: boolean
}>()

const emit = defineEmits<{
  (e: 'complete', result: QuizResult): void
}>()

const { t } = useI18n()
const { invoke } = useLocalApi()
const quiz = ref<QuizDefinition | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

// Quiz state
const currentIndex = ref(0)
const answers = ref<Record<string, number[] | string>>({})
const submitted = ref(false)
const result = ref<QuizResult | null>(null)
const startTime = ref(Date.now())

const currentQuestion = computed(() => quiz.value?.questions[currentIndex.value] ?? null)
const totalQuestions = computed(() => quiz.value?.questions.length ?? 0)
const isLastQuestion = computed(() => currentIndex.value >= totalQuestions.value - 1)

const currentAnswer = computed(() => {
  if (!currentQuestion.value) return undefined
  return answers.value[currentQuestion.value.id]
})

function parseAndResetQuiz(json: string) {
  quiz.value = JSON.parse(json) as QuizDefinition
  currentIndex.value = 0
  answers.value = {}
  submitted.value = false
  result.value = null
  startTime.value = Date.now()
}

async function loadQuiz() {
  // Prefer inline content (works on all platforms including mobile)
  if (props.contentInline) {
    try {
      parseAndResetQuiz(props.contentInline)
    } catch (e: unknown) {
      error.value = t('courses.quiz.parseError', { error: String(e) })
      quiz.value = null
    }
    return
  }
  if (!props.contentCid) { quiz.value = null; return }
  loading.value = true
  error.value = null
  try {
    const bytes = await invoke<number[]>('content_resolve_bytes', { identifier: props.contentCid })
    const decoder = new TextDecoder()
    const json = decoder.decode(new Uint8Array(bytes))
    parseAndResetQuiz(json)
  } catch (e: unknown) {
    error.value = t('courses.quiz.loadError', { error: String(e) })
    quiz.value = null
  } finally {
    loading.value = false
  }
}

function selectOption(questionId: string, optionIndex: number, multi: boolean) {
  // Answered quizzes (fresh or restored from a prior submission) lock via
  // `submitted`; an unanswered quiz stays answerable even in a read-only
  // (completed) course — there's no saved response to protect.
  if (submitted.value) return
  if (multi) {
    const current = (answers.value[questionId] as number[] | undefined) ?? []
    const idx = current.indexOf(optionIndex)
    if (idx >= 0) {
      answers.value[questionId] = current.filter(i => i !== optionIndex)
    } else {
      answers.value[questionId] = [...current, optionIndex]
    }
  } else {
    answers.value[questionId] = [optionIndex]
  }
}

function setTextAnswer(questionId: string, text: string) {
  if (submitted.value) return
  answers.value[questionId] = text
}

function nextQuestion() {
  if (currentIndex.value < totalQuestions.value - 1) {
    currentIndex.value++
  }
}

function prevQuestion() {
  if (currentIndex.value > 0) {
    currentIndex.value--
  }
}

async function gradeQuiz() {
  if (!quiz.value || submitted.value) return

  const questionResults: { question_id: string; correct: boolean; points: number }[] = []
  let totalPoints = 0
  let earnedPoints = 0

  for (const q of quiz.value.questions) {
    totalPoints += q.points
    const answer = answers.value[q.id]
    let correct = false

    if (q.type === 'single_choice' || q.type === 'true_false') {
      const selected = answer as number[] | undefined
      if (selected && selected.length === 1 && q.correct_indices) {
        correct = q.correct_indices.includes(selected[0]!)
      }
    } else if (q.type === 'multiple_choice') {
      const selected = (answer as number[] | undefined) ?? []
      if (q.correct_indices) {
        const sortedSel = [...selected].sort()
        const sortedCorrect = [...q.correct_indices].sort()
        correct = sortedSel.length === sortedCorrect.length &&
          sortedSel.every((v, i) => v === sortedCorrect[i])
      }
    } else if (q.type === 'short_answer') {
      const text = (answer as string | undefined) ?? ''
      if (q.correct_answer) {
        correct = text.trim().toLowerCase() === q.correct_answer.trim().toLowerCase()
      }
    }

    if (correct) earnedPoints += q.points
    questionResults.push({ question_id: q.id, correct, points: correct ? q.points : 0 })
  }

  const score = totalPoints > 0 ? earnedPoints / totalPoints : 0
  const timeSpent = Math.round((Date.now() - startTime.value) / 1000)

  result.value = {
    total_points: totalPoints,
    earned_points: earnedPoints,
    score,
    passed: score >= (quiz.value.pass_threshold ?? 0.7),
    answers: questionResults,
    time_spent_seconds: timeSpent,
  }

  submitted.value = true
  await persistSubmission()
  emit('complete', result.value)
}

// Persist the raw answers + graded result so the response survives reload
// and feeds the course-completion assembler. Best-effort: a persistence
// hiccup never blocks the learner from seeing their score.
async function persistSubmission() {
  if (!props.enrollmentId || !result.value) return
  try {
    await invoke('record_element_submission', {
      enrollmentId: props.enrollmentId,
      elementId: props.elementId,
      elementType: props.elementType ?? 'quiz',
      answersJson: JSON.stringify({ answers: answers.value, result: result.value }),
      score: result.value.score,
    })
  } catch (e) {
    console.error('Failed to record quiz submission:', e)
  }
}

// Restore the learner's last response (answers + graded result) so revisiting
// the element shows what they did rather than a blank quiz.
async function loadPriorSubmission() {
  if (!props.enrollmentId) return
  try {
    const prior = await invoke<ElementSubmissionRecord | null>('get_element_submission', {
      enrollmentId: props.enrollmentId,
      elementId: props.elementId,
    })
    if (!prior?.answers_json) return
    const parsed = JSON.parse(prior.answers_json) as {
      answers?: Record<string, number[] | string>
      result?: QuizResult
    }
    if (parsed.answers) answers.value = parsed.answers
    if (parsed.result) {
      result.value = parsed.result
      submitted.value = true
    }
  } catch (e) {
    console.error('Failed to load prior quiz submission:', e)
  }
}

async function init() {
  await loadQuiz()
  await loadPriorSubmission()
}

function isOptionSelected(questionId: string, optionIndex: number): boolean {
  const answer = answers.value[questionId]
  if (!answer || typeof answer === 'string') return false
  return (answer as number[]).includes(optionIndex)
}

function questionResult(questionId: string): boolean | null {
  if (!submitted.value || !result.value) return null
  return result.value.answers.find(a => a.question_id === questionId)?.correct ?? null
}

onMounted(init)
watch(() => props.contentCid, init)
</script>

<template>
  <div class="quiz-engine">
    <AppSpinner v-if="loading" :label="t('courses.quiz.loading')" />

    <div v-else-if="error" class="text-sm text-destructive">
      {{ error }}
    </div>

    <div v-else-if="!quiz" class="text-sm text-muted-foreground italic">
      {{ $t('courses.quiz.noContent') }}
    </div>

    <div v-else class="space-y-6">
      <!-- Header -->
      <div class="flex items-center justify-between">
        <h3 class="text-base font-semibold">{{ quiz.title }}</h3>
        <span class="text-xs text-muted-foreground">
          {{ $t('courses.quiz.questionProgress', { current: currentIndex + 1, total: totalQuestions }) }}
        </span>
      </div>

      <!-- Progress bar -->
      <div class="h-1 bg-muted/30 rounded-full overflow-hidden">
        <div
          class="h-full bg-primary transition-all duration-300"
          :style="{ width: `${((currentIndex + 1) / totalQuestions) * 100}%` }"
        />
      </div>

      <!-- Results banner -->
      <AppAlert v-if="submitted && result" :variant="result.passed ? 'success' : 'warning'">
        <template #title>{{ result.passed ? $t('courses.quiz.passed') : $t('courses.quiz.notYet') }}</template>
        {{ $t('courses.quiz.scoreLine', { pct: Math.round(result.score * 100), earned: result.earned_points, total: result.total_points }) }}
        <span v-if="quiz.pass_threshold">{{ $t('courses.quiz.thresholdRequired', { pct: Math.round(quiz.pass_threshold * 100) }) }}</span>
      </AppAlert>

      <!-- Question -->
      <div v-if="currentQuestion" class="card p-6 space-y-4">
        <div class="flex items-start gap-2">
          <span
            class="text-xs font-bold px-2 py-0.5 rounded"
            :class="submitted ? (questionResult(currentQuestion.id) ? 'bg-success/15 text-success' : 'bg-destructive/15 text-destructive') : 'bg-muted/30 text-muted-foreground'"
          >
            {{ currentQuestion.type === 'multiple_choice' ? $t('courses.quiz.typeMulti') : currentQuestion.type === 'short_answer' ? $t('courses.quiz.typeText') : $t('courses.quiz.typeMc') }}
          </span>
          <span class="text-xs text-muted-foreground">
            {{ $t('courses.quiz.pointsCount', { count: currentQuestion.points }, currentQuestion.points) }}
          </span>
        </div>

        <p class="text-sm font-medium leading-relaxed">{{ currentQuestion.prompt }}</p>

        <!-- Choice options -->
        <div v-if="currentQuestion.options && currentQuestion.type !== 'short_answer'" class="space-y-2">
          <button
            v-for="(option, idx) in currentQuestion.options"
            :key="idx"
            class="w-full text-start p-3 rounded-lg border text-sm transition-all"
            :class="[
              isOptionSelected(currentQuestion.id, idx)
                ? 'border-primary bg-primary/8'
                : 'border-border hover:border-primary/50',
              submitted && currentQuestion.correct_indices?.includes(idx)
                ? 'border-success bg-success/8'
                : '',
              submitted && isOptionSelected(currentQuestion.id, idx) && !currentQuestion.correct_indices?.includes(idx)
                ? 'border-destructive bg-destructive/8'
                : '',
            ]"
            :disabled="submitted"
            @click="selectOption(currentQuestion.id, idx, currentQuestion.type === 'multiple_choice')"
          >
            <span class="inline-flex items-center gap-2">
              <span class="w-5 h-5 rounded-full border flex items-center justify-center text-xs shrink-0"
                :class="isOptionSelected(currentQuestion.id, idx) ? 'bg-primary text-white border-primary' : 'border-border'"
              >
                {{ String.fromCharCode(65 + idx) }}
              </span>
              {{ option }}
            </span>
          </button>
        </div>

        <!-- Short answer -->
        <div v-if="currentQuestion.type === 'short_answer'">
          <input
            type="text"
            class="input w-full"
            :placeholder="t('courses.quiz.shortAnswerPlaceholder')"
            :value="(currentAnswer as string) ?? ''"
            :disabled="submitted"
            @input="setTextAnswer(currentQuestion.id, ($event.target as HTMLInputElement).value)"
          />
        </div>

        <!-- Explanation (after submit) -->
        <div v-if="submitted && currentQuestion.explanation" class="text-xs text-muted-foreground bg-muted/20 p-3 rounded">
          {{ currentQuestion.explanation }}
        </div>
      </div>

      <!-- Navigation -->
      <div class="flex items-center justify-between">
        <AppButton
          v-if="currentIndex > 0"
          variant="secondary"
          size="sm"
          @click="prevQuestion"
        >
          {{ $t('courses.quiz.previous') }}
        </AppButton>
        <div v-else />

        <div class="flex gap-2">
          <AppButton
            v-if="!isLastQuestion"
            size="sm"
            @click="nextQuestion"
          >
            {{ $t('common.actions.next') }}
          </AppButton>
          <AppButton
            v-if="isLastQuestion && !submitted"
            size="sm"
            @click="gradeQuiz"
          >
            {{ $t('courses.quiz.submitQuiz') }}
          </AppButton>
        </div>
      </div>
    </div>
  </div>
</template>
