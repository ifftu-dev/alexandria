<script setup lang="ts">
// Unified instructor inbox: pending IRL-review submissions + classroom
// join requests, oldest first.
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppTabs, AppTextarea, EmptyState } from '@/components/ui'
import type {
  CourseCompletionBinding,
  CourseCompletionEndorsement,
  InboxItem,
} from '@/types'

const { invoke } = useLocalApi()
const router = useRouter()
const { t } = useI18n()

const items = ref<InboxItem[]>([])
const loading = ref(true)
const activeTab = ref('all')
const endorsementRequestJson = ref('')
const reviewedBinding = ref<CourseCompletionBinding | null>(null)
const signedEndorsement = ref<CourseCompletionEndorsement | null>(null)
const endorsementError = ref('')
const endorsementMessage = ref('')
const endorsementSigning = ref(false)

const signedEndorsementJson = computed(() => signedEndorsement.value
  ? JSON.stringify(signedEndorsement.value, null, 2)
  : '')

const tabs = computed(() => [
  { key: 'all', label: t('instructor.inbox.tabAll'), count: items.value.length },
  { key: 'irl_submission', label: t('instructor.inbox.tabSubmissions'), count: items.value.filter(i => i.kind === 'irl_submission').length },
  { key: 'join_request', label: t('instructor.inbox.tabJoinRequests'), count: items.value.filter(i => i.kind === 'join_request').length },
])

const visible = computed(() =>
  activeTab.value === 'all' ? items.value : items.value.filter(i => i.kind === activeTab.value),
)

onMounted(refresh)

async function refresh() {
  loading.value = true
  try {
    items.value = await invoke<InboxItem[]>('instructor_inbox')
  } finally {
    loading.value = false
  }
}

function open(item: InboxItem) {
  if (item.kind === 'irl_submission') {
    router.push(`/instructor/review/${item.target_id}`)
  } else {
    router.push(`/classrooms/${item.target_id}`)
  }
}

