<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppSpinner, AppAlert } from '@/components/ui'
import type { QuizDefinition, QuizResult } from '@/types'

const props = defineProps<{
  contentCid: string | null
  elementId: string
}>()

const emit = defineEmits<{
  (e: 'complete', result: QuizResult): void
}>()

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

async function loadQuiz() {
  if (!props.contentCid) { quiz.value = null; return }
  loading.value = true
  error.value = null
  try {
    const bytes = await invoke<number[]>('content_resolve_bytes', { identifier: props.contentCid })
    const decoder = new TextDecoder()
    const json = decoder.decode(new Uint8Array(bytes))
    quiz.value = JSON.parse(json) as QuizDefinition
    // Reset state
    currentIndex.value = 0
    answers.value = {}
    submitted.value = false
    result.value = null
    startTime.value = Date.now()
  } catch (e: unknown) {
    error.value = `Failed to load quiz: ${e}`
    quiz.value = null
  } finally {
    loading.value = false
  }
}

function selectOption(questionId: string, optionIndex: number, multi: boolean) {
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

function gradeQuiz() {
  if (!quiz.value) return

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
  emit('complete', result.value)
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

onMounted(loadQuiz)
watch(() => props.contentCid, loadQuiz)
</script>

<template>
  <div class="quiz-engine">
    <AppSpinner v-if="loading" label="Loading quiz..." />

    <div v-else-if="error" class="text-sm text-[rgb(var(--color-destructive))]">
      {{ error }}
    </div>

    <div v-else-if="!quiz" class="text-sm text-[rgb(var(--color-muted-foreground))] italic">
      No quiz content available.
    </div>

    <div v-else class="space-y-6">
      <!-- Header -->
      <div class="flex items-center justify-between">
        <h3 class="text-base font-semibold">{{ quiz.title }}</h3>
        <span class="text-xs text-[rgb(var(--color-muted-foreground))]">
          Question {{ currentIndex + 1 }} / {{ totalQuestions }}
        </span>
      </div>

      <!-- Progress bar -->
      <div class="h-1 bg-[rgb(var(--color-muted)/0.3)] rounded-full overflow-hidden">
        <div
          class="h-full bg-[rgb(var(--color-primary))] transition-all duration-300"
          :style="{ width: `${((currentIndex + 1) / totalQuestions) * 100}%` }"
        />
      </div>

      <!-- Results banner -->
      <AppAlert v-if="submitted && result" :variant="result.passed ? 'success' : 'warning'">
        <template #title>{{ result.passed ? 'Passed!' : 'Not yet' }}</template>
        Score: {{ Math.round(result.score * 100) }}% ({{ result.earned_points }}/{{ result.total_points }} points)
        <span v-if="quiz.pass_threshold"> — {{ Math.round(quiz.pass_threshold * 100) }}% required</span>
      </AppAlert>

      <!-- Question -->
      <div v-if="currentQuestion" class="card p-6 space-y-4">
        <div class="flex items-start gap-2">
          <span
            class="text-xs font-bold px-2 py-0.5 rounded"
            :class="submitted ? (questionResult(currentQuestion.id) ? 'bg-[rgb(var(--color-success)/0.15)] text-[rgb(var(--color-success))]' : 'bg-[rgb(var(--color-destructive)/0.15)] text-[rgb(var(--color-destructive))]') : 'bg-[rgb(var(--color-muted)/0.3)] text-[rgb(var(--color-muted-foreground))]'"
          >
            {{ currentQuestion.type === 'multiple_choice' ? 'Multi' : currentQuestion.type === 'short_answer' ? 'Text' : 'MC' }}
          </span>
          <span class="text-xs text-[rgb(var(--color-muted-foreground))]">
            {{ currentQuestion.points }} pt{{ currentQuestion.points !== 1 ? 's' : '' }}
          </span>
        </div>

        <p class="text-sm font-medium leading-relaxed">{{ currentQuestion.prompt }}</p>

        <!-- Choice options -->
        <div v-if="currentQuestion.options && currentQuestion.type !== 'short_answer'" class="space-y-2">
          <button
            v-for="(option, idx) in currentQuestion.options"
            :key="idx"
            class="w-full text-left p-3 rounded-lg border text-sm transition-all"
            :class="[
              isOptionSelected(currentQuestion.id, idx)
                ? 'border-[rgb(var(--color-primary))] bg-[rgb(var(--color-primary)/0.08)]'
                : 'border-[rgb(var(--color-border))] hover:border-[rgb(var(--color-primary)/0.5)]',
              submitted && currentQuestion.correct_indices?.includes(idx)
                ? 'border-[rgb(var(--color-success))] bg-[rgb(var(--color-success)/0.08)]'
                : '',
              submitted && isOptionSelected(currentQuestion.id, idx) && !currentQuestion.correct_indices?.includes(idx)
                ? 'border-[rgb(var(--color-destructive))] bg-[rgb(var(--color-destructive)/0.08)]'
                : '',
            ]"
            :disabled="submitted"
            @click="selectOption(currentQuestion.id, idx, currentQuestion.type === 'multiple_choice')"
          >
            <span class="inline-flex items-center gap-2">
              <span class="w-5 h-5 rounded-full border flex items-center justify-center text-xs shrink-0"
                :class="isOptionSelected(currentQuestion.id, idx) ? 'bg-[rgb(var(--color-primary))] text-white border-[rgb(var(--color-primary))]' : 'border-[rgb(var(--color-border))]'"
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
            placeholder="Type your answer..."
            :value="(currentAnswer as string) ?? ''"
            :disabled="submitted"
            @input="setTextAnswer(currentQuestion.id, ($event.target as HTMLInputElement).value)"
          />
        </div>

        <!-- Explanation (after submit) -->
        <div v-if="submitted && currentQuestion.explanation" class="text-xs text-[rgb(var(--color-muted-foreground))] bg-[rgb(var(--color-muted)/0.2)] p-3 rounded">
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
          Previous
        </AppButton>
        <div v-else />

        <div class="flex gap-2">
          <AppButton
            v-if="!isLastQuestion"
            size="sm"
            @click="nextQuestion"
          >
            Next
          </AppButton>
          <AppButton
            v-if="isLastQuestion && !submitted"
            size="sm"
            @click="gradeQuiz"
          >
            Submit Quiz
          </AppButton>
        </div>
      </div>
    </div>
  </div>
</template>
