<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRouter } from 'vue-router'
import { AppButton, AppBadge } from '@/components/ui'
import { useLocalApi } from '@/composables/useLocalApi'
import type {
  KeystrokeEvent,
  LoadedClassifierInfo,
  ScorePasteResponse,
} from '@/types'

/**
 * In-app diagnostic page for the Sentinel paste classifier.
 *
 * Drives the backend tract session with synthetic keystroke streams
 * for each attack archetype and reports the score + features. Use to
 * answer "would this detect me cheating?" without typing a single
 * keystroke.
 *
 * NOTE: scores depend on the loaded model (bundled or DAO-swapped).
 * Numbers here are diagnostic, not a guarantee of real-world detection.
 */

const router = useRouter()
const { invoke } = useLocalApi()

// Tiny LCG so streams are reproducible without pulling a full PRNG.
function rng(seed: number) {
  let s = (seed >>> 0) || 1
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0
    return s / 0xffffffff
  }
}

function gauss(r: () => number, mean: number, std: number): number {
  const u1 = Math.max(r(), 1e-9)
  const u2 = r()
  return mean + std * Math.sqrt(-2 * Math.log(u1)) * Math.cos(2 * Math.PI * u2)
}

type AttackLabel =
  | 'paste_macro'
  | 'typing_bot_constant'
  | 'typing_bot_jitter'
  | 'llm_paste_edit'
  | 'remote_control'
  | 'human_baseline'

const ALL_ATTACK_LABELS: AttackLabel[] = [
  'paste_macro',
  'typing_bot_constant',
  'typing_bot_jitter',
  'llm_paste_edit',
  'remote_control',
  'human_baseline',
]

function generateStream(label: AttackLabel, count: number, seed: number): KeystrokeEvent[] {
  const r = rng(seed)
  const out: KeystrokeEvent[] = []
  switch (label) {
    case 'paste_macro':
      for (let i = 0; i < count; i++) out.push({ key: 'char', dwellMs: r() * 3, flightMs: r() * 2 })
      return out
    case 'typing_bot_constant':
      for (let i = 0; i < count; i++) out.push({
        key: 'char',
        dwellMs: Math.max(0, 50 + gauss(r, 0, 3)),
        flightMs: Math.max(0, 80 + gauss(r, 0, 3)),
      })
      return out
    case 'typing_bot_jitter':
      for (let i = 0; i < count; i++) out.push({
        key: 'char',
        dwellMs: Math.max(1, gauss(r, 85, 25)),
        flightMs: Math.max(1, gauss(r, 110, 35)),
      })
      return out
    case 'llm_paste_edit': {
      const pasteN = Math.floor(count * 0.8)
      for (let i = 0; i < pasteN; i++) out.push({ key: 'char', dwellMs: r() * 3, flightMs: r() * 2 })
      for (let i = pasteN; i < count; i++) out.push({
        key: 'char',
        dwellMs: Math.max(20, Math.min(400, gauss(r, 120, 40))),
        flightMs: Math.max(50, Math.min(800, gauss(r, 250, 90))),
      })
      return out
    }
    case 'remote_control':
      for (let i = 0; i < count; i++) out.push({
        key: 'char',
        dwellMs: Math.max(1, gauss(r, 85, 20)),
        flightMs: Math.max(10, gauss(r, 180, 80)),
      })
      return out
    case 'human_baseline':
      for (let i = 0; i < count; i++) {
        const freqBucket = i % 4 === 0 ? 1.0 : 1.2
        out.push({
          key: 'char',
          dwellMs: Math.max(30, gauss(r, 85, 18) * freqBucket),
          flightMs: Math.max(40, gauss(r, 130, 35) * freqBucket),
        })
      }
      return out
  }
}

const PASTE_ANOMALY_THRESHOLD = 0.95
const PASTE_ANOMALY_CRITICAL_THRESHOLD = 0.99

interface Row {
  label: AttackLabel
  expectedDetection: 'attack' | 'human'
  features: number[]
  rawScore: number
  flag: 'critical' | 'anomaly' | 'clean' | 'unavailable'
  passOrFail: 'pass' | 'fail' | 'na'
}

const rows = ref<Row[]>([])
const running = ref(false)
const loadedInfo = ref<LoadedClassifierInfo>({ source: 'bundled', version: 'bundled-v1' })

const sampleSize = ref<number>(120)
const seed = ref<number>(42)

