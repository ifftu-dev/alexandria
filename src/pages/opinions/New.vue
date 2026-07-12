<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { invoke } from '@tauri-apps/api/core'
import { AppButton, AppInput, AppTextarea, AppAlert, AppBadge } from '@/components/ui'
import {
  extractSkillClaim,
  type SubjectFieldInfo,
  type SkillInfo,
  type OpinionRow,
  type PublishOpinionRequest,
  type VerifiableCredential,
} from '@/types'

const { t } = useI18n()
const router = useRouter()

const SUMMARY_MAX = 280
const APPLY_LEVEL = 2 // Bloom's apply — minimum to post an opinion.

// -----------------------------------------------------------------------------
// State
// -----------------------------------------------------------------------------

const subjectFieldId = ref<string>('')
const title = ref('')
const summary = ref('')

const videoFile = ref<File | null>(null)
const videoHash = ref<string | null>(null)
const videoDuration = ref<number | null>(null)
const videoUploading = ref(false)
const videoProgress = ref('')

const thumbHash = ref<string | null>(null)
const thumbUploading = ref(false)

const myCredentials = ref<VerifiableCredential[]>([])
const allSkills = ref<SkillInfo[]>([])
const eligibleFields = ref<string[]>([])
const subjectFields = ref<SubjectFieldInfo[]>([])
const localDid = ref<string | null>(null)

const selectedCredentialIds = ref<Set<string>>(new Set())

const submitting = ref(false)
const error = ref('')

// -----------------------------------------------------------------------------
// Derived
// -----------------------------------------------------------------------------

const eligibleFieldsInfo = computed(() =>
  subjectFields.value.filter((f) => eligibleFields.value.includes(f.id)),
)

/**
 * Credentials that qualify the author to post in the currently
 * selected subject field:
 *   - SkillClaim with level >= apply (2)
 *   - the SkillClaim's `skillId` lives under the selected
 *     subject_field_id
 *   - subject == local DID (their own credential)
 */
const qualifyingCredentialsForField = computed<VerifiableCredential[]>(() => {
  if (!subjectFieldId.value) return []
  const skillById = new Map(allSkills.value.map((s) => [s.id, s]))
  return myCredentials.value.filter((vc) => {
    if (localDid.value && vc.credentialSubject.id !== localDid.value) return false
    const claim = extractSkillClaim(vc.credentialSubject)
    if (!claim) return false
    if (claim.level < APPLY_LEVEL) return false
    const skill = skillById.get(claim.skillId)
    return skill?.subject_field_id === subjectFieldId.value
  })
})

const summaryLength = computed(() => summary.value.length)

const canSubmit = computed(
  () =>
    subjectFieldId.value.length > 0 &&
    title.value.trim().length > 0 &&
    summaryLength.value <= SUMMARY_MAX &&
    videoHash.value !== null &&
    selectedCredentialIds.value.size > 0 &&
    !submitting.value &&
    !videoUploading.value &&
    !thumbUploading.value,
)

// -----------------------------------------------------------------------------
// Init
// -----------------------------------------------------------------------------

onMounted(async () => {
  try {
    const [fields, eligible, did, creds, skills] = await Promise.all([
      invoke<SubjectFieldInfo[]>('list_subject_fields', {}),
      invoke<string[]>('list_eligible_subject_fields_for_posting'),
      invoke<string | null>('get_local_did').catch(() => null),
      invoke<VerifiableCredential[]>('list_credentials', {}).catch(() => []),
      invoke<SkillInfo[]>('list_skills', {}),
    ])
    subjectFields.value = fields
    eligibleFields.value = eligible
    localDid.value = did
    myCredentials.value = creds
    allSkills.value = skills

    const first = eligible[0]
    if (first !== undefined) {
      subjectFieldId.value = first
      autoSelectCredentials()
    }
  } catch (e) {
    error.value = t('opinions.new.loadFailed', { error: String(e) })
  }
})

function onFieldChange() {
  selectedCredentialIds.value = new Set()
  autoSelectCredentials()
}

function autoSelectCredentials() {
  selectedCredentialIds.value = new Set(
    qualifyingCredentialsForField.value
      .map((vc) => vc.id)
      .filter((id): id is string => typeof id === 'string'),
  )
}

function toggleCredential(id: string) {
  const s = new Set(selectedCredentialIds.value)
  if (s.has(id)) s.delete(id)
  else s.add(id)
  selectedCredentialIds.value = s
}

