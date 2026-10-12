<script setup lang="ts">
/**
 * Talent-index consent and publish client.
 *
 * The index that receives a record is a service outside this repository, but
 * what leaves this device is the learner's decision, and the surface making
 * that decision has to be readable by the person it affects — see
 * `docs/enterprise-boundary.md`.
 *
 * Two design rules drive the layout:
 *
 * 1. **Nothing is pre-ticked.** Consent starts empty and stays empty until the
 *    learner acts. The peer-to-peer skill graph defaults earned skills to
 *    public, which is right for finding a tutor and wrong for an employer
 *    directory; the two are deliberately unrelated settings.
 * 2. **The preview is the record, not a description of it.** It renders the
 *    exact JSON the backend produced, so there is no gap between what the
 *    learner is shown and what would be sent. A summary written by hand could
 *    drift from the wire format; this cannot.
 *
 * Saving and publishing stay separate steps. The listing goes to the
 * network's Alexandria Cloud when the learner presses Publish now, or by
 * itself whenever it changes once they switch on keeping it up to date.
 */

import { computed, onMounted, ref } from 'vue'
import { useLocalApi } from "@/composables/useLocalApi"
import { useSettings } from '@/composables/useSettings'
import { useI18n } from 'vue-i18n'

import { AppButton, AppSwitch } from '@/components/ui'
import { bloomName } from '@/utils/bloom'

const { invoke } = useLocalApi()

interface PreviewCandidate {
  skillId: string
  name: string
  level: number
  issuerClusters: number
  /** Highest Bloom rank evidenced, 0 = remember … 5 = create. */
  bloomLevel: number
  consented: boolean
}

interface TalentIndexConsent {
  skills: string[]
  displayName: boolean
  bio: boolean
}

interface TalentIndexPreview {
  candidates: PreviewCandidate[]
  consent: TalentIndexConsent
  record: unknown | null
}

const { t } = useI18n()

const candidates = ref<PreviewCandidate[]>([])
const record = ref<unknown | null>(null)
const selected = ref<Set<string>>(new Set())
const includeDisplayName = ref(false)
const includeBio = ref(false)

const SYNC_KEY = 'talent_index.sync'

const loading = ref(true)
const saving = ref(false)
const savedAt = ref(0)
const error = ref('')
/** Skills in the stored consent: what Publish now would actually send. */
const savedSkills = ref(0)
const publishing = ref(false)
const published = ref(false)
const publishError = ref('')
const switching = ref(false)

const { byKey, initialize, setSetting } = useSettings()
const keepUpToDate = computed(() => {
  const value = byKey.value.get(SYNC_KEY)?.current_value
  return value === 'true' || value === '1'
})

const nothingSelected = computed(() => selected.value.size === 0)

function apply(preview: TalentIndexPreview) {
  candidates.value = preview.candidates
  record.value = preview.record
  selected.value = new Set(preview.consent.skills)
  includeDisplayName.value = preview.consent.displayName
  includeBio.value = preview.consent.bio
  savedSkills.value = preview.consent.skills.length
}

