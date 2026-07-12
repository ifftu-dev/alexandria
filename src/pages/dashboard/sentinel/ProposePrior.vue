<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { AppButton } from '@/components/ui'
import { useLocalApi } from '@/composables/useLocalApi'
import { useSentinel } from '@/composables/useSentinel'
import type { IntegritySession } from '@/types'

// Propose an adversarial prior to the Sentinel DAO for ratification.
// Upload flow: pick model_kind → pick label → upload JSON blob → (optional)
// attach a source session → preview → submit. The blob is pinned locally
// via content_add, then a governance_proposal (category='sentinel_prior')
// is filed under the Sentinel DAO with content_cid = blob hash.

const router = useRouter()
const { t } = useI18n()
const { invoke } = useLocalApi()
const { testBlobAgainstClassifier } = useSentinel()

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

// Classifier self-check: does our own local model already flag this
// blob as anomalous? If yes → prior is genuinely adversarial and worth
// proposing. If no → ratifying it would teach the model to flag legit
// users. Null means the local classifier isn't trained yet (can't compare).
const classifierCheck = ref<{ meanScore: number; adversarialFraction: number; sampleCount: number } | null>(null)
const classifierCheckStatus = ref<'untested' | 'untrained' | 'ok'>('untested')

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
  file.arrayBuffer().then(async buf => {
    const bytes = new Uint8Array(buf)
    fileBytes.value = bytes
    try {
      const json = new TextDecoder().decode(bytes)
      const obj = JSON.parse(json) as Record<string, unknown>
      const sv = Number(obj.schema_version)
      const mk = String(obj.model_kind ?? '')
      const lb = String(obj.label ?? '')
      const samples = Array.isArray(obj.samples) ? obj.samples : null
      if (!samples) throw new Error(t('sentinel.propose.errors.samplesArray'))
      if (samples.length < 20) throw new Error(t('sentinel.propose.errors.tooFew', { count: samples.length }))
      if (mk === 'face') throw new Error(t('sentinel.propose.errors.faceForbidden'))
      if (mk !== 'keystroke' && mk !== 'mouse') throw new Error(t('sentinel.propose.errors.unsupportedKind', { kind: mk }))
      if (sv !== 1) throw new Error(t('sentinel.propose.errors.unsupportedSchema', { schema: sv }))
      if (!lb || lb.trim().length === 0) throw new Error(t('sentinel.propose.errors.emptyLabel'))
      parsedBlob.value = { schema_version: sv, model_kind: mk, label: lb, sampleCount: samples.length }

      // Run the local classifier against the blob to verify it's
      // actually adversarial. The proposer sees this before submitting;
      // DAO voters would see the same signal (future work).
      const kind = mk as ModelKind
      const check = await testBlobAgainstClassifier(kind, samples)
      if (check) {
        classifierCheck.value = check
        classifierCheckStatus.value = 'ok'
      } else {
        classifierCheck.value = null
        classifierCheckStatus.value = 'untrained'
      }
    } catch (e) {
      parseError.value = e instanceof Error ? e.message : String(e)
    }
  }).catch(e => {
    parseError.value = t('sentinel.propose.errors.readFailed', { error: String(e) })
  })
}

