<script setup lang="ts">
// Standalone skill assessment. Sentinel is auto-activated for the whole
// attempt (the learner is told, and sees the live integrity score); questions
// are drawn + shuffled per attempt and graded host-side (the answer key never
// reaches the client). Passing issues an integrity-bound AssessmentCredential
// that raises the skill's confidence.
import { onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useSentinel } from '@/composables/useSentinel'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAssessment } from '@/composables/useAssessment'
import { useSentinelView } from '@/composables/useSentinelView'
import { useDiagnostics } from '@/composables/useDiagnostics'
import { AppButton } from '@/components/ui'
import type { StartedAttempt, GradeResult } from '@/types'

const route = useRoute()
const router = useRouter()
const sentinel = useSentinel()
const sentinelView = useSentinelView()
const { startAttempt, saveDraft, submitAnswers, grade } = useAssessment()
const diagnostics = useDiagnostics()
const { invoke } = useLocalApi()

const skillId = String(route.params.skillId ?? '')
const attempt = ref<StartedAttempt | null>(null)
// selected[questionId] = Set of served option positions.
const selected = ref<Record<string, Set<number>>>({})
const loading = ref(true)
const grading = ref(false)
const submitted = ref(false)
const error = ref('')
const result = ref<GradeResult | null>(null)
const closing = ref(false)
const cleanupError = ref('')
let disposed = false
let monitoringClosed = false
let cleanupTask: Promise<void> | null = null
let unregisterDiagnostics: (() => void) | null = null

function currentAnswers() {
  if (!attempt.value) return []
  return attempt.value.questions.map(q => ({
    question_id: q.id,
    selected: [...(selected.value[q.id] ?? new Set())].sort((a, b) => a - b),
  }))
}

function stopMonitoring(): Promise<void> {
  if (monitoringClosed) return Promise.resolve()
  if (cleanupTask) return cleanupTask
  closing.value = true
  cleanupTask = sentinel.stop().then(() => { cleanupError.value = ''; monitoringClosed = true }).catch((e: unknown) => {
    if (!disposed) cleanupError.value = String(e)
    else console.warn('Assessment monitoring cleanup failed', e)
  }).finally(() => {
    closing.value = false
    cleanupTask = null
  })
  return cleanupTask
}

onMounted(async () => {
  try {
    const recovered = await invoke<GradeResult | null>('assessment_recover', { skillId, requestId: typeof route.query.request === 'string' ? route.query.request : null })
    if (disposed) return
    if (recovered) { result.value = recovered; monitoringClosed = true; return }
    // Auto-activate Sentinel for the assessment (standalone, no enrollment).
    await sentinel.start(null)
    if (disposed) return
    const sessionId = sentinel.getSessionId()
    if (!sessionId || !sentinel.isActive.value) throw new Error('Assessment monitoring is not active')
    const started = typeof route.query.request === 'string' && typeof route.query.directory === 'string'
      ? await invoke<StartedAttempt>('exchange_start_assessment', { directoryUrl: route.query.directory, requestId: route.query.request, integritySessionId: sessionId })
      : await startAttempt(skillId, sessionId)
    if (disposed) return
    attempt.value = started
    for (const q of attempt.value.questions) selected.value[q.id] = new Set()
    for (const answer of attempt.value.draft_answers) {
      selected.value[answer.question_id] = new Set(answer.selected)
    }
    unregisterDiagnostics = diagnostics.registerEntryPreparation(async () => {
      if (attempt.value && !result.value && !submitted.value) {
        await saveDraft(attempt.value.attempt_id, currentAnswers())
      }
    })
  } catch (e) {
    if (!disposed) {
      error.value = String(e)
      await stopMonitoring()
    }
  } finally {
    loading.value = false
  }
})

onUnmounted(() => {
  disposed = true
  unregisterDiagnostics?.()
  unregisterDiagnostics = null
  void stopMonitoring()
})

function toggle(qid: string, pos: number) {
  if (submitted.value || grading.value) return
  const set = selected.value[qid] ?? new Set<number>()
  set.has(pos) ? set.delete(pos) : set.add(pos)
  selected.value = { ...selected.value, [qid]: set }
}

async function submit() {
  if (!attempt.value || grading.value || result.value || disposed) return
  grading.value = true
  error.value = ''
  try {
    const answers = currentAnswers()
    await submitAnswers(attempt.value.attempt_id, answers)
    submitted.value = true
    await stopMonitoring()
    if (disposed) return
    if (cleanupError.value) throw new Error(cleanupError.value)
    const graded = await grade(attempt.value.attempt_id, answers)
    if (disposed) return
    result.value = graded
  } catch (e) {
    if (!disposed) error.value = String(e)
  } finally {
    grading.value = false
  }
}
</script>