const stats = computed(() => {
  if (rows.value.length === 0) return null
  const attacks = rows.value.filter(r => r.expectedDetection === 'attack')
  const humans = rows.value.filter(r => r.expectedDetection === 'human')
  const tp = attacks.filter(r => r.flag === 'anomaly' || r.flag === 'critical').length
  const fp = humans.filter(r => r.flag === 'anomaly' || r.flag === 'critical').length
  return {
    tp,
    totalAttacks: attacks.length,
    fp,
    totalHumans: humans.length,
    tpr: attacks.length > 0 ? tp / attacks.length : 0,
    fpr: humans.length > 0 ? fp / humans.length : 0,
  }
})

function flagOf(score: number): Row['flag'] {
  if (score < 0) return 'unavailable'
  if (score >= PASTE_ANOMALY_CRITICAL_THRESHOLD) return 'critical'
  if (score >= PASTE_ANOMALY_THRESHOLD) return 'anomaly'
  return 'clean'
}

function expectedFor(label: AttackLabel): 'attack' | 'human' {
  return label === 'human_baseline' ? 'human' : 'attack'
}

function passOrFail(row: Omit<Row, 'passOrFail'>): Row['passOrFail'] {
  if (row.flag === 'unavailable') return 'na'
  const isAlerting = row.flag === 'anomaly' || row.flag === 'critical'
  if (row.expectedDetection === 'attack') return isAlerting ? 'pass' : 'fail'
  return isAlerting ? 'fail' : 'pass'
}

async function runAll() {
  running.value = true
  rows.value = []
  try {
    for (const label of ALL_ATTACK_LABELS) {
      const stream = generateStream(label, sampleSize.value, seed.value)
      const resp = await invoke<ScorePasteResponse>('sentinel_score_paste', {
        req: {
          events: stream,
          paste_event_count: label === 'paste_macro' || label === 'llm_paste_edit' ? 1 : 0,
          pasted_char_count:
            label === 'paste_macro' ? stream.length : label === 'llm_paste_edit' ? Math.floor(stream.length * 0.8) : 0,
          window_ms: 30_000,
        },
      })
      const partial = {
        label,
        expectedDetection: expectedFor(label),
        features: resp.features,
        rawScore: resp.score,
        flag: flagOf(resp.score),
      }
      rows.value.push({ ...partial, passOrFail: passOrFail(partial) })
      loadedInfo.value = resp.classifier
    }
  } finally {
    running.value = false
  }
}

const FEATURE_NAMES = [
  'mean dwell',
  'std dwell',
  'mean flight',
  'std flight',
  'near-zero-flight frac',
  'max zero-run /200',
  'char rate /50',
  'dwell CV',
  'flight CV',
  'paste events /10',
  'pasted chars /1000',
  'buffer len /200',
]
const FEATURE_DIM = FEATURE_NAMES.length
</script>

