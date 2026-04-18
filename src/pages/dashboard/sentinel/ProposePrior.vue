<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { AppButton } from '@/components/ui'
import { useLocalApi } from '@/composables/useLocalApi'
import type { IntegritySession } from '@/types'

// Propose an adversarial prior to the Sentinel DAO for ratification.
// Upload flow: pick model_kind → pick label → upload JSON blob → (optional)
// attach a source session → preview → submit. The blob is pinned locally
// via content_add, then a governance_proposal (category='sentinel_prior')
// is filed under the Sentinel DAO with content_cid = blob hash.

const router = useRouter()
const { invoke } = useLocalApi()

type ModelKind = 'keystroke' | 'mouse'

// Known labels. Curators can extend via free-text; the backend doesn't
// constrain the label enum so future attack families don't need a
// coordinated client update.
const KNOWN_LABELS: Record<ModelKind, readonly string[]> = {
  keystroke: ['paste_macro', 'scripted_typing', 'remote_injection', 'remote_control'],
  mouse: ['bot_script', 'linear_interp', 'teleport', 'constant_velocity', 'sine_wave'],
}

const modelKind = ref<ModelKind>('keystroke')
const labelChoice = ref<string>('paste_macro')
const labelCustom = ref<string>('')
const notes = ref<string>('')
const title = ref<string>('')
const description = ref<string>('')

const fileName = ref<string>('')
const fileBytes = ref<Uint8Array | null>(null)
const parsedBlob = ref<{ schema_version: number; model_kind: string; label: string; sampleCount: number } | null>(null)
const parseError = ref<string | null>(null)

const sessions = ref<IntegritySession[]>([])
const sourceSessionId = ref<string>('')

const submitting = ref(false)
const submitError = ref<string | null>(null)
const submittedProposalId = ref<string | null>(null)

const effectiveLabel = computed<string>(() => {
  const c = labelCustom.value.trim()
  return c.length > 0 ? c : labelChoice.value
})

// Sessions eligible to source priors: only clean/completed (not flagged
// or suspended — decision 3, forfeiture). Backend re-checks, this is
// just so the dropdown doesn't tempt the user with futile picks.
const eligibleSessions = computed<IntegritySession[]>(() =>
  sessions.value.filter(s => s.status === 'completed' || s.status === 'active'),
)

const canSubmit = computed<boolean>(() =>
  !submitting.value
  && title.value.trim().length > 0
  && effectiveLabel.value.length > 0
  && parsedBlob.value !== null
  && parseError.value === null
  && parsedBlob.value.model_kind === modelKind.value,
)

onMounted(async () => {
  try {
    sessions.value = await invoke<IntegritySession[]>('integrity_list_sessions')
  } catch {
    sessions.value = []
  }
})

function onFileChosen(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  fileName.value = file.name
  parseError.value = null
  parsedBlob.value = null
  file.arrayBuffer().then(buf => {
    const bytes = new Uint8Array(buf)
    fileBytes.value = bytes
    try {
      const json = new TextDecoder().decode(bytes)
      const obj = JSON.parse(json) as Record<string, unknown>
      const sv = Number(obj.schema_version)
      const mk = String(obj.model_kind ?? '')
      const lb = String(obj.label ?? '')
      const samples = Array.isArray(obj.samples) ? obj.samples : null
      if (!samples) throw new Error('samples must be a JSON array')
      if (samples.length < 20) throw new Error(`need at least 20 samples (got ${samples.length})`)
      if (mk === 'face') throw new Error('face kind is forbidden for adversarial priors')
      if (mk !== 'keystroke' && mk !== 'mouse') throw new Error(`unsupported model_kind: ${mk}`)
      if (sv !== 1) throw new Error(`unsupported schema_version: ${sv}`)
      if (!lb || lb.trim().length === 0) throw new Error('label must be non-empty')
      parsedBlob.value = { schema_version: sv, model_kind: mk, label: lb, sampleCount: samples.length }
    } catch (e) {
      parseError.value = e instanceof Error ? e.message : String(e)
    }
  }).catch(e => {
    parseError.value = `file read failed: ${String(e)}`
  })
}

async function submit() {
  if (!canSubmit.value || !fileBytes.value) return
  submitError.value = null
  submitting.value = true
  try {
    // Pin the blob — returns { hash, size }
    const pinned = await invoke<{ hash: string; size: number }>('content_add', {
      data: Array.from(fileBytes.value),
    })
    const result = await invoke<{ proposal_id: string }>('sentinel_propose_prior', {
      req: {
        blob_cid: pinned.hash,
        title: title.value.trim(),
        description: description.value.trim() || null,
        source_session_id: sourceSessionId.value || null,
      },
    })
    submittedProposalId.value = result.proposal_id
  } catch (e) {
    submitError.value = e instanceof Error ? e.message : String(e)
  } finally {
    submitting.value = false
  }
}

function reset() {
  title.value = ''
  description.value = ''
  notes.value = ''
  labelCustom.value = ''
  fileName.value = ''
  fileBytes.value = null
  parsedBlob.value = null
  parseError.value = null
  sourceSessionId.value = ''
  submittedProposalId.value = null
  submitError.value = null
}
</script>

