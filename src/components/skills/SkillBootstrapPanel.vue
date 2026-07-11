<script setup lang="ts">
// Reusable resume/transcript → skills flow. Upload a document (stored as
// evidence) or paste its text, confirm the extracted skills, and claim them as
// provenance-tagged self-asserted credentials. Emits `claimed` with the count.
// Used both in onboarding and on the standalone /skills/bootstrap page.
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useSkillBootstrap, type DocType } from '@/composables/useSkillBootstrap'
import { AppButton, AppBadge } from '@/components/ui'
import type { SkillSuggestion } from '@/types'

const emit = defineEmits<{ (e: 'claimed', count: number): void }>()

const { t } = useI18n()
const { pickFile, readFile, extract, confirm } = useSkillBootstrap()

const docType = ref<DocType>('resume')
const docTypes = computed<{ id: DocType; label: string; hint: string }[]>(() => [
  { id: 'resume', label: t('skills.bootstrap.docResumeLabel'), hint: t('skills.bootstrap.docResumeHint') },
  { id: 'transcript', label: t('skills.bootstrap.docTranscriptLabel'), hint: t('skills.bootstrap.docTranscriptHint') },
  { id: 'accredited_credential', label: t('skills.bootstrap.docCredentialLabel'), hint: t('skills.bootstrap.docCredentialHint') },
])

const text = ref('')
const fileName = ref('')
const contentHash = ref<string | undefined>(undefined)
const suggestions = ref<SkillSuggestion[]>([])
const chosen = ref<Set<string>>(new Set())
const busy = ref(false)
const error = ref('')

async function chooseFile() {
  error.value = ''
  const path = await pickFile()
  if (!path) return
  busy.value = true
  try {
    const picked = await readFile(path)
    contentHash.value = picked.hash
    fileName.value = path.split('/').pop() ?? path
    if (picked.text.trim()) text.value = picked.text
  } catch (err) {
    error.value = String(err)
  } finally {
    busy.value = false
  }
}

async function findSkills() {
  busy.value = true
  error.value = ''
  suggestions.value = []
  try {
    suggestions.value = await extract(text.value)
    chosen.value = new Set(suggestions.value.filter((s) => s.score >= 0.6).map((s) => s.skill_id))
    if (!suggestions.value.length) error.value = t('skills.bootstrap.noSkillsFound')
  } catch (err) {
    error.value = String(err)
  } finally {
    busy.value = false
  }
}

function toggle(id: string) {
  const next = new Set(chosen.value)
  next.has(id) ? next.delete(id) : next.add(id)
  chosen.value = next
}

async function claim() {
  const ids = suggestions.value.filter((s) => chosen.value.has(s.skill_id)).map((s) => s.skill_id)
  if (!ids.length) return
  busy.value = true
  try {
    const n = await confirm(ids, docType.value, contentHash.value)
    suggestions.value = []
    text.value = ''
    fileName.value = ''
    emit('claimed', n)
  } catch (err) {
    error.value = String(err)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="space-y-4">
    <div class="grid gap-2 sm:grid-cols-3">
      <button
        v-for="d in docTypes"
        :key="d.id"
        class="rounded-xl border p-3 text-left transition-colors"
        :class="docType === d.id ? 'border-primary bg-primary/5' : 'border-border hover:border-primary/40'"
        @click="docType = d.id"
      >
        <div class="text-sm font-semibold text-foreground">{{ d.label }}</div>
        <div class="mt-0.5 text-xs text-muted-foreground">{{ d.hint }}</div>
      </button>
    </div>

    <div class="flex items-center gap-3 rounded-lg border border-dashed border-border p-4 text-sm">
      <AppButton variant="outline" :loading="busy" @click="chooseFile">{{ $t('skills.bootstrap.chooseFile') }}</AppButton>
      <span class="text-muted-foreground">{{ fileName || $t('skills.bootstrap.filePlaceholder') }}</span>
    </div>
    <textarea
      v-model="text"
      rows="6"
      class="w-full rounded-lg border border-border bg-input px-3 py-2 text-sm text-foreground"
      :placeholder="$t('skills.bootstrap.textPlaceholder')"
    />
    <AppButton variant="outline" :loading="busy" :disabled="!text.trim()" @click="findSkills">
      {{ $t('skills.bootstrap.findSkills') }}
    </AppButton>

    <div v-if="suggestions.length" class="space-y-2">
      <p class="text-sm text-muted-foreground">{{ $t('skills.bootstrap.confirmPrompt') }}</p>
      <label
        v-for="s in suggestions"
        :key="s.skill_id"
        class="flex items-center gap-3 rounded-lg border border-border p-2.5 text-sm"
      >
        <input type="checkbox" :checked="chosen.has(s.skill_id)" @change="toggle(s.skill_id)" />
        <span class="flex-1 text-foreground">{{ s.name }}</span>
        <AppBadge v-if="s.score >= 0.6" variant="success">{{ $t('skills.bootstrap.strong') }}</AppBadge>
        <span class="text-xs text-muted-foreground">“{{ s.matched }}”</span>
      </label>
      <AppButton :loading="busy" :disabled="!chosen.size" @click="claim">
        {{ $t('skills.bootstrap.claim', { count: chosen.size }, chosen.size) }}
      </AppButton>
    </div>

    <p v-if="error" class="text-sm text-error">{{ error }}</p>
  </div>
</template>
