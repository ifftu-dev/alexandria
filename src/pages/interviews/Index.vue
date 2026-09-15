<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useInterviews } from '@/composables/useInterviews'
import { useSponsor } from '@/composables/useSponsor'
import { AppAlert, AppBadge, AppButton, AppInput, AppModal, AppTextarea, EmptyState } from '@/components/ui'
import type { CreateInterviewRequest, InterviewParticipantRole, InterviewStatus } from '@/types'

const router = useRouter()
const { interviews, loading, error, listInterviews, createInterview, deleteInterview } = useInterviews()
const sponsor = useSponsor()

const showCreate = ref(false)
const showDelete = ref<string | null>(null)
const title = ref('')
const objective = ref('')
const candidateName = ref('')
const interviewerName = ref('')
const criteriaText = ref('Communication\nProblem solving\nTechnical depth\nReflection and learning')
const durationMinutes = ref('45')
const retentionDays = ref('30')
const sentinelEnabled = ref(true)
const recordAudio = ref(false)
const recordVideo = ref(false)
const roleAssessmentId = ref('')

const activeInterviews = computed(() => interviews.value.filter(item => item.status !== 'completed'))
const completedInterviews = computed(() => interviews.value.filter(item => item.status === 'completed'))

onMounted(() => {
  listInterviews().catch(() => undefined)
  sponsor.listRoleAssessments().catch(() => undefined)
})

watch(roleAssessmentId, (id) => {
  const assessment = sponsor.roleAssessments.value.find(item => item.id === id)
  if (!assessment) return
  if (!title.value.trim()) title.value = `${assessment.role_title} interview`
  if (!objective.value.trim() && assessment.job_description) objective.value = assessment.job_description
})

function statusVariant(status: InterviewStatus): 'primary' | 'success' | 'warning' | 'secondary' {
  if (status === 'live') return 'success'
  if (status === 'ready') return 'primary'
  if (status === 'completed') return 'secondary'
  return 'warning'
}

function resetForm() {
  title.value = ''
  objective.value = ''
  candidateName.value = ''
  criteriaText.value = 'Communication\nProblem solving\nTechnical depth\nReflection and learning'
  durationMinutes.value = '45'
  retentionDays.value = '30'
  sentinelEnabled.value = true
  recordAudio.value = false
  recordVideo.value = false
  roleAssessmentId.value = ''
}

async function submitCreate() {
  if (!title.value.trim() || !candidateName.value.trim()) return
  const participants: CreateInterviewRequest['participants'] = [
    {
      display_name: candidateName.value.trim(),
      role: 'candidate' satisfies InterviewParticipantRole,
    },
  ]
  if (interviewerName.value.trim()) {
    participants.unshift({
      display_name: interviewerName.value.trim(),
      role: 'interviewer' satisfies InterviewParticipantRole,
    })
  }
  const bundle = await createInterview({
    title: title.value.trim(),
    objective: objective.value.trim() || undefined,
    role_assessment_id: roleAssessmentId.value || undefined,
    duration_minutes: Number(durationMinutes.value),
    retention_days: Number(retentionDays.value),
    record_audio: recordAudio.value,
    record_video: recordVideo.value,
    sentinel_enabled: sentinelEnabled.value,
    participants,
    criteria: criteriaText.value.split('\n').map(item => item.trim()).filter(Boolean),
  })
  showCreate.value = false
  resetForm()
  await router.push(`/interviews/${bundle.session.id}`)
}

async function confirmDelete() {
  if (!showDelete.value) return
  await deleteInterview(showDelete.value)
  showDelete.value = null
}