const bloomOrder = ['remember', 'understand', 'apply', 'analyze', 'evaluate', 'create']

function describeSkill(vc: VerifiableCredential): string {
  const claim = extractSkillClaim(vc.credentialSubject)
  if (!claim) return vc.id ?? '(no id)'
  const skill = allSkills.value.find((s) => s.id === claim.skillId)
  return skill?.name ?? claim.skillId
}

function claimLevel(vc: VerifiableCredential): string {
  const claim = extractSkillClaim(vc.credentialSubject)
  if (!claim) return ''
  return bloomOrder[claim.level] ?? String(claim.level)
}

function claimScore(vc: VerifiableCredential): number {
  const claim = extractSkillClaim(vc.credentialSubject)
  return claim?.score ?? 0
}

// -----------------------------------------------------------------------------
// Uploads
// -----------------------------------------------------------------------------

async function readFileAsBytes(file: File): Promise<number[]> {
  const buf = await file.arrayBuffer()
  return Array.from(new Uint8Array(buf))
}

async function onVideoChange(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  videoFile.value = file
  videoUploading.value = true
  videoProgress.value = t('opinions.new.readingMb', {
    mb: Math.round(file.size / 1024 / 1024),
  })
  try {
    const bytes = await readFileAsBytes(file)
    videoProgress.value = t('opinions.new.uploading')
    const result = await invoke<{ hash: string; size: number }>('content_add', {
      data: bytes,
    })
    videoHash.value = result.hash
    const probe = document.createElement('video')
    probe.preload = 'metadata'
    probe.src = URL.createObjectURL(file)
    await new Promise<void>((resolve) => {
      probe.onloadedmetadata = () => resolve()
      probe.onerror = () => resolve()
    })
    if (Number.isFinite(probe.duration) && probe.duration > 0) {
      videoDuration.value = Math.round(probe.duration)
    }
    URL.revokeObjectURL(probe.src)
    videoProgress.value = ''
  } catch (e) {
    error.value = t('opinions.new.videoUploadFailed', { error: String(e) })
    videoHash.value = null
  } finally {
    videoUploading.value = false
  }
}

async function onThumbChange(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  thumbUploading.value = true
  try {
    const bytes = await readFileAsBytes(file)
    const result = await invoke<{ hash: string }>('content_add', { data: bytes })
    thumbHash.value = result.hash
  } catch (e) {
    error.value = t('opinions.new.thumbnailUploadFailed', { error: String(e) })
  } finally {
    thumbUploading.value = false
  }
}

// -----------------------------------------------------------------------------
// Submit
// -----------------------------------------------------------------------------

