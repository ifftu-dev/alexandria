<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { invoke } from '@tauri-apps/api/core'
import { AppButton, AppInput, AppTextarea, AppAlert, AppBadge } from '@/components/ui'
import type {
  SubjectFieldInfo,
  SkillProof,
  SkillInfo,
  OpinionRow,
  PublishOpinionRequest,
} from '@/types'

const router = useRouter()

const SUMMARY_MAX = 280
const QUALIFYING_LEVELS = new Set(['apply', 'analyze', 'evaluate', 'create'])

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

// Author's own skill proofs (we'll compute qualifying ones per-field)
const myProofs = ref<SkillProof[]>([])
const allSkills = ref<SkillInfo[]>([])
const eligibleFields = ref<string[]>([])
const subjectFields = ref<SubjectFieldInfo[]>([])

// Which proofs the author is staking on this opinion. Users pick at
// least one — but we auto-select all qualifying proofs under the
// chosen subject field so the happy path is a single click.
const selectedProofIds = ref<Set<string>>(new Set())

const submitting = ref(false)
const error = ref('')

// -----------------------------------------------------------------------------
// Derived state
// -----------------------------------------------------------------------------

const eligibleFieldsInfo = computed(() =>
  subjectFields.value.filter((f) => eligibleFields.value.includes(f.id)),
)

/**
 * Proofs that would qualify the author to post in the currently
 * selected subject field. These are the proofs where:
 *   - proficiency_level is apply / analyze / evaluate / create
 *   - the skill's subject is under the selected subject_field_id
 */