<template>
  <div class="mx-auto max-w-3xl px-4 py-6 space-y-6">
    <!-- Header -->
    <div>
      <button
        class="mb-2 flex items-center gap-1.5 text-xs text-muted-foreground transition-colors hover:text-foreground"
        @click="router.push('/dashboard/sentinel')"
      >
        <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
        </svg>
        Back to Sentinel
      </button>
      <h1 class="text-xl font-bold text-foreground">Propose an adversarial prior</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        Submit a labeled cheat pattern for Sentinel DAO ratification. Ratified priors train every client's local
        anti-cheat model as negative examples. Face-kind patterns are not accepted.
      </p>
    </div>

    <!-- Success state -->
    <div v-if="submittedProposalId" class="rounded-xl border border-emerald-500/40 bg-emerald-500/10 p-5">
      <h2 class="text-sm font-semibold text-emerald-700 dark:text-emerald-400">Proposal submitted</h2>
      <p class="mt-1 text-xs text-muted-foreground">
        Proposal ID: <code class="font-mono">{{ submittedProposalId }}</code>. Sentinel DAO members will vote on
        ratification; check the governance dashboard for progress.
      </p>
      <div class="mt-4 flex gap-2">
        <AppButton size="sm" variant="secondary" @click="reset">Propose another</AppButton>
        <AppButton size="sm" @click="router.push('/dashboard/sentinel')">Done</AppButton>
      </div>
    </div>

    <!-- Form -->
    <template v-else>
      <!-- Kind -->
      <div class="card p-5 space-y-3">
        <h2 class="text-sm font-semibold text-foreground">1. Model kind</h2>
        <p class="text-xs text-muted-foreground">
          Which local model should learn to flag this pattern? Face is never federated.
        </p>
        <div class="flex gap-2">
          <label
            v-for="kind in (['keystroke', 'mouse'] as const)"
            :key="kind"
            class="flex-1 cursor-pointer rounded-lg border px-4 py-3 text-sm transition-colors"
            :class="modelKind === kind
              ? 'border-primary bg-primary/5 text-primary font-medium'
              : 'border-border text-muted-foreground hover:border-primary/40'"
          >
            <input v-model="modelKind" type="radio" :value="kind" class="sr-only" />
            {{ kind }}
          </label>
        </div>
      </div>

      <!-- Label -->
      <div class="card p-5 space-y-3">
        <h2 class="text-sm font-semibold text-foreground">2. Label</h2>
        <p class="text-xs text-muted-foreground">
          What kind of attack is this? Pick a known label or type a custom one.
        </p>
        <select
          v-model="labelChoice"
          class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        >
          <option v-for="lbl in KNOWN_LABELS[modelKind]" :key="lbl" :value="lbl">{{ lbl }}</option>
        </select>
        <input
          v-model="labelCustom"
          type="text"
          placeholder="Or enter a custom label (overrides selection above)"
          class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        />
      </div>

      <!-- Blob upload -->
      <div class="card p-5 space-y-3">
        <h2 class="text-sm font-semibold text-foreground">3. Labeled-samples blob</h2>
        <p class="text-xs text-muted-foreground">
          JSON file with schema_version=1, model_kind matching the choice above, a non-empty label, and at least
          20 samples. See <code class="font-mono">docs/sentinel-adversarial-priors.md</code> for the schema.
        </p>
        <input
          type="file"
          accept="application/json,.json"
          class="block w-full text-sm text-muted-foreground file:mr-3 file:rounded-md file:border-0 file:bg-primary/10 file:px-3 file:py-1.5 file:text-xs file:font-medium file:text-primary file:hover:bg-primary/20"
          @change="onFileChosen"
        />
        <div v-if="fileName" class="text-xs text-muted-foreground">
          Selected: <code class="font-mono">{{ fileName }}</code>
        </div>
        <div v-if="parseError" class="rounded-md bg-red-500/10 px-3 py-2 text-xs text-red-600 dark:text-red-400">
          {{ parseError }}
        </div>
        <div
          v-else-if="parsedBlob"
          class="rounded-md bg-emerald-500/10 px-3 py-2 text-xs text-emerald-700 dark:text-emerald-400 space-y-0.5"
        >
          <div>Parsed OK — {{ parsedBlob.sampleCount }} samples, label <code class="font-mono">{{ parsedBlob.label }}</code>.</div>
          <div v-if="parsedBlob.model_kind !== modelKind" class="text-amber-600 dark:text-amber-400">
            Warning: blob model_kind is "{{ parsedBlob.model_kind }}" but you selected "{{ modelKind }}". Change one before submitting.
          </div>
        </div>
      </div>

      <!-- Source session (optional) -->
      <div class="card p-5 space-y-3">
        <h2 class="text-sm font-semibold text-foreground">4. Source session (optional)</h2>
        <p class="text-xs text-muted-foreground">
          If you extracted these samples from one of your assessment sessions, pick it here. Flagged or suspended
          sessions aren't eligible — attacker data must not shape the classifier. The backend re-checks regardless.
        </p>
        <select
          v-model="sourceSessionId"
          class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        >
          <option value="">None</option>
          <option
            v-for="s in eligibleSessions"
            :key="s.id"
            :value="s.id"
          >
            {{ s.started_at }} — status: {{ s.status }}
          </option>
        </select>
      </div>

      <!-- Proposal metadata -->
      <div class="card p-5 space-y-3">
        <h2 class="text-sm font-semibold text-foreground">5. Proposal summary</h2>
        <input
          v-model="title"
          type="text"
          placeholder="Short title shown to voters"
          class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        />
        <textarea
          v-model="description"
          rows="3"
          placeholder="Why should this be ratified? How was the data collected?"
          class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        />
      </div>

      <!-- Submit -->
      <div v-if="submitError" class="rounded-md bg-red-500/10 px-3 py-2 text-xs text-red-600 dark:text-red-400">
        {{ submitError }}
      </div>
      <div class="flex justify-end gap-2">
        <AppButton variant="secondary" size="sm" @click="router.push('/dashboard/sentinel')">Cancel</AppButton>
        <AppButton size="sm" :disabled="!canSubmit" :loading="submitting" @click="submit">
          Submit proposal
        </AppButton>
      </div>
    </template>
  </div>
</template>
