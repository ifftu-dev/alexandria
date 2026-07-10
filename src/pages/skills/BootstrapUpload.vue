<script setup lang="ts">
// Upload a resume / transcript / credential → confirm extracted skills →
// claim them as self-asserted, provenance-tagged credentials. Accredited
// documents carry higher confidence than a self-made resume.
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useSkillBootstrap, type DocType } from '@/composables/useSkillBootstrap'
import { AppButton, AppBadge, EmptyState } from '@/components/ui'
import type { SkillSuggestion } from '@/types'

const router = useRouter()
const { uploadEvidence, extract, confirm } = useSkillBootstrap()

const docType = ref<DocType>('resume')
const docTypes: { id: DocType; label: string; hint: string }[] = [
  { id: 'resume', label: 'Resume / CV', hint: 'Self-made — lower confidence' },
  { id: 'transcript', label: 'Academic transcript', hint: 'Accredited — higher confidence' },
  { id: 'accredited_credential', label: 'Institution credential', hint: 'Accredited — higher confidence' },
]

const text = ref('')
const fileName = ref('')
const contentHash = ref<string | undefined>(undefined)
const suggestions = ref<SkillSuggestion[]>([])
const chosen = ref<Set<string>>(new Set())
const busy = ref(false)
const error = ref('')
const done = ref<number | null>(null)

async function onFile(e: Event) {
  const file = (e.target as HTMLInputElement).files?.[0]
  if (!file) return
  fileName.value = file.name
  error.value = ''
  busy.value = true
  try {
    // Store the file as durable evidence (any type).
    contentHash.value = await uploadEvidence(file)
    // Best-effort client-side text read (works for text-like files); for
    // binary formats (PDF), the user pastes the text below.
    if (/\.(txt|md|csv|json)$/i.test(file.name) || file.type.startsWith('text/')) {
      text.value = await file.text()
    }
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
    if (!suggestions.value.length) error.value = 'No matching skills found — try adding more detail.'
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
    done.value = await confirm(ids, docType.value, contentHash.value)
  } catch (err) {
    error.value = String(err)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="mx-auto max-w-2xl space-y-6 py-6">
    <div>
      <h1 class="text-2xl font-bold text-foreground">Bootstrap your skills</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        Upload a resume, transcript, or credential. We'll suggest skills to
        claim — accredited documents count for more than a self-made resume.
        These are starting points; take an assessment to verify and raise them.
      </p>
    </div>

    <div v-if="done !== null">
      <EmptyState
        title="Skills added"
        :description="`Claimed ${done} skill${done === 1 ? '' : 's'}. Take an assessment to verify them and raise your confidence.`"
      />
      <div class="mt-4 flex gap-2">
        <AppButton @click="router.push('/skills')">View my skills</AppButton>
        <AppButton variant="outline" @click="router.push('/goals')">Set a goal</AppButton>
      </div>
    </div>

    <template v-else>
      <!-- Document type -->
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

      <!-- File + text -->
      <div class="space-y-3">
        <label class="flex cursor-pointer items-center gap-3 rounded-lg border border-dashed border-border p-4 text-sm">
          <input type="file" class="hidden" @change="onFile" />
          <span class="rounded-md bg-muted/40 px-3 py-1.5 text-foreground">Choose file</span>
          <span class="text-muted-foreground">{{ fileName || 'PDF, image, or text' }}</span>
        </label>
        <textarea
          v-model="text"
          rows="8"
          class="w-full rounded-lg border border-border bg-input px-3 py-2 text-sm text-foreground"
          placeholder="Paste the document text here (required for PDFs / images)…"
        />
        <AppButton variant="outline" :loading="busy" :disabled="!text.trim()" @click="findSkills">
          Find skills
        </AppButton>
      </div>

      <!-- Suggestions -->
      <div v-if="suggestions.length" class="space-y-2">
        <p class="text-sm text-muted-foreground">Confirm which skills to claim:</p>
        <label
          v-for="s in suggestions"
          :key="s.skill_id"
          class="flex items-center gap-3 rounded-lg border border-border p-2.5 text-sm"
        >
          <input type="checkbox" :checked="chosen.has(s.skill_id)" @change="toggle(s.skill_id)" />
          <span class="flex-1 text-foreground">{{ s.name }}</span>
          <AppBadge v-if="s.score >= 0.6" variant="success">strong</AppBadge>
          <span class="text-xs text-muted-foreground">“{{ s.matched }}”</span>
        </label>
        <AppButton :loading="busy" :disabled="!chosen.size" @click="claim">
          Claim {{ chosen.size }} skill{{ chosen.size === 1 ? '' : 's' }}
        </AppButton>
      </div>

      <p v-if="error" class="text-sm text-error">{{ error }}</p>
    </template>
  </div>
</template>