const classifierVerdict = computed<{ tone: 'success' | 'warning' | 'info'; message: string } | null>(() => {
  if (classifierCheckStatus.value === 'untested') return null
  if (classifierCheckStatus.value === 'untrained') {
    return {
      tone: 'info',
      message: t('sentinel.propose.verdict.untrained'),
    }
  }
  const c = classifierCheck.value!
  const pct = Math.round(c.meanScore * 100)
  const adv = Math.round(c.adversarialFraction * 100)
  if (c.meanScore >= 0.65 && c.adversarialFraction >= 0.5) {
    return {
      tone: 'success',
      message: t('sentinel.propose.verdict.strong', { pct, adv }),
    }
  }
  if (c.meanScore < 0.35) {
    return {
      tone: 'warning',
      message: t('sentinel.propose.verdict.weak', { pct }),
    }
  }
  return {
    tone: 'warning',
    message: t('sentinel.propose.verdict.borderline', { pct, adv }),
  }
})

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
        {{ $t('sentinel.propose.back') }}
      </button>
      <h1 class="text-xl font-bold text-foreground">{{ $t('sentinel.propose.title') }}</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        {{ $t('sentinel.propose.subtitle') }}
      </p>
    </div>

    <!-- Success state -->
    <div v-if="submittedProposalId" class="rounded-xl border border-emerald-500/40 bg-emerald-500/10 p-5">
      <h2 class="text-sm font-semibold text-emerald-700 dark:text-emerald-400">{{ $t('sentinel.propose.submittedTitle') }}</h2>
      <p class="mt-1 text-xs text-muted-foreground">
        <i18n-t keypath="sentinel.propose.submittedBody" tag="span">
          <template #id><code class="font-mono">{{ submittedProposalId }}</code></template>
        </i18n-t>
      </p>
      <div class="mt-4 flex gap-2">
        <AppButton size="sm" variant="secondary" @click="reset">{{ $t('sentinel.propose.another') }}</AppButton>
        <AppButton size="sm" @click="router.push('/dashboard/sentinel')">{{ $t('common.actions.done') }}</AppButton>
      </div>
    </div>

    <!-- Form -->
    <template v-else>
      <!-- Kind -->
      <div class="card p-5 space-y-3">
        <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.propose.step1') }}</h2>
        <p class="text-xs text-muted-foreground">
          {{ $t('sentinel.propose.step1Body') }}
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
        <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.propose.step2') }}</h2>
        <p class="text-xs text-muted-foreground">
          {{ $t('sentinel.propose.step2Body') }}
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
          :placeholder="$t('sentinel.propose.labelPlaceholder')"
          class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        />
      </div>

      <!-- Blob upload -->
      <div class="card p-5 space-y-3">
        <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.propose.step3') }}</h2>
        <p class="text-xs text-muted-foreground">
          <i18n-t keypath="sentinel.propose.step3Body" tag="span">
            <template #doc><code class="font-mono">docs/sentinel-adversarial-priors.md</code></template>
          </i18n-t>
        </p>
        <input
          type="file"
          accept="application/json,.json"
          class="block w-full text-sm text-muted-foreground file:me-3 file:rounded-md file:border-0 file:bg-primary/10 file:px-3 file:py-1.5 file:text-xs file:font-medium file:text-primary file:hover:bg-primary/20"
          @change="onFileChosen"
        />
        <div v-if="fileName" class="text-xs text-muted-foreground">
          <i18n-t keypath="sentinel.propose.selected" tag="span">
            <template #name><code class="font-mono">{{ fileName }}</code></template>
          </i18n-t>
        </div>
        <div v-if="parseError" class="rounded-md bg-red-500/10 px-3 py-2 text-xs text-red-600 dark:text-red-400">
          {{ parseError }}
        </div>
        <div
          v-else-if="parsedBlob"
          class="rounded-md bg-emerald-500/10 px-3 py-2 text-xs text-emerald-700 dark:text-emerald-400 space-y-0.5"
        >
          <div>
            <i18n-t keypath="sentinel.propose.parsedOk" tag="span">
              <template #count>{{ parsedBlob.sampleCount }}</template>
              <template #label><code class="font-mono">{{ parsedBlob.label }}</code></template>
            </i18n-t>
          </div>
          <div v-if="parsedBlob.model_kind !== modelKind" class="text-amber-600 dark:text-amber-400">
            {{ $t('sentinel.propose.kindMismatch', { fileKind: parsedBlob.model_kind, selectedKind: modelKind }) }}
          </div>
        </div>
        <!-- Classifier self-check verdict (follow-up #6) -->
        <div
          v-if="classifierVerdict"
          class="rounded-md px-3 py-2 text-xs"
          :class="{
            'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400': classifierVerdict.tone === 'success',
            'bg-amber-500/10 text-amber-700 dark:text-amber-400': classifierVerdict.tone === 'warning',
            'bg-muted/60 text-muted-foreground': classifierVerdict.tone === 'info',
          }"
        >
          <div class="font-medium">{{ $t('sentinel.propose.selfCheckTitle') }}</div>
          <div class="mt-0.5">{{ classifierVerdict.message }}</div>
        </div>
      </div>

      <!-- Source session (optional) -->
      <div class="card p-5 space-y-3">
        <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.propose.step4') }}</h2>
        <p class="text-xs text-muted-foreground">
          {{ $t('sentinel.propose.step4Body') }}
        </p>
        <select
          v-model="sourceSessionId"
          class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        >
          <option value="">{{ $t('sentinel.propose.sessionNone') }}</option>
          <option
            v-for="s in eligibleSessions"
            :key="s.id"
            :value="s.id"
          >
            {{ $t('sentinel.propose.statusOption', { date: s.started_at, status: s.status }) }}
          </option>
        </select>
      </div>

      <!-- Proposal metadata -->
      <div class="card p-5 space-y-3">
        <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.propose.step5') }}</h2>
        <input
          v-model="title"
          type="text"
          :placeholder="$t('sentinel.propose.titlePlaceholder')"
          class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        />
        <textarea
          v-model="description"
          rows="3"
          :placeholder="$t('sentinel.propose.descriptionPlaceholder')"
          class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        />
      </div>

      <!-- Submit -->
      <div v-if="submitError" class="rounded-md bg-red-500/10 px-3 py-2 text-xs text-red-600 dark:text-red-400">
        {{ submitError }}
      </div>
      <div class="flex justify-end gap-2">
        <AppButton variant="secondary" size="sm" @click="router.push('/dashboard/sentinel')">{{ $t('common.actions.cancel') }}</AppButton>
        <AppButton size="sm" :disabled="!canSubmit" :loading="submitting" @click="submit">
          {{ $t('sentinel.propose.submit') }}
        </AppButton>
      </div>
    </template>
  </div>
</template>
