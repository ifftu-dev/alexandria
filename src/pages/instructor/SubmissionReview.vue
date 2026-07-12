<script setup lang="ts">
// Review one pending IRL submission: inspect the payload, score it
// (0–100 in the UI, 0–1 on the wire), rate each declared skill, and
// leave feedback. Wraps the existing `irl_post_review` command.
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppBadge, EmptyState, StatusBadge } from '@/components/ui'
import type { IrlSubmission } from '@/types'

const { invoke } = useLocalApi()
const route = useRoute()
const router = useRouter()

const submissionId = route.params.id as string

const submission = ref<IrlSubmission | null>(null)
const loading = ref(true)
const error = ref('')

const scorePct = ref(80)
const feedback = ref('')
const skillRatings = ref<Record<string, number>>({})
const submitting = ref(false)

const declaredSkills = computed<string[]>(() => {
  try {
    const parsed = JSON.parse(submission.value?.skills_json ?? '[]')
    return Array.isArray(parsed) ? parsed.map(String) : []
  } catch {
    return []
  }
})

const prettyPayload = computed(() => {
  try {
    return JSON.stringify(JSON.parse(submission.value?.submission_json ?? ''), null, 2)
  } catch {
    return submission.value?.submission_json ?? ''
  }
})

onMounted(async () => {
  try {
    submission.value = await invoke<IrlSubmission | null>('irl_get_submission', { submissionId })
    for (const s of declaredSkills.value) skillRatings.value[s] = 0.8
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
})

async function postReview() {
  submitting.value = true
  error.value = ''
  try {
    await invoke('irl_post_review', {
      submissionId,
      score: Math.min(100, Math.max(0, scorePct.value)) / 100,
      feedback: feedback.value.trim(),
      skillRatingsJson: JSON.stringify(skillRatings.value),
    })
    router.replace('/instructor/inbox')
  } catch (e) {
    error.value = String(e)
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="max-w-3xl space-y-6">
    <div>
      <p class="text-xs uppercase tracking-wide text-muted-foreground">{{ $t('instructor.submissionReview.eyebrow') }}</p>
      <h1 class="text-2xl font-bold text-foreground">{{ $t('instructor.submissionReview.title') }}</h1>
    </div>

    <div v-if="loading" class="h-64 animate-pulse rounded-xl bg-muted-foreground/8" />

    <EmptyState
      v-else-if="!submission"
      :title="$t('instructor.submissionReview.notFoundTitle')"
      :description="$t('instructor.submissionReview.notFoundDesc')"
    />

    <template v-else>
      <div class="rounded-xl border border-border bg-card p-5 space-y-3">
        <div class="flex flex-wrap items-center gap-2 text-sm">
          <StatusBadge :status="submission.status" />
          <span class="text-muted-foreground">{{ $t('instructor.submissionReview.from') }}</span>
          <code class="text-xs text-muted-foreground">{{ submission.learner_did.slice(0, 32) }}…</code>
          <span class="ms-auto text-xs text-muted-foreground">{{ submission.created_at.slice(0, 16) }}</span>
        </div>
        <div v-if="declaredSkills.length" class="flex flex-wrap gap-1.5">
          <AppBadge v-for="s in declaredSkills" :key="s" size="xs">{{ s }}</AppBadge>
        </div>
        <div>
          <p class="mb-1 text-xs font-medium text-muted-foreground">{{ $t('instructor.submissionReview.submittedWork') }}</p>
          <pre class="max-h-80 overflow-auto rounded-lg bg-muted/20 p-3 font-mono text-xs">{{ prettyPayload }}</pre>
        </div>
      </div>

      <div v-if="submission.status === 'pending'" class="rounded-xl border border-border bg-card p-5 space-y-4">
        <h2 class="text-sm font-semibold text-foreground">{{ $t('instructor.submissionReview.yourReview') }}</h2>

        <div>
          <label class="mb-1 block text-xs font-medium text-muted-foreground">
            {{ $t('instructor.submissionReview.scoreLabel', { percent: scorePct }) }}
          </label>
          <input v-model.number="scorePct" type="range" min="0" max="100" step="5" class="w-full">
        </div>

        <div v-if="declaredSkills.length" class="space-y-2">
          <p class="text-xs font-medium text-muted-foreground">{{ $t('instructor.submissionReview.perSkillRatings') }}</p>
          <div v-for="s in declaredSkills" :key="s" class="flex items-center gap-3">
            <span class="w-48 truncate text-sm text-foreground">{{ s }}</span>
            <input
              v-model.number="skillRatings[s]"
              type="range"
              min="0"
              max="1"
              step="0.05"
              class="flex-1"
            >
            <span class="w-10 text-end text-xs text-muted-foreground">
              {{ Math.round((skillRatings[s] ?? 0) * 100) }}%
            </span>
          </div>
        </div>

        <div>
          <label class="mb-1 block text-xs font-medium text-muted-foreground">{{ $t('instructor.submissionReview.feedback') }}</label>
          <textarea
            v-model="feedback"
            rows="4"
            class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
            :placeholder="$t('instructor.submissionReview.feedbackPlaceholder')"
          />
        </div>

        <p v-if="error" class="text-sm text-error">{{ error }}</p>

        <div class="flex gap-2">
          <AppButton :loading="submitting" @click="postReview">{{ $t('instructor.submissionReview.postReview') }}</AppButton>
          <AppButton variant="ghost" @click="router.back()">{{ $t('common.actions.cancel') }}</AppButton>
        </div>
      </div>

      <div v-else class="rounded-xl border border-border bg-card p-5 space-y-2">
        <h2 class="text-sm font-semibold text-foreground">{{ $t('instructor.submissionReview.reviewed') }}</h2>
        <p class="text-sm text-foreground">
          {{ $t('instructor.submissionReview.finalScore') }} {{ submission.score === null ? '—' : `${Math.round(submission.score * 100)}%` }}
        </p>
        <p v-if="submission.feedback" class="text-sm text-muted-foreground">{{ submission.feedback }}</p>
      </div>
    </template>
  </div>
</template>
