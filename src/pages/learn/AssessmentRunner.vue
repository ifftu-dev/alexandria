<script setup lang="ts">
// Standalone skill assessment. Sentinel is auto-activated for the whole
// attempt (the learner is told, and sees the live integrity score); questions
// are drawn + shuffled per attempt and graded host-side (the answer key never
// reaches the client). Passing issues an integrity-bound AssessmentCredential
// that raises the skill's confidence.
//
// When published sponsor roles cover the skill, the learner picks the role
// they are assessing for before the attempt starts. A role whose policy
// requires camera coverage has the camera turned on first, so the
// requirement applies from the first question instead of surfacing as a
// refusal at issuance.
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useSentinel } from '@/composables/useSentinel'
import { useAssessment } from '@/composables/useAssessment'
import { useCameraPresence } from '@/composables/useCameraPresence'
import { useDiagnostics } from '@/composables/useDiagnostics'
import { AppButton } from '@/components/ui'
import type { AttemptRoleTarget, StartedAttempt, GradeResult } from '@/types'

const route = useRoute()
const router = useRouter()
const sentinel = useSentinel()
const { openRoles, startAttempt, saveDraft, grade } = useAssessment()
const diagnostics = useDiagnostics()

const skillId = String(route.params.skillId ?? '')
const attempt = ref<StartedAttempt | null>(null)
// selected[questionId] = Set of served option positions.
const selected = ref<Record<string, Set<number>>>({})
const loading = ref(true)
const grading = ref(false)
const error = ref('')
const result = ref<GradeResult | null>(null)
const closing = ref(false)
const cleanupError = ref('')
let disposed = false
let cleanupTask: Promise<void> | null = null
let unregisterDiagnostics: (() => void) | null = null

// Role chooser (only when at least one published role covers the skill).
const roles = ref<AttemptRoleTarget[]>([])
const choosing = ref(false)
const selectedRoleId = ref('')
const selectedRole = computed(() =>
  roles.value.find(r => r.role_assessment_id === selectedRoleId.value) ?? null,
)

const cameraVideoRef = ref<HTMLVideoElement | null>(null)
const {
  stream: cameraStream,
  starting: cameraStarting,
  error: cameraError,
  lost: cameraLost,
  lastFacePresent,
  enable: enableCamera,
  bind: bindCamera,
  dispose: disposeCamera,
} = useCameraPresence(sentinel, cameraVideoRef)

const cameraRequired = computed(() =>
  attempt.value ? !!attempt.value.role?.camera_required : !!selectedRole.value?.camera_required,
)
const startBlocked = computed(() => loading.value || (cameraRequired.value && !cameraStream.value))

function currentAnswers() {
  if (!attempt.value) return []
  return attempt.value.questions.map(q => ({
    question_id: q.id,
    selected: [...(selected.value[q.id] ?? new Set())].sort((a, b) => a - b),
  }))
}

function stopMonitoring(): Promise<void> {
  if (cleanupTask) return cleanupTask
  closing.value = true
  disposeCamera()
  cleanupTask = sentinel.stop().then(() => { cleanupError.value = '' }).catch((e: unknown) => {
    if (!disposed) cleanupError.value = String(e)
    else console.warn('Assessment monitoring cleanup failed', e)
  }).finally(() => {
    closing.value = false
    cleanupTask = null
  })
  return cleanupTask
}