async function submit() {
  if (!canSubmit.value || !videoHash.value) return

  submitting.value = true
  error.value = ''

  const req: PublishOpinionRequest = {
    subject_field_id: subjectFieldId.value,
    title: title.value.trim(),
    summary: summary.value.trim() || null,
    video_cid: videoHash.value,
    thumbnail_cid: thumbHash.value,
    duration_seconds: videoDuration.value,
    credential_proof_ids: Array.from(selectedCredentialIds.value),
  }

  try {
    const row = await invoke<OpinionRow>('publish_opinion', { req })
    router.push(`/opinions/${row.id}`)
  } catch (e) {
    error.value = t('opinions.new.publishFailed', { error: String(e) })
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="max-w-3xl">
    <div class="mb-8">
      <h1 class="text-3xl font-bold text-foreground">{{ $t('opinions.new.title') }}</h1>
      <p class="mt-2 text-muted-foreground">
        {{ $t('opinions.new.intro') }}
      </p>
    </div>

    <AppAlert
      v-if="eligibleFields.length === 0"
      type="warning"
      class="mb-6"
    >
      {{ $t('opinions.new.noEligibleWarning') }}
    </AppAlert>

    <div v-else class="space-y-6">
      <section class="rounded-xl border border-border bg-card p-6 space-y-4">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
          {{ $t('opinions.new.topicHeading') }}
        </h2>
        <select
          v-model="subjectFieldId"
          class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
          @change="onFieldChange"
        >
          <option v-for="f in eligibleFieldsInfo" :key="f.id" :value="f.id">
            {{ f.icon_emoji ? f.icon_emoji + ' ' : '' }}{{ f.name }}
          </option>
        </select>
      </section>

      <section class="rounded-xl border border-border bg-card p-6 space-y-4">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
          {{ $t('opinions.new.contentHeading') }}
        </h2>

        <AppInput
          v-model="title"
          :label="t('opinions.new.titleLabel')"
          :placeholder="t('opinions.new.titlePlaceholder')"
        />

        <div>
          <AppTextarea
            v-model="summary"
            :label="t('opinions.new.summaryLabel')"
            :placeholder="t('opinions.new.summaryPlaceholder')"
            :rows="2"
          />
          <p
            class="mt-1 text-xs"
            :class="summaryLength > SUMMARY_MAX ? 'text-red-500' : 'text-muted-foreground'"
          >
            {{ summaryLength }} / {{ SUMMARY_MAX }}
          </p>
        </div>

        <label class="block">
          <span class="mb-1 block text-sm font-medium text-foreground">{{ $t('opinions.new.videoLabel') }}</span>
          <input
            type="file"
            accept="video/*"
            class="block w-full text-sm text-muted-foreground file:me-4 file:rounded-md file:border-0 file:bg-primary/10 file:px-4 file:py-2 file:text-sm file:font-semibold file:text-primary hover:file:bg-primary/15 cursor-pointer"
            :disabled="videoUploading"
            @change="onVideoChange"
          />
        </label>
        <div v-if="videoUploading" class="text-sm text-muted-foreground">
          {{ videoProgress }}
        </div>
        <div v-else-if="videoHash" class="flex items-center gap-2 text-sm">
          <AppBadge variant="success">{{ $t('opinions.new.uploaded') }}</AppBadge>
          <code class="text-xs text-muted-foreground">{{ videoHash.slice(0, 24) }}…</code>
          <span v-if="videoDuration" class="text-xs text-muted-foreground">
            · {{ $t('opinions.new.minutes', { count: Math.round(videoDuration / 60) }) }}
          </span>
        </div>

        <label class="block pt-2">
          <span class="mb-1 block text-sm font-medium text-foreground">
            {{ $t('opinions.new.thumbnailLabel') }}
          </span>
          <input
            type="file"
            accept="image/*"
            class="block w-full text-sm text-muted-foreground file:me-4 file:rounded-md file:border-0 file:bg-muted file:px-3 file:py-1.5 file:text-sm file:text-foreground hover:file:bg-muted/80 cursor-pointer"
            :disabled="thumbUploading"
            @change="onThumbChange"
          />
          <span v-if="thumbHash" class="mt-1 block text-xs text-muted-foreground">
            {{ $t('opinions.new.uploadedPrefix') }} {{ thumbHash.slice(0, 16) }}…
          </span>
        </label>
      </section>

      <section class="rounded-xl border border-border bg-card p-6 space-y-4">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
          {{ $t('opinions.new.credentialsHeading') }}
        </h2>
        <p class="text-xs text-muted-foreground">
          {{ $t('opinions.new.credentialsHelp') }}
        </p>

        <div v-if="qualifyingCredentialsForField.length === 0" class="text-sm text-red-500">
          {{ $t('opinions.new.noQualifying') }}
        </div>
        <div v-else class="space-y-2">
          <label
            v-for="vc in qualifyingCredentialsForField"
            v-show="vc.id"
            :key="vc.id ?? vc.issuer + vc.validFrom"
            class="flex items-center gap-3 p-2 rounded-md hover:bg-muted/40 cursor-pointer"
          >
            <input
              type="checkbox"
              :checked="vc.id ? selectedCredentialIds.has(vc.id) : false"
              @change="vc.id && toggleCredential(vc.id)"
            />
            <div class="min-w-0 flex-1">
              <div class="text-sm font-medium text-foreground">
                {{ describeSkill(vc) }}
                <AppBadge variant="secondary" class="ms-2">{{ claimLevel(vc) }}</AppBadge>
                <AppBadge v-if="vc.witness" variant="success" class="ms-1">{{ $t('opinions.new.verified') }}</AppBadge>
              </div>
              <div class="text-xs text-muted-foreground">
                {{ $t('opinions.new.credentialMeta', { score: Math.round(claimScore(vc) * 100), date: vc.validFrom.slice(0, 10) }) }}
              </div>
            </div>
          </label>
        </div>
      </section>

      <AppAlert v-if="error" type="error">{{ error }}</AppAlert>

      <div class="flex gap-3">
        <AppButton :loading="submitting" :disabled="!canSubmit" @click="submit">
          {{ $t('opinions.new.publish') }}
        </AppButton>
        <AppButton variant="ghost" @click="router.back()">
          {{ $t('common.actions.cancel') }}
        </AppButton>
      </div>
    </div>
  </div>
</template>