<template>
  <div class="mx-auto max-w-2xl space-y-5 py-6">
    <div class="flex justify-end"><AppButton variant="outline" @click="sentinelView.toggle">{{ $t('profile.exchange.liveView') }}</AppButton></div>
    <!-- Sentinel notice (always shown during an attempt) -->
    <div v-if="sentinel.isActive.value" class="flex items-center gap-3 rounded-lg border border-border bg-card p-3 text-sm">
      <span
        class="h-2.5 w-2.5 rounded-full"
        :class="sentinel.isActive.value ? 'bg-success' : 'bg-muted-foreground'"
      />
      <div class="flex-1">
        <span class="font-medium text-foreground">{{ $t('learn.assessment.integrityOn') }}</span>
        <span class="text-muted-foreground">
          {{ $t('learn.assessment.integrityNote') }}
        </span>
      </div>
      <span class="font-mono text-xs text-muted-foreground">
        {{ Math.round(sentinel.integrityScore.value * 100) }}%
      </span>
    </div>

    <div v-if="loading" class="py-10 text-center text-sm text-muted-foreground">{{ $t('learn.assessment.preparing') }}</div>

    <div v-else-if="error && !attempt" class="rounded-lg border border-error/40 bg-error/5 p-4 text-sm text-error">
      {{ error }}
    </div>

    <!-- Result -->
    <div v-else-if="result" class="space-y-4 text-center">
      <div
        class="mx-auto flex h-16 w-16 items-center justify-center rounded-full text-2xl"
        :class="result.passed ? 'bg-success/15 text-success' : 'bg-warning/15 text-warning'"
      >
        {{ result.passed ? '✓' : '—' }}
      </div>
      <h1 class="text-xl font-bold text-foreground">
        {{ result.passed ? $t('learn.assessment.passed') : $t('learn.assessment.notPassedYet') }} — {{ Math.round(result.score * 100) }}%
      </h1>
      <p class="text-sm text-muted-foreground">
        {{ result.passed
          ? $t('learn.assessment.passedNote')
          : $t('learn.assessment.retryNote') }}
      </p>
      <div class="flex justify-center gap-2">
        <AppButton v-if="result.credential_id" @click="router.push('/credentials/' + encodeURIComponent(result.credential_id))">{{ $t('credentials.title') }}</AppButton>
        <AppButton v-if="result.credential_id" variant="outline" @click="router.push('/settings/directories')">{{ $t('profile.exchange.shareNext') }}</AppButton>
        <AppButton @click="router.push('/skills')">{{ $t('learn.assessment.viewSkills') }}</AppButton>
        <AppButton v-if="!result.passed" variant="outline" :disabled="closing || !!cleanupError" @click="router.go(0)">{{ $t('learn.assessment.retake') }}</AppButton>
      </div>
    </div>

    <!-- Questions -->
    <template v-else-if="attempt">
      <h1 class="text-xl font-bold text-foreground">{{ $t('learn.assessment.verifyTitle', { name: skillId.replace('skill_', '').replace(/_/g, ' ') }) }}</h1>
      <p class="text-sm text-muted-foreground">
        {{ $t('learn.assessment.selectAll', { percent: Math.round(attempt.pass_threshold * 100) }) }}
      </p>

      <div v-for="(q, qi) in attempt.questions" :key="q.id" class="rounded-xl border border-border p-4">
        <p class="mb-3 font-medium text-foreground">{{ qi + 1 }}. {{ q.prompt }}</p>
        <label
          v-for="(opt, pi) in q.options"
          :key="pi"
          class="mb-1.5 flex items-center gap-3 rounded-lg border border-border p-2.5 text-sm"
        >
          <input
            type="checkbox"
            :disabled="grading || submitted"
            :checked="selected[q.id]?.has(pi)"
            @change="toggle(q.id, pi)"
          />
          <span class="text-foreground">{{ opt }}</span>
        </label>
      </div>

      <p v-if="error" class="text-sm text-error">{{ error }}</p>
      <AppButton :loading="grading" @click="submit">{{ $t('learn.assessment.submit') }}</AppButton>
    </template>

    <div v-if="cleanupError" role="alert" class="space-y-2 rounded-lg border border-error/40 bg-error/5 p-4 text-sm text-error">
      <p>{{ cleanupError }}</p>
      <AppButton variant="outline" :loading="closing" @click="stopMonitoring">{{ $t('common.actions.retry') }}</AppButton>
    </div>
  </div>
</template>