<template>
  <div class="mx-auto max-w-5xl p-6">
    <div class="mb-6 flex items-center justify-between">
      <div>
        <h1 class="text-xl font-semibold text-foreground">{{ $t('sentinel.cheatTest.title') }}</h1>
        <p class="mt-1 text-sm text-muted-foreground">
          {{ $t('sentinel.cheatTest.subtitle') }}
        </p>
      </div>
      <AppButton variant="secondary" size="sm" @click="router.push('/dashboard/sentinel')">
        {{ $t('common.actions.back') }}
      </AppButton>
    </div>

    <div class="card mb-4 p-5">
      <div class="flex flex-wrap items-end gap-4">
        <div>
          <label class="text-xs text-muted-foreground">{{ $t('sentinel.cheatTest.loadedModel') }}</label>
          <div class="mt-1 font-mono text-sm text-foreground">
            {{ loadedInfo.version }}
            <span class="text-muted-foreground">({{ loadedInfo.source }})</span>
          </div>
        </div>
        <div>
          <label class="text-xs text-muted-foreground">{{ $t('sentinel.cheatTest.streamLength') }}</label>
          <input
            v-model.number="sampleSize"
            type="number"
            min="20"
            max="200"
            class="mt-1 w-24 rounded border border-border bg-background px-2 py-1 text-sm"
          />
        </div>
        <div>
          <label class="text-xs text-muted-foreground">{{ $t('sentinel.cheatTest.seed') }}</label>
          <input
            v-model.number="seed"
            type="number"
            class="mt-1 w-24 rounded border border-border bg-background px-2 py-1 text-sm"
          />
        </div>
        <AppButton size="sm" :loading="running" :disabled="running" @click="runAll">
          {{ $t('sentinel.cheatTest.run') }}
        </AppButton>
      </div>
    </div>

    <div v-if="stats" class="card mb-4 p-5">
      <h2 class="text-sm font-semibold text-foreground">{{ $t('sentinel.cheatTest.summary') }}</h2>
      <div class="mt-3 grid grid-cols-2 gap-3 text-sm md:grid-cols-4">
        <div class="rounded bg-muted/40 p-3">
          <div class="text-xs text-muted-foreground">{{ $t('sentinel.cheatTest.caught') }}</div>
          <div class="text-foreground">{{ stats.tp }} / {{ stats.totalAttacks }}</div>
        </div>
        <div class="rounded bg-muted/40 p-3">
          <div class="text-xs text-muted-foreground">{{ $t('sentinel.cheatTest.falseAlarms') }}</div>
          <div class="text-foreground">{{ stats.fp }} / {{ stats.totalHumans }}</div>
        </div>
        <div class="rounded bg-muted/40 p-3">
          <div class="text-xs text-muted-foreground">{{ $t('sentinel.cheatTest.catchRate') }}</div>
          <div class="text-foreground">{{ stats.tpr.toFixed(2) }}</div>
        </div>
        <div class="rounded bg-muted/40 p-3">
          <div class="text-xs text-muted-foreground">{{ $t('sentinel.cheatTest.falseAlarmRate') }}</div>
          <div class="text-foreground">{{ stats.fpr.toFixed(2) }}</div>
        </div>
      </div>
    </div>

    <div v-if="rows.length > 0" class="card p-5">
      <div class="overflow-x-auto">
        <table class="w-full text-left text-sm">
          <thead>
            <tr class="border-b border-border text-xs text-muted-foreground">
              <th class="py-2 pr-3">{{ $t('sentinel.cheatTest.colPattern') }}</th>
              <th class="py-2 pr-3">{{ $t('sentinel.cheatTest.colExpected') }}</th>
              <th class="py-2 pr-3">{{ $t('sentinel.cheatTest.colScore') }}</th>
              <th class="py-2 pr-3">{{ $t('sentinel.cheatTest.colFlag') }}</th>
              <th class="py-2 pr-3">{{ $t('sentinel.cheatTest.colVerdict') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="r in rows" :key="r.label" class="border-b border-border/50">
              <td class="py-2 pr-3 font-mono">{{ r.label }}</td>
              <td class="py-2 pr-3">{{ r.expectedDetection }}</td>
              <td class="py-2 pr-3 font-mono">
                {{ r.rawScore < 0 ? '—' : r.rawScore.toFixed(3) }}
              </td>
              <td class="py-2 pr-3">
                <AppBadge
                  :variant="
                    r.flag === 'critical' ? 'error'
                    : r.flag === 'anomaly' ? 'warning'
                    : r.flag === 'unavailable' ? 'secondary'
                    : 'success'
                  "
                >
                  {{ r.flag }}
                </AppBadge>
              </td>
              <td class="py-2 pr-3">
                <AppBadge
                  :variant="
                    r.passOrFail === 'pass' ? 'success'
                    : r.passOrFail === 'fail' ? 'error'
                    : 'secondary'
                  "
                >
                  {{ r.passOrFail }}
                </AppBadge>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <details class="mt-6">
        <summary class="cursor-pointer text-xs text-muted-foreground">
          {{ $t('sentinel.cheatTest.featureDetails') }}
        </summary>
        <div class="mt-3 overflow-x-auto">
          <table class="w-full text-left text-xs">
            <thead>
              <tr class="border-b border-border text-muted-foreground">
                <th class="py-1 pr-2">{{ $t('sentinel.cheatTest.featureLabel') }}</th>
                <th v-for="(name, i) in FEATURE_NAMES" :key="i" class="py-1 pr-2">{{ name }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="r in rows" :key="r.label" class="border-b border-border/50 font-mono">
                <td class="py-1 pr-2">{{ r.label }}</td>
                <td v-for="i in FEATURE_DIM" :key="i" class="py-1 pr-2">
                  {{ r.features[i - 1]?.toFixed(2) ?? '—' }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </details>
    </div>

    <div v-if="rows.length === 0 && !running" class="card p-8 text-center text-sm text-muted-foreground">
      <i18n-t keypath="sentinel.cheatTest.emptyHint" tag="span">
        <template #action><strong>{{ $t('sentinel.cheatTest.emptyHintAction') }}</strong></template>
      </i18n-t>
    </div>
  </div>
</template>
