<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { AppButton, AppBadge } from '@/components/ui'
import { useLocalApi } from '@/composables/useLocalApi'
import { useSentinel } from '@/composables/useSentinel'
import type {
  SentinelHoldoutRef,
  SentinelHoldoutKeyPolicy,
  SentinelHoldoutPlaintextShare,
  SentinelPriorBlob,
} from '@/types'

// Committee-member flow for running classifier evaluation against the
// DAO-private holdout set (Phase 6 + follow-up #3). Each member brings
// their unsealed share; once `threshold` shares are collected, the
// backend combines via Shamir, AES-decrypts the blob, and returns the
// parsed PriorBlob. We then run the local classifier against its
// labeled samples and show accuracy + FP fraction.

const router = useRouter()
const { invoke } = useLocalApi()
const { testBlobAgainstClassifier } = useSentinel()

const holdouts = ref<SentinelHoldoutRef[]>([])
const selectedId = ref<string>('')
const policy = ref<SentinelHoldoutKeyPolicy | null>(null)
const policyError = ref<string | null>(null)

// Share text inputs keyed by share_index from the policy
const shareInputs = ref<Record<number, string>>({})

const evaluating = ref(false)
const evalError = ref<string | null>(null)
const evalBlob = ref<SentinelPriorBlob | null>(null)
const evalVerdict = ref<{
  meanScore: number
  adversarialFraction: number
  sampleCount: number
} | null>(null)
const evalVerdictUntrained = ref(false)

const selectedHoldout = computed<SentinelHoldoutRef | null>(() =>
  holdouts.value.find(h => h.id === selectedId.value) ?? null,
)

const providedShareCount = computed<number>(() =>
  Object.values(shareInputs.value).filter(v => v.trim().length > 0).length,
)

const canEvaluate = computed<boolean>(() =>
  !evaluating.value
  && selectedHoldout.value !== null
  && policy.value !== null
  && providedShareCount.value >= (policy.value?.threshold ?? 99),
)

onMounted(async () => {
  try {
    holdouts.value = await invoke<SentinelHoldoutRef[]>('sentinel_holdout_list')
  } catch {
    holdouts.value = []
  }
})

async function onSelect(holdoutId: string) {
  selectedId.value = holdoutId
  policy.value = null
  policyError.value = null
  shareInputs.value = {}
  evalBlob.value = null
  evalVerdict.value = null
  evalVerdictUntrained.value = false
  evalError.value = null
  if (!holdoutId) return
  try {
    policy.value = await invoke<SentinelHoldoutKeyPolicy>(
      'sentinel_holdout_get_policy',
      { holdoutId },
    )
    for (const s of policy.value.shares) {
      shareInputs.value[s.share_index] = ''
    }
  } catch (e) {
    policyError.value = e instanceof Error ? e.message : String(e)
  }
}

async function runEvaluation() {
  if (!canEvaluate.value || !selectedHoldout.value || !policy.value) return
  evalError.value = null
  evaluating.value = true
  evalBlob.value = null
  evalVerdict.value = null
  evalVerdictUntrained.value = false
  try {
    const shares: SentinelHoldoutPlaintextShare[] = Object.entries(shareInputs.value)
      .map(([idx, y]) => ({ share_index: Number(idx), y_hex: y.trim() }))
      .filter(s => s.y_hex.length > 0)

    const blob = await invoke<SentinelPriorBlob>('sentinel_holdout_evaluate', {
      req: { holdout_id: selectedHoldout.value.id, shares },
    })
    evalBlob.value = blob

    const kind = blob.model_kind
    if (kind !== 'keystroke' && kind !== 'mouse') {
      evalError.value = `unsupported model_kind: ${kind}`
      return
    }
    const verdict = testBlobAgainstClassifier(kind, blob.samples)
    if (verdict === null) {
      evalVerdictUntrained.value = true
    } else {
      evalVerdict.value = verdict
    }
  } catch (e) {
    evalError.value = e instanceof Error ? e.message : String(e)
  } finally {
    evaluating.value = false
  }
}

const verdictTone = computed<'success' | 'warning' | 'error' | 'info'>(() => {
  const v = evalVerdict.value
  if (!v) return 'info'
  if (v.meanScore >= 0.65) return 'success'
  if (v.meanScore >= 0.35) return 'warning'
  return 'error'
})

