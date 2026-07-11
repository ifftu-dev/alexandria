<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import type { ElementSubmissionRecord } from '@/types'

type McqType = 'objective_single_mcq' | 'objective_multi_mcq' | 'subjective_mcq'

interface McqOption {
  id: string
  text: string
}

interface McqContent {
  question: string
  options: McqOption[]
  correct_option_index?: number
  correct_option_indices?: number[]
  context?: string
  explanation?: string
}

const props = defineProps<{
  contentCid: string | null
  contentInline?: string | null
  elementId: string
  type: McqType
  isCompleted?: boolean
  /** Enrollment the response is recorded under. Null while browsing
   *  unenrolled; persistence is skipped in that case. */
  enrollmentId?: string | null
  /** Course completed → review-only: prior answer shown, no re-submit. */
  readOnly?: boolean
}>()

const emit = defineEmits<{
  (e: 'complete', score: number): void
}>()

const { t } = useI18n()
const { invoke } = useLocalApi()
const mcq = ref<McqContent | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const selectedIndices = ref<number[]>([])
const submitted = ref(false)
const score = ref(0)

const isSingle = computed(() => props.type === 'objective_single_mcq' || props.type === 'subjective_mcq')
const isSubjective = computed(() => props.type === 'subjective_mcq')
const isMulti = computed(() => props.type === 'objective_multi_mcq')

