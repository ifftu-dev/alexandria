<script setup lang="ts">
// Standalone skill assessment. Sentinel is auto-activated for the whole
// attempt (the learner is told, and sees the live integrity score); questions
// are drawn + shuffled per attempt and graded host-side (the answer key never
// reaches the client). Passing issues an integrity-bound AssessmentCredential
// that raises the skill's confidence.
import { onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useSentinel } from '@/composables/useSentinel'
import { useAssessment } from '@/composables/useAssessment'
import { AppButton } from '@/components/ui'
import type { StartedAttempt, GradeResult } from '@/types'

const route = useRoute()
const router = useRouter()
const sentinel = useSentinel()
const { startAttempt, grade } = useAssessment()

const skillId = String(route.params.skillId ?? '')
const attempt = ref<StartedAttempt | null>(null)
// selected[questionId] = Set of served option positions.
const selected = ref<Record<string, Set<number>>>({})
const loading = ref(true)
const grading = ref(false)
const error = ref('')
const result = ref<GradeResult | null>(null)

onMounted(async () => {
  try {
    // Auto-activate Sentinel for the assessment (standalone, no enrollment).
    await sentinel.start(null)
    const sessionId = sentinel.getSessionId()
    attempt.value = await startAttempt(skillId, sessionId)
    for (const q of attempt.value.questions) selected.value[q.id] = new Set()
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
})

onUnmounted(() => {
  void sentinel.stop()
})

function toggle(qid: string, pos: number) {
  const set = selected.value[qid] ?? new Set<number>()
  set.has(pos) ? set.delete(pos) : set.add(pos)
  selected.value = { ...selected.value, [qid]: set }
}

async function submit() {
  if (!attempt.value) return
  grading.value = true
  error.value = ''
  try {
    const answers = attempt.value.questions.map((q) => ({
      question_id: q.id,
      selected: [...(selected.value[q.id] ?? new Set())].sort((a, b) => a - b),
    }))
    result.value = await grade(attempt.value.attempt_id, answers)
  } catch (e) {
    error.value = String(e)
  } finally {
    grading.value = false
    void sentinel.stop()
  }
}
</script>

<template>
  <div class="mx-auto max-w-2xl space-y-5 py-6">
    <!-- Sentinel notice (always shown during an attempt) -->
    <div class="flex items-center gap-3 rounded-lg border border-border bg-card p-3 text-sm">
      <span
        class="h-2.5 w-2.5 rounded-full"
        :class="sentinel.isActive.value ? 'bg-success' : 'bg-muted-foreground'"
      />
      <div class="flex-1">
        <span class="font-medium text-foreground">Integrity monitoring is on.</span>
        <span class="text-muted-foreground">
          This is a graded assessment, so Sentinel confirms it's you. Everything
          runs on your device — only an integrity score is recorded.
        </span>
      </div>
      <span class="font-mono text-xs text-muted-foreground">
        {{ Math.round(sentinel.integrityScore.value * 100) }}%
      </span>
    </div>

    <div v-if="loading" class="py-10 text-center text-sm text-muted-foreground">Preparing your assessment…</div>

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
        {{ result.passed ? 'Passed' : 'Not passed yet' }} — {{ Math.round(result.score * 100) }}%
      </h1>
      <p class="text-sm text-muted-foreground">
        {{ result.passed
          ? 'A verified assessment credential was issued — your confidence in this skill has gone up.'
          : 'Review the material and try again — a fresh set of questions is drawn each attempt.' }}
      </p>
      <div class="flex justify-center gap-2">
        <AppButton @click="router.push('/skills')">View my skills</AppButton>
        <AppButton v-if="!result.passed" variant="outline" @click="router.go(0)">Retake</AppButton>
      </div>
    </div>

    <!-- Questions -->
    <template v-else-if="attempt">
      <h1 class="text-xl font-bold text-foreground">Verify: {{ skillId.replace('skill_', '').replace(/_/g, ' ') }}</h1>
      <p class="text-sm text-muted-foreground">
        Select all correct answers. Pass mark {{ Math.round(attempt.pass_threshold * 100) }}%.
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
            :checked="selected[q.id]?.has(pi)"
            @change="toggle(q.id, pi)"
          />
          <span class="text-foreground">{{ opt }}</span>
        </label>
      </div>

      <p v-if="error" class="text-sm text-error">{{ error }}</p>
      <AppButton :loading="grading" @click="submit">Submit assessment</AppButton>
    </template>
  </div>
</template>