const verdictSummary = computed<string | null>(() => {
  const v = evalVerdict.value
  if (!v || !evalBlob.value) return null
  const label = evalBlob.value.label
  const scorePct = Math.round(v.meanScore * 100)
  const advPct = Math.round(v.adversarialFraction * 100)
  const kind = evalBlob.value.model_kind
  // For adversarial-labeled blobs we want HIGH anomaly score (true-positive rate).
  // For "human"-labeled blobs we want LOW anomaly score (low false-positive rate).
  // The holdout curator picks what each set is measuring.
  if (label.toLowerCase().includes('human')) {
    return `Clean-human holdout: classifier mean anomaly ${scorePct}% (lower is better), `
      + `${advPct}% of samples cross the threshold — that's your false-positive rate.`
  }
  return `Adversarial holdout: classifier mean anomaly ${scorePct}% (higher is better), `
    + `${advPct}% of ${v.sampleCount} ${kind} samples were detected as attacks — that's `
    + `your true-positive rate for the '${label}' family.`
})
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
      <h1 class="text-xl font-bold text-foreground">Holdout classifier evaluation</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        Measures local classifier accuracy against a DAO-private holdout set. Requires the
        threshold number of committee-member shares to decrypt; each member runs
        <code class="font-mono">sentinel_holdout_unseal_share</code> on their own device and pastes
        the result below.
      </p>
    </div>

    <!-- Holdout picker -->
    <div class="card p-5 space-y-3">
      <h2 class="text-sm font-semibold text-foreground">1. Pick a holdout set</h2>
      <div v-if="holdouts.length === 0" class="rounded-md bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
        No holdout sets uploaded yet. A committee member uploads one via
        <code class="font-mono">sentinel_holdout_upload</code>.
      </div>
      <select
        v-else
        v-model="selectedId"
        class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        @change="onSelect(selectedId)"
      >
        <option value="">— Select —</option>
        <option v-for="h in holdouts" :key="h.id" :value="h.id">
          {{ h.model_kind }} · threshold {{ h.threshold }} · {{ h.created_at }}
        </option>
      </select>
      <div v-if="policyError" class="rounded-md bg-red-500/10 px-3 py-2 text-xs text-red-600 dark:text-red-400">
        {{ policyError }}
      </div>
    </div>

    <!-- Share inputs -->
    <div v-if="policy && selectedHoldout" class="card p-5 space-y-3">
      <div class="flex items-center justify-between">
        <h2 class="text-sm font-semibold text-foreground">
          2. Collect {{ policy.threshold }} shares
        </h2>
        <AppBadge :variant="providedShareCount >= policy.threshold ? 'success' : 'secondary'">
          {{ providedShareCount }} / {{ policy.threshold }}
        </AppBadge>
      </div>
      <p class="text-xs text-muted-foreground">
        Each committee member whose pubkey is listed below decrypts their share locally
        (via <code class="font-mono">sentinel_holdout_unseal_share</code>) and shares the
        resulting <code class="font-mono">y_hex</code> value over a secure channel. Paste any
        {{ policy.threshold }} of them here — the backend combines them via Shamir interpolation
        and decrypts the holdout blob.
      </p>
      <div class="space-y-3">
        <div
          v-for="share in policy.shares"
          :key="share.share_index"
          class="space-y-1"
        >
          <div class="flex items-center justify-between text-xs">
            <code class="truncate font-mono text-muted-foreground">
              #{{ share.share_index }} · {{ share.stake_address }}
            </code>
          </div>
          <input
            v-model="shareInputs[share.share_index]"
            type="text"
            placeholder="paste y_hex from this member (or leave blank)"
            class="w-full rounded-md border border-border bg-background px-3 py-1.5 font-mono text-xs"
          />
        </div>
      </div>
    </div>

    <!-- Evaluate -->
    <div v-if="policy" class="flex justify-end">
      <AppButton size="sm" :disabled="!canEvaluate" :loading="evaluating" @click="runEvaluation">
        Run evaluation
      </AppButton>
    </div>

    <!-- Result -->
    <div v-if="evalError" class="rounded-md bg-red-500/10 px-3 py-2 text-xs text-red-600 dark:text-red-400">
      {{ evalError }}
    </div>
    <div v-if="evalBlob" class="card p-5 space-y-3">
      <div class="flex items-center justify-between">
        <h2 class="text-sm font-semibold text-foreground">Evaluation result</h2>
        <AppBadge variant="success">Holdout decrypted</AppBadge>
      </div>
      <div class="grid grid-cols-2 gap-3 text-xs">
        <div>
          <div class="text-muted-foreground">Model kind</div>
          <div class="font-mono text-foreground">{{ evalBlob.model_kind }}</div>
        </div>
        <div>
          <div class="text-muted-foreground">Label</div>
          <div class="font-mono text-foreground">{{ evalBlob.label }}</div>
        </div>
        <div>
          <div class="text-muted-foreground">Samples</div>
          <div class="font-mono text-foreground">{{ evalBlob.samples.length }}</div>
        </div>
        <div>
          <div class="text-muted-foreground">Schema</div>
          <div class="font-mono text-foreground">v{{ evalBlob.schema_version }}</div>
        </div>
      </div>

      <div v-if="evalVerdictUntrained" class="rounded-md bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
        Local classifier not trained — can't compute a verdict. Train Sentinel first.
      </div>
      <div
        v-else-if="verdictSummary"
        class="rounded-md px-3 py-2 text-xs"
        :class="{
          'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400': verdictTone === 'success',
          'bg-amber-500/10 text-amber-700 dark:text-amber-400': verdictTone === 'warning',
          'bg-red-500/10 text-red-600 dark:text-red-400': verdictTone === 'error',
          'bg-muted/40 text-muted-foreground': verdictTone === 'info',
        }"
      >
        {{ verdictSummary }}
      </div>

      <p class="text-[11px] text-muted-foreground">
        The decrypted holdout stays on this device. Do not redistribute the labeled samples —
        leaking them gives attackers the evaluation criteria.
      </p>
    </div>
  </div>
</template>