function iconFor(kind: InboxItem['kind']): string {
  return kind === 'irl_submission'
    ? 'M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z'
    : 'M18 9v3m0 0v3m0-3h3m-3 0h-3m-2-5a4 4 0 11-8 0 4 4 0 018 0zM3 20a6 6 0 0112 0v1H3v-1z'
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isCompletionBinding(value: unknown): value is CourseCompletionBinding {
  if (!isRecord(value) || !Array.isArray(value.evidence)) return false
  return value.format_version === 1
    && typeof value.network_id === 'string'
    && typeof value.subject_did === 'string'
    && typeof value.course_id === 'string'
    && typeof value.course_document_cid === 'string'
    && typeof value.course_document_version === 'number'
    && Number.isInteger(value.course_document_version)
    && typeof value.completion_root === 'string'
    && (value.witness_tx_hash === undefined
      || value.witness_tx_hash === null
      || typeof value.witness_tx_hash === 'string')
    && value.evidence.every(item => isRecord(item)
      && typeof item.kind === 'string'
      && typeof item.format_version === 'number'
      && Number.isInteger(item.format_version)
      && typeof item.id === 'string'
      && typeof item.digest === 'string')
}

watch(endorsementRequestJson, () => {
  reviewedBinding.value = null
  signedEndorsement.value = null
  endorsementError.value = ''
  endorsementMessage.value = ''
})

function reviewEndorsementRequest() {
  endorsementError.value = ''
  endorsementMessage.value = ''
  try {
    const parsed: unknown = JSON.parse(endorsementRequestJson.value)
    if (!isCompletionBinding(parsed)) {
      throw new Error(t('instructor.inbox.endorsementInvalidRequest'))
    }
    reviewedBinding.value = parsed
  } catch (error) {
    endorsementError.value = String(error)
  }
}

async function signEndorsementRequest() {
  if (!reviewedBinding.value) return
  endorsementSigning.value = true
  endorsementError.value = ''
  endorsementMessage.value = ''
  try {
    signedEndorsement.value = await invoke<CourseCompletionEndorsement>(
      'sign_course_completion_endorsement',
      { binding: reviewedBinding.value },
    )
    endorsementMessage.value = t('instructor.inbox.endorsementSigned')
  } catch (error) {
    endorsementError.value = String(error)
  } finally {
    endorsementSigning.value = false
  }
}

async function copySignedEndorsement() {
  if (!signedEndorsementJson.value) return
  try {
    await navigator.clipboard.writeText(signedEndorsementJson.value)
    endorsementMessage.value = t('instructor.inbox.endorsementCopied')
  } catch (error) {
    endorsementError.value = t('instructor.inbox.endorsementCopyFailed', { error: String(error) })
  }
}
</script>

<template>
  <div class="space-y-6">
    <div>
      <h1 class="text-2xl font-bold text-foreground">{{ $t('instructor.inbox.title') }}</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        {{ $t('instructor.inbox.subtitle') }}
      </p>
    </div>

    <section class="rounded-xl border border-border bg-card p-5 space-y-4">
      <div>
        <h2 class="font-semibold text-foreground">{{ $t('instructor.inbox.endorsementTitle') }}</h2>
        <p class="mt-1 text-sm text-muted-foreground">{{ $t('instructor.inbox.endorsementDescription') }}</p>
      </div>
      <AppTextarea
        v-model="endorsementRequestJson"
        :label="$t('instructor.inbox.endorsementRequestLabel')"
        :placeholder="$t('instructor.inbox.endorsementRequestPlaceholder')"
        :rows="6"
      />
      <AppButton
        size="sm"
        :disabled="!endorsementRequestJson.trim()"
        @click="reviewEndorsementRequest"
      >
        {{ $t('instructor.inbox.endorsementReview') }}
      </AppButton>

      <div v-if="reviewedBinding" class="rounded-lg border border-border/70 bg-background/50 p-4 space-y-3">
        <h3 class="text-sm font-semibold text-foreground">{{ $t('instructor.inbox.endorsementFactsTitle') }}</h3>
        <dl class="grid gap-3 text-xs sm:grid-cols-2">
          <div><dt class="text-muted-foreground">{{ $t('instructor.inbox.endorsementNetwork') }}</dt><dd class="break-all font-mono text-foreground">{{ reviewedBinding.network_id }}</dd></div>
          <div><dt class="text-muted-foreground">{{ $t('instructor.inbox.endorsementSubject') }}</dt><dd class="break-all font-mono text-foreground">{{ reviewedBinding.subject_did }}</dd></div>
          <div><dt class="text-muted-foreground">{{ $t('instructor.inbox.endorsementCourse') }}</dt><dd class="break-all font-mono text-foreground">{{ reviewedBinding.course_id }}</dd></div>
          <div><dt class="text-muted-foreground">{{ $t('instructor.inbox.endorsementVersion') }}</dt><dd class="font-mono text-foreground">{{ reviewedBinding.course_document_version }}</dd></div>
          <div class="sm:col-span-2"><dt class="text-muted-foreground">{{ $t('instructor.inbox.endorsementDocument') }}</dt><dd class="break-all font-mono text-foreground">{{ reviewedBinding.course_document_cid }}</dd></div>
          <div class="sm:col-span-2"><dt class="text-muted-foreground">{{ $t('instructor.inbox.endorsementRoot') }}</dt><dd class="break-all font-mono text-foreground">{{ reviewedBinding.completion_root }}</dd></div>
          <div v-if="reviewedBinding.witness_tx_hash" class="sm:col-span-2"><dt class="text-muted-foreground">{{ $t('instructor.inbox.endorsementWitness') }}</dt><dd class="break-all font-mono text-foreground">{{ reviewedBinding.witness_tx_hash }}</dd></div>
        </dl>
        <div>
          <h4 class="text-xs font-medium text-muted-foreground">
            {{ $t('instructor.inbox.endorsementEvidence', { count: reviewedBinding.evidence.length }, reviewedBinding.evidence.length) }}
          </h4>
          <ul class="mt-1 space-y-1">
            <li v-for="evidence in reviewedBinding.evidence" :key="`${evidence.kind}-${evidence.id}`" class="break-all font-mono text-xs text-foreground">
              {{ evidence.kind }} v{{ evidence.format_version }} · {{ evidence.id }} · {{ evidence.digest }}
            </li>
          </ul>
        </div>
        <p class="text-xs text-warning">{{ $t('instructor.inbox.endorsementSignWarning') }}</p>
        <AppButton :loading="endorsementSigning" @click="signEndorsementRequest">
          {{ $t('instructor.inbox.endorsementSign') }}
        </AppButton>
      </div>

      <div v-if="signedEndorsement" class="space-y-3">
        <AppTextarea
          :model-value="signedEndorsementJson"
          :label="$t('instructor.inbox.endorsementSignedLabel')"
          :rows="6"
          disabled
        />
        <AppButton size="sm" variant="secondary" @click="copySignedEndorsement">
          {{ $t('instructor.inbox.endorsementCopy') }}
        </AppButton>
      </div>
      <p v-if="endorsementMessage" class="text-sm text-success" role="status">{{ endorsementMessage }}</p>
      <p v-if="endorsementError" class="text-sm text-error" role="alert">{{ endorsementError }}</p>
    </section>

    <AppTabs v-model="activeTab" :tabs="tabs" />

    <div v-if="loading" class="space-y-2">
      <div v-for="i in 3" :key="i" class="h-16 animate-pulse rounded-lg bg-muted-foreground/8" />
    </div>

    <EmptyState
      v-else-if="!visible.length"
      :title="$t('instructor.inbox.emptyTitle')"
      :description="$t('instructor.inbox.emptyDesc')"
    />

    <div v-else class="space-y-2">
      <button
        v-for="item in visible"
        :key="`${item.kind}-${item.id}`"
        class="flex w-full items-center gap-3 rounded-xl border border-border bg-card p-4 text-start transition-colors hover:border-primary/50"
        @click="open(item)"
      >
        <span class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
          <svg class="h-4.5 w-4.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
            <path stroke-linecap="round" stroke-linejoin="round" :d="iconFor(item.kind)" />
          </svg>
        </span>
        <span class="min-w-0 flex-1">
          <span class="block truncate text-sm font-medium text-foreground">{{ item.title }}</span>
          <span v-if="item.subtitle" class="block truncate text-xs text-muted-foreground">{{ item.subtitle }}</span>
        </span>
        <span class="shrink-0 text-xs text-muted-foreground">{{ item.created_at.slice(0, 16) }}</span>
      </button>
    </div>
  </div>
</template>