function formatDate(value: string) {
  return new Date(value).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-6 pb-10">
    <header class="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
      <div>
        <div class="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-[0.16em] text-primary">
          <span class="h-1.5 w-1.5 rounded-full bg-primary" />
          {{ $t('interviews.index.eyebrow') }}
        </div>
        <h1 class="text-2xl font-semibold tracking-tight text-foreground">{{ $t('interviews.index.title') }}</h1>
        <p class="mt-1 max-w-2xl text-sm text-muted-foreground">{{ $t('interviews.index.subtitle') }}</p>
      </div>
      <AppButton @click="showCreate = true">{{ $t('interviews.index.new') }}</AppButton>
    </header>

    <AppAlert v-if="error" variant="error">{{ error }}</AppAlert>

    <section class="grid gap-3 sm:grid-cols-3">
      <div class="card p-4">
        <p class="text-xs font-medium text-muted-foreground">{{ $t('interviews.index.activePlans') }}</p>
        <p class="mt-2 text-2xl font-semibold tabular-nums text-foreground">{{ activeInterviews.length }}</p>
      </div>
      <div class="card p-4">
        <p class="text-xs font-medium text-muted-foreground">{{ $t('interviews.index.completedLocally') }}</p>
        <p class="mt-2 text-2xl font-semibold tabular-nums text-foreground">{{ completedInterviews.length }}</p>
      </div>
      <div class="card border-primary/20 bg-primary/[0.035] p-4">
        <p class="text-xs font-medium text-primary">{{ $t('interviews.index.privacyPosture') }}</p>
        <p class="mt-2 text-sm font-medium text-foreground">{{ $t('interviews.index.privacyValue') }}</p>
      </div>
    </section>

    <section>
      <div class="mb-3 flex items-center justify-between">
        <h2 class="text-sm font-semibold text-foreground">{{ $t('interviews.index.yourInterviews') }}</h2>
        <span class="text-xs text-muted-foreground">{{ $t('interviews.index.total', { count: interviews.length }) }}</span>
      </div>

      <EmptyState
        v-if="!loading && interviews.length === 0"
        :title="$t('interviews.index.emptyTitle')"
        :description="$t('interviews.index.emptyDescription')"
      >
        <template #action><AppButton size="sm" @click="showCreate = true">{{ $t('interviews.index.create') }}</AppButton></template>
      </EmptyState>

      <div v-else class="grid gap-3 lg:grid-cols-2">
        <article v-for="interview in interviews" :key="interview.id" class="card group p-5 transition-shadow hover:shadow-md">
          <div class="flex items-start justify-between gap-4">
            <div class="min-w-0">
              <div class="flex items-center gap-2">
                <AppBadge :variant="statusVariant(interview.status)">{{ interview.status }}</AppBadge>
                <span class="text-xs text-muted-foreground">{{ $t('interviews.index.minutes', { count: interview.duration_minutes }) }}</span>
              </div>
              <h3 class="mt-3 truncate text-base font-semibold text-foreground">{{ interview.title }}</h3>
              <p class="mt-1 line-clamp-2 text-sm text-muted-foreground">{{ interview.objective || 'Structured evidence-based interview' }}</p>
            </div>
            <button class="rounded-md p-1.5 text-muted-foreground opacity-60 hover:bg-destructive/10 hover:text-destructive group-hover:opacity-100" :title="$t('interviews.index.deleteTitle')" @click="showDelete = interview.id">
              <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M14.74 9l-.35 9m-4.78 0L9.26 9m9.97-3.21c.34.05.68.1 1.02.16m-1.02-.16L18.07 20.7A2.25 2.25 0 0115.82 22H8.18a2.25 2.25 0 01-2.24-2.05L4.77 5.79m14.46 0a48.1 48.1 0 00-3.48-.4m-12 .56c.34-.06.68-.11 1.02-.16m0 0a48.1 48.1 0 013.48-.4m7.5 0V4.48c0-1.18-.91-2.16-2.09-2.2a52 52 0 00-3.32 0c-1.18.04-2.09 1.02-2.09 2.2v.91m7.5 0a48.7 48.7 0 00-7.5 0" /></svg>
            </button>
          </div>
          <div class="mt-5 flex items-center justify-between border-t border-border pt-4">
            <div>
              <p class="text-[0.68rem] uppercase tracking-wide text-muted-foreground">{{ $t('interviews.index.created') }}</p>
              <p class="mt-0.5 text-xs text-foreground">{{ formatDate(interview.created_at) }}</p>
            </div>
            <AppButton size="sm" :variant="interview.status === 'completed' ? 'outline' : 'primary'" @click="router.push(interview.status === 'completed' ? `/interviews/${interview.id}/review` : `/interviews/${interview.id}`)">
              {{ interview.status === 'completed' ? 'Review' : interview.status === 'live' ? 'Rejoin' : 'Open plan' }}
            </AppButton>
          </div>
        </article>
      </div>
    </section>

    <AppModal :open="showCreate" :title="$t('interviews.index.prepareTitle')" max-width="44rem" @close="showCreate = false">
      <form class="space-y-5" @submit.prevent="submitCreate">
        <div class="grid gap-4 sm:grid-cols-2">
          <label class="sm:col-span-2"><span class="label text-xs text-muted-foreground">{{ $t('interviews.index.roleAssessment') }}</span><select v-model="roleAssessmentId" class="input"><option value="">{{ $t('interviews.index.standalone') }}</option><option v-for="assessment in sponsor.roleAssessments.value" :key="assessment.id" :value="assessment.id">{{ assessment.role_title }} · {{ assessment.status }}</option></select></label>
          <div class="sm:col-span-2"><AppInput v-model="title" :label="$t('interviews.index.titleLabel')" :placeholder="$t('interviews.index.titlePlaceholder')" /></div>
          <AppInput v-model="candidateName" :label="$t('interviews.index.candidateLabel')" :placeholder="$t('interviews.index.candidatePlaceholder')" />
          <AppInput v-model="interviewerName" :label="$t('interviews.index.interviewerLabel')" :placeholder="$t('interviews.index.interviewerPlaceholder')" />
          <div class="sm:col-span-2"><AppTextarea v-model="objective" :label="$t('interviews.index.objectiveLabel')" :placeholder="$t('interviews.index.objectivePlaceholder')" :rows="2" /></div>
          <div class="sm:col-span-2"><AppTextarea v-model="criteriaText" :label="$t('interviews.index.criteriaLabel')" :rows="5" /></div>
          <AppInput v-model="durationMinutes" :label="$t('interviews.index.durationLabel')" type="number" />
          <AppInput v-model="retentionDays" :label="$t('interviews.index.retentionLabel')" type="number" />
        </div>

        <div class="rounded-xl border border-border bg-muted/25 p-4">
          <p class="text-sm font-semibold text-foreground">{{ $t('interviews.index.recordTitle') }}</p>
          <p class="mt-1 text-xs leading-relaxed text-muted-foreground">{{ $t('interviews.index.recordDescription') }}</p>
          <div class="mt-4">
            <label class="flex items-center gap-2 text-sm text-foreground"><input v-model="sentinelEnabled" type="checkbox" class="accent-primary" /> {{ $t('interviews.index.sentinelSignals') }}</label>
          </div>
        </div>

        <div class="flex justify-end gap-2">
          <AppButton type="button" variant="outline" @click="showCreate = false">{{ $t('interviews.common.cancel') }}</AppButton>
          <AppButton type="submit" :loading="loading" :disabled="!title.trim() || !candidateName.trim()">{{ $t('interviews.index.createPlan') }}</AppButton>
        </div>
      </form>
    </AppModal>

    <AppModal :open="Boolean(showDelete)" :title="$t('interviews.index.deleteConfirmTitle')" max-width="28rem" @close="showDelete = null">
      <p class="text-sm leading-relaxed text-muted-foreground">{{ $t('interviews.index.deleteConfirmDescription') }}</p>
      <template #footer>
        <div class="flex justify-end gap-2">
          <AppButton variant="outline" @click="showDelete = null">{{ $t('interviews.common.cancel') }}</AppButton>
          <AppButton variant="danger" @click="confirmDelete">{{ $t('interviews.index.deleteLocal') }}</AppButton>
        </div>
      </template>
    </AppModal>
  </div>
</template>
