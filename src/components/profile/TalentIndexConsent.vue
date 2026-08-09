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
 */

import { computed, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'

import { AppButton } from '@/components/ui'

interface PreviewCandidate {
  skillId: string
  name: string
  level: number
  issuerClusters: number
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

const loading = ref(true)
const saving = ref(false)
const savedAt = ref(0)
const error = ref('')

const nothingSelected = computed(() => selected.value.size === 0)

function apply(preview: TalentIndexPreview) {
  candidates.value = preview.candidates
  record.value = preview.record
  selected.value = new Set(preview.consent.skills)
  includeDisplayName.value = preview.consent.displayName
  includeBio.value = preview.consent.bio
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

const recordJson = computed(() =>
  record.value === null ? '' : JSON.stringify(record.value, null, 2),
)

onMounted(load)
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
        separate thing that has nowhere to go yet.
      -->
      <p v-if="candidates.length > 0" class="mt-2 text-xs text-muted-foreground">
        {{ t('profile.talentIndex.notPublishedYet') }}
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
              {{ t('profile.talentIndex.level', { level: c.level }) }} ·
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
    </template>
  </section>
</template>