async function load() {
  loading.value = true
  try {
    apply(await invoke<TalentIndexPreview>('get_talent_index_preview'))
  } catch (e) {
    // A profile with no identity yet is the common case here, not a fault.
    // Leaving the list empty says the same thing without an alarming message.
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

function toggle(skillId: string) {
  const next = new Set(selected.value)
  if (next.has(skillId)) next.delete(skillId)
  else next.add(skillId)
  selected.value = next
  savedAt.value = 0
}

/**
 * Sends the whole consent record, not a delta.
 *
 * The preview that comes back is authoritative — it is rebuilt from what was
 * actually stored, so what the learner sees afterwards is the record their
 * choices produced rather than the one the UI predicted.
 */
async function save() {
  saving.value = true
  error.value = ''
  try {
    const consent: TalentIndexConsent = {
      skills: [...selected.value],
      displayName: includeDisplayName.value,
      bio: includeBio.value,
    }
    apply(await invoke<TalentIndexPreview>('set_talent_index_consent', { consent }))
    savedAt.value = Date.now()
  } catch (e) {
    error.value = String(e)
  } finally {
    saving.value = false
  }
}

/** Sends the stored listing — the one previewed above once saved. */
async function publish() {
  publishing.value = true
  published.value = false
  publishError.value = ''
  try {
    await invoke('publish_listing')
    published.value = true
  } catch (e) {
    publishError.value = String(e)
  } finally {
    publishing.value = false
  }
}

async function setKeepUpToDate(value: boolean) {
  switching.value = true
  publishError.value = ''
  try {
    await setSetting(SYNC_KEY, value ? 'true' : 'false')
  } catch (e) {
    publishError.value = String(e)
  } finally {
    switching.value = false
  }
}

const recordJson = computed(() =>
  record.value === null ? '' : JSON.stringify(record.value, null, 2),
)

onMounted(() => {
  void initialize().catch(() => {})
  void load()
})
</script>

<template>
  <section v-if="!loading" class="space-y-5 rounded-xl border border-border bg-card p-5">
    <header>
      <h2 class="text-base font-semibold text-foreground">{{ t('profile.talentIndex.title') }}</h2>
      <p class="mt-1 text-sm text-muted-foreground">
        {{ t('profile.talentIndex.description') }}
      </p>
      <!--
        Framing, not a result. Sitting under the Save button it read as a
        contradiction of "Saved." — the save persists consent; publication is a
        separate step, below.
      -->
      <p
        v-if="candidates.length > 0"
        data-testid="talent-index-framing"
        class="mt-3 rounded-lg border border-border bg-muted/40 px-3 py-2 text-sm text-foreground"
      >
        {{ t('organisations.listing.framing') }}
      </p>
    </header>

    <p v-if="candidates.length === 0" class="text-sm text-muted-foreground">
      {{ t('profile.talentIndex.noSkills') }}
    </p>

    <template v-else>
      <div class="space-y-2">
        <h3 class="text-sm font-medium text-foreground">
          {{ t('profile.talentIndex.skillsHeading') }}
        </h3>
        <label
          v-for="c in candidates"
          :key="c.skillId"
          class="flex cursor-pointer items-center gap-3 rounded-lg border border-border p-3"
        >
          <input
            type="checkbox"
            class="h-4 w-4 shrink-0"
            :checked="selected.has(c.skillId)"
            @change="toggle(c.skillId)"
          />
          <span class="min-w-0 flex-1">
            <span class="block truncate text-sm text-foreground">{{ c.name }}</span>
            <span class="block text-xs text-muted-foreground">
              {{ t('profile.talentIndex.level', { bloom: t(`common.bloom.${bloomName(c.bloomLevel)}`), level: c.level }) }} ·
              {{ t('profile.talentIndex.issuers', { count: c.issuerClusters }, c.issuerClusters) }}
            </span>
          </span>
        </label>
      </div>

      <div class="space-y-2">
        <h3 class="text-sm font-medium text-foreground">
          {{ t('profile.talentIndex.identityHeading') }}
        </h3>
        <label class="flex cursor-pointer items-center gap-3 text-sm text-foreground">
          <input v-model="includeDisplayName" type="checkbox" class="h-4 w-4" />
          {{ t('profile.talentIndex.includeDisplayName') }}
        </label>
        <label class="flex cursor-pointer items-center gap-3 text-sm text-foreground">
          <input v-model="includeBio" type="checkbox" class="h-4 w-4" />
          {{ t('profile.talentIndex.includeBio') }}
        </label>
        <p class="text-xs text-muted-foreground">{{ t('profile.talentIndex.identityNote') }}</p>
      </div>

      <div class="space-y-2">
        <h3 class="text-sm font-medium text-foreground">
          {{ t('profile.talentIndex.previewHeading') }}
        </h3>
        <p v-if="nothingSelected || !recordJson" class="text-sm text-muted-foreground">
          {{ t('profile.talentIndex.nothingSelected') }}
        </p>
        <pre
          v-else
          class="overflow-x-auto rounded-lg border border-border bg-background p-3 font-mono text-xs text-foreground"
          >{{ recordJson }}</pre
        >
      </div>

      <p class="text-xs text-muted-foreground">
        {{ t('profile.talentIndex.withdrawNote') }}
      </p>

      <div class="flex flex-wrap items-center gap-3 border-t border-border pt-4">
        <AppButton :disabled="saving" @click="save">
          {{ saving ? t('profile.talentIndex.saving') : t('profile.talentIndex.save') }}
        </AppButton>
        <span v-if="savedAt" class="text-sm text-muted-foreground">
          {{ t('profile.talentIndex.saved') }}
        </span>
        <span v-if="error" class="text-sm text-destructive">
          {{ t('profile.talentIndex.saveFailed', { error }) }}
        </span>
      </div>

      <div class="space-y-3 border-t border-border pt-4" data-testid="talent-index-publish">
        <h3 class="text-sm font-medium text-foreground">{{ t('organisations.listing.heading') }}</h3>
        <div class="flex flex-wrap items-center gap-3">
          <AppButton variant="outline" :disabled="publishing || savedSkills === 0" @click="publish">
            {{ publishing ? t('organisations.listing.publishing') : t('organisations.listing.publishNow') }}
          </AppButton>
          <span v-if="published" role="status" class="text-sm text-muted-foreground">
            {{ t('organisations.listing.published') }}
          </span>
        </div>
        <p v-if="savedSkills === 0" class="text-xs text-muted-foreground">
          {{ t('organisations.listing.nothingToPublish') }}
        </p>
        <div class="flex items-start gap-3">
          <AppSwitch
            :model-value="keepUpToDate"
            :disabled="switching"
            :label="t('organisations.listing.keepUpToDate')"
            @update:model-value="setKeepUpToDate"
          />
          <span class="min-w-0 flex-1">
            <span class="block text-sm text-foreground">{{ t('organisations.listing.keepUpToDate') }}</span>
            <span class="block text-xs text-muted-foreground">{{ t('organisations.listing.keepNote') }}</span>
          </span>
        </div>
        <p v-if="publishError" role="alert" class="text-sm text-destructive">
          {{ t('organisations.listing.publishFailed', { error: publishError }) }}
        </p>
      </div>
    </template>
  </section>
</template>