const qualifyingProofsForField = computed<SkillProof[]>(() => {
  if (!subjectFieldId.value) return []
  const skillById = new Map(allSkills.value.map((s) => [s.id, s]))
  return myProofs.value.filter((p) => {
    if (!QUALIFYING_LEVELS.has(p.proficiency_level)) return false
    const skill = skillById.get(p.skill_id)
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
    selectedProofIds.value.size > 0 &&
    !submitting.value &&
    !videoUploading.value &&
    !thumbUploading.value,
)

// -----------------------------------------------------------------------------
// Init
// -----------------------------------------------------------------------------

onMounted(async () => {
  try {
    const [fields, eligible, proofs, skills] = await Promise.all([
      invoke<SubjectFieldInfo[]>('list_subject_fields', {}),
      invoke<string[]>('list_eligible_subject_fields_for_posting'),
      invoke<SkillProof[]>('list_skill_proofs', {}),
      invoke<SkillInfo[]>('list_skills', {}),
    ])
    subjectFields.value = fields
    eligibleFields.value = eligible
    myProofs.value = proofs
    allSkills.value = skills

    // Default: pick the first eligible field if there is one
    const first = eligible[0]
    if (first !== undefined) {
      subjectFieldId.value = first
      autoSelectProofs()
    }
  } catch (e) {
    error.value = `Failed to load taxonomy: ${e}`
  }
})

function onFieldChange() {
  // Reset proof selection when field changes — the set of qualifying
  // proofs is per-field.
  selectedProofIds.value = new Set()
  autoSelectProofs()
}

function autoSelectProofs() {
  selectedProofIds.value = new Set(qualifyingProofsForField.value.map((p) => p.id))
}

function toggleProof(id: string) {
  const s = new Set(selectedProofIds.value)
  if (s.has(id)) s.delete(id)
  else s.add(id)
  selectedProofIds.value = s
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
  videoProgress.value = `Reading ${Math.round(file.size / 1024 / 1024)} MB…`
  try {
    const bytes = await readFileAsBytes(file)
    videoProgress.value = 'Adding to iroh…'
    const result = await invoke<{ hash: string; size: number }>('content_add', {
      data: bytes,
    })
    videoHash.value = result.hash
    // Best-effort duration probe
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
    error.value = `Video upload failed: ${e}`
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
    error.value = `Thumbnail upload failed: ${e}`
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
    credential_proof_ids: Array.from(selectedProofIds.value),
  }

  try {
    const row = await invoke<OpinionRow>('publish_opinion', { req })
    router.push(`/opinions/${row.id}`)
  } catch (e) {
    error.value = `Publish failed: ${e}`
  } finally {
    submitting.value = false
  }
}
</script>

<template>
  <div class="max-w-3xl">
    <div class="mb-8">
      <h1 class="text-3xl font-bold text-foreground">Post an Opinion</h1>
      <p class="mt-2 text-muted-foreground">
        Opinions are scoped to a subject field. To post in a field you must
        hold at least one skill proof (level ≥ <em>apply</em>) under that field.
        Your opinion will be chronological in that field's view — no global feed,
        no rankings.
      </p>
    </div>

    <!-- No eligible fields -->
    <AppAlert
      v-if="eligibleFields.length === 0"
      type="warning"
      class="mb-6"
    >
      You don't hold any qualifying skill proofs yet. Earn a proof at level
      <em>apply</em> or above in any skill, and you'll be eligible to post
      opinions in that skill's subject field.
    </AppAlert>

    <div v-else class="space-y-6">
      <!-- Subject field picker -->
      <section class="rounded-xl border border-border bg-card p-6 space-y-4">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
          Subject field
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

      <!-- Content -->
      <section class="rounded-xl border border-border bg-card p-6 space-y-4">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
          Content
        </h2>

        <AppInput
          v-model="title"
          label="Title"
          placeholder="e.g., Why functional-first is the wrong default for CS1"
        />

        <div>
          <AppTextarea
            v-model="summary"
            label="Summary (optional)"
            placeholder="A one-line framing for the list view."
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
          <span class="mb-1 block text-sm font-medium text-foreground">Video</span>
          <input
            type="file"
            accept="video/*"
            class="block w-full text-sm text-muted-foreground file:mr-4 file:rounded-md file:border-0 file:bg-primary/10 file:px-4 file:py-2 file:text-sm file:font-semibold file:text-primary hover:file:bg-primary/15 cursor-pointer"
            :disabled="videoUploading"
            @change="onVideoChange"
          />
        </label>
        <div v-if="videoUploading" class="text-sm text-muted-foreground">
          {{ videoProgress }}
        </div>
        <div v-else-if="videoHash" class="flex items-center gap-2 text-sm">
          <AppBadge variant="success">Uploaded</AppBadge>
          <code class="text-xs text-muted-foreground">{{ videoHash.slice(0, 24) }}…</code>
          <span v-if="videoDuration" class="text-xs text-muted-foreground">
            · {{ Math.round(videoDuration / 60) }} min
          </span>
        </div>

        <label class="block pt-2">
          <span class="mb-1 block text-sm font-medium text-foreground">
            Thumbnail (optional)
          </span>
          <input
            type="file"
            accept="image/*"
            class="block w-full text-sm text-muted-foreground file:mr-4 file:rounded-md file:border-0 file:bg-muted file:px-3 file:py-1.5 file:text-sm file:text-foreground hover:file:bg-muted/80 cursor-pointer"
            :disabled="thumbUploading"
            @change="onThumbChange"
          />
          <span v-if="thumbHash" class="mt-1 block text-xs text-muted-foreground">
            Uploaded: {{ thumbHash.slice(0, 16) }}…
          </span>
        </label>
      </section>

      <!-- Credential selection -->
      <section class="rounded-xl border border-border bg-card p-6 space-y-4">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
          Credentials
        </h2>
        <p class="text-xs text-muted-foreground">
          Which of your skill proofs you're staking on this opinion. At least
          one must qualify you to post in this subject field — we've
          pre-selected all of those. You can add or remove proofs below.
        </p>

        <div v-if="qualifyingProofsForField.length === 0" class="text-sm text-red-500">
          You don't have a qualifying proof for this field. Pick a different field.
        </div>
        <div v-else class="space-y-2">
          <label
            v-for="p in qualifyingProofsForField"
            :key="p.id"
            class="flex items-center gap-3 p-2 rounded-md hover:bg-muted/40 cursor-pointer"
          >
            <input
              type="checkbox"
              :checked="selectedProofIds.has(p.id)"
              @change="toggleProof(p.id)"
            />
            <div class="min-w-0 flex-1">
              <div class="text-sm font-medium text-foreground">
                {{ p.skill_id }}
                <AppBadge variant="secondary" class="ml-2">{{ p.proficiency_level }}</AppBadge>
              </div>
              <div class="text-xs text-muted-foreground">
                confidence {{ Math.round(p.confidence * 100) }}% ·
                {{ p.evidence_count }} evidence
              </div>
            </div>
          </label>
        </div>
      </section>

      <AppAlert v-if="error" type="error">{{ error }}</AppAlert>

      <div class="flex gap-3">
        <AppButton :loading="submitting" :disabled="!canSubmit" @click="submit">
          Publish Opinion
        </AppButton>
        <AppButton variant="ghost" @click="router.back()">
          Cancel
        </AppButton>
      </div>
    </div>
  </div>
</template>