const typeBadge = computed(() => {
  switch (props.type) {
    case 'objective_single_mcq': return { label: t('courses.mcq.typeSingleChoice'), color: 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400' }
    case 'objective_multi_mcq': return { label: t('courses.mcq.typeMultipleChoice'), color: 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400' }
    case 'subjective_mcq': return { label: t('courses.mcq.typeSubjective'), color: 'bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400' }
  }
})

function parseAndReset(json: string) {
  mcq.value = JSON.parse(json) as McqContent
  selectedIndices.value = []
  submitted.value = false
  score.value = 0
}

async function loadContent() {
  // Prefer inline content (works on all platforms including mobile)
  if (props.contentInline) {
    try {
      parseAndReset(props.contentInline)
    } catch (e: unknown) {
      error.value = t('courses.mcq.parseError', { error: String(e) })
      mcq.value = null
    }
    return
  }
  if (!props.contentCid) { mcq.value = null; return }
  loading.value = true
  error.value = null
  try {
    const bytes = await invoke<number[]>('content_resolve_bytes', { identifier: props.contentCid })
    const decoder = new TextDecoder()
    const json = decoder.decode(new Uint8Array(bytes))
    parseAndReset(json)
  } catch (e: unknown) {
    error.value = t('courses.mcq.loadError', { error: String(e) })
    mcq.value = null
  } finally {
    loading.value = false
  }
}

function toggleOption(idx: number) {
  // Only a submitted answer (fresh or restored from a prior submission) locks
  // selection — an unanswered element stays answerable even if the element was
  // marked complete before or the course is read-only (nothing to protect).
  if (submitted.value) return
  if (isSingle.value) {
    selectedIndices.value = [idx]
  } else {
    const i = selectedIndices.value.indexOf(idx)
    if (i >= 0) {
      selectedIndices.value = selectedIndices.value.filter(v => v !== idx)
    } else {
      selectedIndices.value = [...selectedIndices.value, idx]
    }
  }
}

async function submitAnswer() {
  if (!mcq.value || selectedIndices.value.length === 0 || submitted.value) return
  submitted.value = true

  if (isSubjective.value) {
    // Subjective MCQ always scored as 100% locally
    score.value = 1
  } else if (props.type === 'objective_single_mcq') {
    const correct = mcq.value.correct_option_index
    score.value = (correct !== undefined && selectedIndices.value[0] === correct) ? 1 : 0
  } else if (props.type === 'objective_multi_mcq') {
    const correctIndices = mcq.value.correct_option_indices ?? []
    const selected = selectedIndices.value
    const correctSelected = selected.filter(i => correctIndices.includes(i)).length
    const incorrectSelected = selected.filter(i => !correctIndices.includes(i)).length
    const totalCorrect = correctIndices.length || 1
    score.value = Math.max(0, (correctSelected - incorrectSelected) / totalCorrect)
  }

  await persistSubmission()
  emit('complete', score.value)
}

// Persist the chosen options + score so the response survives reload and
// counts toward course completion. Best-effort.
async function persistSubmission() {
  if (!props.enrollmentId) return
  try {
    await invoke('record_element_submission', {
      enrollmentId: props.enrollmentId,
      elementId: props.elementId,
      elementType: props.type,
      answersJson: JSON.stringify({ selectedIndices: selectedIndices.value, score: score.value }),
      score: score.value,
    })
  } catch (e) {
    console.error('Failed to record MCQ submission:', e)
  }
}

// Restore the learner's last response so revisiting shows their answer.
async function loadPriorSubmission() {
  if (!props.enrollmentId) return
  try {
    const prior = await invoke<ElementSubmissionRecord | null>('get_element_submission', {
      enrollmentId: props.enrollmentId,
      elementId: props.elementId,
    })
    if (!prior?.answers_json) return
    const parsed = JSON.parse(prior.answers_json) as { selectedIndices?: number[]; score?: number }
    if (parsed.selectedIndices) {
      selectedIndices.value = parsed.selectedIndices
      score.value = parsed.score ?? 0
      submitted.value = true
    }
  } catch (e) {
    console.error('Failed to load prior MCQ submission:', e)
  }
}

async function init() {
  await loadContent()
  await loadPriorSubmission()
}

function tryAgain() {
  if (props.readOnly) return
  selectedIndices.value = []
  submitted.value = false
  score.value = 0
}

function isCorrectOption(idx: number): boolean {
  if (!mcq.value) return false
  if (props.type === 'objective_single_mcq') {
    return mcq.value.correct_option_index === idx
  }
  if (props.type === 'objective_multi_mcq') {
    return (mcq.value.correct_option_indices ?? []).includes(idx)
  }
  return false
}

onMounted(init)
watch(() => props.contentCid, init)
watch(() => props.elementId, () => {
  selectedIndices.value = []
  submitted.value = false
  score.value = 0
  void init()
})
</script>

<template>
  <div class="mcq-question">
    <!-- Loading -->
    <div v-if="loading" class="flex items-center justify-center py-12">
      <div class="h-8 w-8 animate-spin rounded-full border-2 border-primary border-t-transparent" />
    </div>

    <!-- Error -->
    <div v-else-if="error" class="rounded-lg border border-red-500/20 bg-red-500/10 p-4 text-sm text-red-600 dark:text-red-400">
      {{ error }}
    </div>

    <!-- No content -->
    <div v-else-if="!mcq" class="py-8 text-center text-sm text-muted-foreground">
      {{ $t('courses.mcq.noContent') }}
    </div>

    <!-- MCQ Content -->
    <div v-else class="space-y-5">
      <!-- Type badge -->
      <div class="flex items-center gap-2">
        <span class="rounded-full px-2.5 py-0.5 text-xs font-medium" :class="typeBadge.color">
          {{ typeBadge.label }}
        </span>
        <span v-if="isMulti" class="text-xs text-muted-foreground">
          {{ $t('courses.mcq.selectAllThatApply') }}
        </span>
      </div>

      <!-- Question -->
      <p class="text-base font-medium leading-relaxed text-foreground">
        {{ mcq.question }}
      </p>

      <!-- Context (subjective only) -->
      <div v-if="isSubjective && mcq.context" class="rounded-lg bg-muted/30 p-4 text-sm text-muted-foreground">
        {{ mcq.context }}
      </div>

      <!-- Options -->
      <div class="space-y-2">
        <button
          v-for="(option, idx) in mcq.options"
          :key="option.id || idx"
          class="flex w-full items-start gap-3 rounded-lg border p-4 text-left text-sm transition-all"
          :class="[
            selectedIndices.includes(idx) && !submitted
              ? 'border-primary bg-primary/6'
              : 'border-border hover:border-primary/40',
            submitted && !isSubjective && isCorrectOption(idx)
              ? 'border-emerald-500 bg-emerald-50 dark:border-emerald-500/50 dark:bg-emerald-900/20'
              : '',
            submitted && !isSubjective && selectedIndices.includes(idx) && !isCorrectOption(idx)
              ? 'border-red-500 bg-red-50 dark:border-red-500/50 dark:bg-red-900/20'
              : '',
          ]"
          :disabled="submitted"
          @click="toggleOption(idx)"
        >
          <!-- Radio/Checkbox indicator -->
          <span
            class="mt-0.5 flex h-5 w-5 flex-shrink-0 items-center justify-center border"
            :class="[
              isSingle ? 'rounded-full' : 'rounded',
              selectedIndices.includes(idx)
                ? 'border-primary bg-primary text-white'
                : 'border-border',
              submitted && !isSubjective && isCorrectOption(idx)
                ? 'border-emerald-500 bg-emerald-500 text-white'
                : '',
              submitted && !isSubjective && selectedIndices.includes(idx) && !isCorrectOption(idx)
                ? 'border-red-500 bg-red-500 text-white'
                : '',
            ]"
          >
            <svg v-if="selectedIndices.includes(idx)" class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="3">
              <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
            </svg>
          </span>
          <span class="flex-1" :class="submitted && !isSubjective && isCorrectOption(idx) ? 'font-medium' : ''">
            {{ option.text }}
          </span>
        </button>
      </div>

      <!-- Result banner -->
      <div v-if="submitted && !isSubjective" class="flex items-center gap-3 rounded-lg p-4" :class="score >= 0.7 ? 'bg-emerald-50 dark:bg-emerald-900/20' : 'bg-red-50 dark:bg-red-900/20'">
        <svg v-if="score >= 0.7" class="h-5 w-5 text-emerald-600 dark:text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <svg v-else class="h-5 w-5 text-red-600 dark:text-red-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <div>
          <p class="text-sm font-medium" :class="score >= 0.7 ? 'text-emerald-800 dark:text-emerald-300' : 'text-red-800 dark:text-red-300'">
            {{ score >= 0.7 ? $t('courses.mcq.correct') : $t('courses.mcq.incorrect') }}
          </p>
          <p v-if="isMulti" class="text-xs" :class="score >= 0.7 ? 'text-emerald-600 dark:text-emerald-400' : 'text-red-600 dark:text-red-400'">
            {{ $t('courses.mcq.scorePct', { pct: Math.round(score * 100) }) }}
          </p>
        </div>
      </div>

      <!-- Subjective result -->
      <div v-if="submitted && isSubjective" class="flex items-center gap-3 rounded-lg bg-blue-50 p-4 dark:bg-blue-900/20">
        <svg class="h-5 w-5 text-blue-600 dark:text-blue-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <p class="text-sm text-blue-800 dark:text-blue-300">
          {{ $t('courses.mcq.answerReview') }}
        </p>
      </div>

      <!-- Explanation -->
      <div v-if="submitted && mcq.explanation && !isSubjective" class="flex gap-3 rounded-lg bg-muted/20 p-4">
        <svg class="mt-0.5 h-4 w-4 flex-shrink-0 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <div>
          <p class="text-xs font-medium text-muted-foreground">{{ $t('courses.mcq.explanation') }}</p>
          <p class="mt-1 text-sm text-foreground">{{ mcq.explanation }}</p>
        </div>
      </div>

      <!-- Actions -->
      <div class="flex items-center gap-3">
        <AppButton
          v-if="!submitted"
          :disabled="selectedIndices.length === 0"
          @click="submitAnswer"
        >
          {{ $t('courses.mcq.submitAnswer') }}
        </AppButton>
        <AppButton
          v-if="submitted && !isSubjective && score < 0.7 && !readOnly"
          variant="secondary"
          size="sm"
          @click="tryAgain"
        >
          {{ $t('common.actions.retry') }}
        </AppButton>
      </div>
    </div>
  </div>
</template>