async function begin(roleAssessmentId: string | null) {
  if (disposed || attempt.value) return
  loading.value = true
  error.value = ''
  try {
    // Auto-activate Sentinel for the assessment (standalone, no enrollment).
    // A camera the learner already turned on is opted in from the start.
    await sentinel.start(null, cameraStream.value !== null)
    if (disposed) return
    const sessionId = sentinel.getSessionId()
    if (!sessionId || !sentinel.isActive.value) throw new Error('Assessment monitoring is not active')
    const started = await startAttempt(skillId, sessionId, roleAssessmentId)
    if (disposed) return
    attempt.value = started
    choosing.value = false
    for (const q of attempt.value.questions) selected.value[q.id] = new Set()
    for (const answer of attempt.value.draft_answers) {
      selected.value[answer.question_id] = new Set(answer.selected)
    }
    bindCamera()
    unregisterDiagnostics = diagnostics.registerEntryPreparation(async () => {
      if (attempt.value && !result.value) {
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
}

onMounted(async () => {
  try {
    roles.value = await openRoles(skillId)
  } catch (e) {
    if (!disposed) error.value = String(e)
    loading.value = false
    return
  }
  if (disposed) return
  if (roles.value.length === 0) {
    await begin(null)
    return
  }
  choosing.value = true
  loading.value = false
})

onUnmounted(() => {
  disposed = true
  unregisterDiagnostics?.()
  unregisterDiagnostics = null
  void stopMonitoring()
})

function toggle(qid: string, pos: number) {
  const set = selected.value[qid] ?? new Set<number>()
  set.has(pos) ? set.delete(pos) : set.add(pos)
  selected.value = { ...selected.value, [qid]: set }
}

async function submit() {
  if (!attempt.value || grading.value || result.value || disposed) return
  grading.value = true
  error.value = ''
  try {
    const graded = await grade(attempt.value.attempt_id, currentAnswers())
    if (disposed) return
    result.value = graded
    await stopMonitoring()
  } catch (e) {
    if (!disposed) error.value = String(e)
  } finally {
    grading.value = false
  }
}
</script>

<template>
  <div class="mx-auto max-w-2xl space-y-5 py-6">
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

    <div v-else-if="error && !attempt && !choosing" class="rounded-lg border border-error/40 bg-error/5 p-4 text-sm text-error">
      {{ error }}
    </div>

    <!-- Role chooser -->
    <div v-else-if="choosing && !attempt" class="space-y-4" data-testid="role-chooser">
      <h1 class="text-xl font-bold text-foreground">{{ $t('learn.assessment.verifyTitle', { name: skillId.replace('skill_', '').replace(/_/g, ' ') }) }}</h1>
      <p class="text-sm text-muted-foreground">{{ $t('learn.assessment.roleIntro') }}</p>
      <fieldset class="space-y-2">
        <label class="flex items-center gap-3 rounded-lg border border-border p-2.5 text-sm">
          <input v-model="selectedRoleId" type="radio" name="role" value="" data-testid="role-none" />
          <span class="text-foreground">{{ $t('learn.assessment.roleNone') }}</span>
        </label>
        <label
          v-for="r in roles"
          :key="r.role_assessment_id"
          class="flex items-center gap-3 rounded-lg border border-border p-2.5 text-sm"
        >
          <input v-model="selectedRoleId" type="radio" name="role" :value="r.role_assessment_id" :data-testid="`role-${r.role_assessment_id}`" />
          <span class="flex-1 text-foreground">{{ r.role_title }} — {{ r.org_name }}</span>
          <span v-if="r.camera_required" class="rounded bg-warning/15 px-1.5 py-0.5 text-[11px] font-medium text-warning">
            {{ $t('learn.assessment.roleCameraRequired') }}
          </span>
        </label>
      </fieldset>

      <div v-if="selectedRole?.camera_required" class="space-y-2 rounded-lg border border-border bg-card p-3 text-sm" data-testid="camera-gate">
        <p class="text-muted-foreground">{{ $t('learn.assessment.roleCameraNote') }}</p>
        <p v-if="cameraStream" class="font-medium text-success">{{ $t('learn.assessment.cameraReady') }}</p>
        <AppButton v-else variant="outline" size="sm" :loading="cameraStarting" data-testid="camera-enable" @click="enableCamera">
          {{ $t('learn.assessment.cameraEnable') }}
        </AppButton>
        <p v-if="cameraError" class="text-error">{{ cameraError }}</p>
      </div>

      <p v-if="error" class="text-sm text-error">{{ error }}</p>
      <AppButton :disabled="startBlocked" data-testid="start-attempt" @click="begin(selectedRoleId || null)">
        {{ $t('learn.assessment.startAttempt') }}
      </AppButton>
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

      <div v-if="attempt.role" class="space-y-1 rounded-lg border border-border bg-card p-3 text-sm" data-testid="role-banner">
        <p class="text-foreground">{{ $t('learn.assessment.roleBanner', { role: attempt.role.role_title, org: attempt.role.org_name }) }}</p>
        <template v-if="attempt.role.camera_required">
          <p v-if="cameraStream" class="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
            <span class="h-1.5 w-1.5 rounded-full" :class="lastFacePresent ? 'bg-success' : 'bg-muted-foreground/50'" />
            {{ lastFacePresent === null ? $t('learn.assessment.cameraConnecting') : lastFacePresent ? $t('learn.assessment.faceVerified') : $t('learn.assessment.noFaceDetected') }}
          </p>
          <div v-else class="flex items-center justify-between gap-2 text-xs text-warning" data-testid="camera-lost">
            <span>{{ cameraLost ? $t('learn.assessment.cameraLost') : $t('learn.assessment.roleCameraNote') }}</span>
            <AppButton variant="outline" size="sm" :loading="cameraStarting" @click="enableCamera">{{ $t('learn.assessment.cameraEnable') }}</AppButton>
          </div>
          <p v-if="cameraError" class="text-xs text-error">{{ cameraError }}</p>
        </template>
      </div>

      <div v-for="(q, qi) in attempt.questions" :key="q.id" class="rounded-xl border border-border p-4">
        <p class="mb-3 font-medium text-foreground">{{ qi + 1 }}. {{ q.prompt }}</p>
        <label
          v-for="(opt, pi) in q.options"
          :key="pi"
          class="mb-1.5 flex items-center gap-3 rounded-lg border border-border p-2.5 text-sm"
        >
          <input
            type="checkbox"
            :disabled="grading"
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

    <!-- Off-screen video the backend reads frames from. Rendered rather than
         display:none: WKWebView does not paint frames from a hidden video. -->
    <video
      v-if="cameraStream"
      ref="cameraVideoRef"
      class="pointer-events-none fixed -start-[10000px] top-0 h-[240px] w-[320px] opacity-0"
      playsinline
      autoplay
      muted
      width="320"
      height="240"
    />
  </div>
</template>
