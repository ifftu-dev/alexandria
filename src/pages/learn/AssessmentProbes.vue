<script setup lang="ts">
// Follow-up probes after an attempt is submitted, while monitoring is still
// on. The backend picks the probes and grades them; this shows each one,
// starts its time box by rendering it, and sends the served positions the
// learner chose plus the time to their first input. Skipping or running out
// of time never counts against the learner, and a probe step that fails is
// never allowed to block grading: the component always emits `done`.
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useAssessment } from '@/composables/useAssessment'
import { AppButton } from '@/components/ui'
import type { ServedProbe } from '@/types'

const props = defineProps<{ attemptId: string }>()
const emit = defineEmits<{ done: [] }>()

const { startProbes, renderProbe, answerProbe, finishProbes } = useAssessment()

const probes = ref<ServedProbe[]>([])
const index = ref(0)
const selected = ref<Set<number>>(new Set())
const remainingMs = ref<number | null>(null)
const busy = ref(false)
/** Probes are being prepared; a generated one waits on a model. */
const starting = ref(true)
let shownAt = 0
let deadline = 0
let firstInputMs: number | null = null
let timer: ReturnType<typeof setInterval> | null = null
let disposed = false
let finished = false

const current = computed(() => probes.value[index.value] ?? null)
const options = computed(() => {
  const raw = current.value?.content.options
  return Array.isArray(raw) ? raw.map(o => String(o)) : []
})
const prompt = computed(() => String(current.value?.content.prompt ?? ''))
const multiple = computed(() => current.value?.content.kind !== 'single')
const complement = computed(() => current.value?.content.probe === 'complement')
const secondsLeft = computed(() => Math.ceil((remainingMs.value ?? 0) / 1000))

function stopTimer() {
  if (timer !== null) clearInterval(timer)
  timer = null
}

async function finish() {
  if (finished) return
  finished = true
  stopTimer()
  if (probes.value.length > 0) {
    try {
      await finishProbes(props.attemptId)
    } catch (e) {
      console.warn('Closing the probe step failed', e)
    }
  }
  if (!disposed) emit('done')
}

async function show() {
  stopTimer()
  selected.value = new Set()
  firstInputMs = null
  remainingMs.value = null
  // This runner renders choice items only; a probe it cannot show is left
  // for the backend to record as not shown.
  while (current.value && current.value.item_kind !== 'mcq') index.value++
  const probe = current.value
  if (!probe || disposed) return finish()
  try {
    const rendered = await renderProbe(probe.id)
    if (disposed) return
    shownAt = Date.now()
    deadline = shownAt + (rendered.remaining_ms ?? rendered.time_box_ms)
    remainingMs.value = Math.max(0, deadline - shownAt)
    timer = setInterval(() => {
      remainingMs.value = Math.max(0, deadline - Date.now())
      if (remainingMs.value === 0) void submitCurrent(true)
    }, 250)
  } catch (e) {
    console.warn('Showing a probe failed', e)
    next()
  }
}

function next() {
  index.value++
  void show()
}

function toggle(pos: number) {
  if (busy.value) return
  if (firstInputMs === null) firstInputMs = Date.now() - shownAt
  if (!multiple.value) {
    selected.value = new Set([pos])
    return
  }
  const set = new Set(selected.value)
  set.has(pos) ? set.delete(pos) : set.add(pos)
  selected.value = set
}

async function submitCurrent(answer: boolean) {
  const probe = current.value
  if (!probe || busy.value) return
  busy.value = true
  stopTimer()
  try {
    if (answer) {
      await answerProbe(probe.id, { selected_positions: [...selected.value].sort((a, b) => a - b) }, firstInputMs)
    }
  } catch (e) {
    console.warn('Answering a probe failed', e)
  } finally {
    busy.value = false
    if (!disposed) next()
  }
}

onMounted(async () => {
  try {
    probes.value = await startProbes(props.attemptId)
  } catch (e) {
    console.warn('Starting probes failed', e)
    probes.value = []
  }
  starting.value = false
  if (disposed) return
  if (probes.value.length === 0) {
    finished = true
    emit('done')
    return
  }
  await show()
})

onUnmounted(() => {
  disposed = true
  stopTimer()
})
</script>

<template>
  <div v-if="current" class="space-y-4" data-testid="assessment-probe">
    <div class="flex items-baseline justify-between gap-3">
      <h2 class="text-lg font-semibold text-foreground">{{ $t('learn.assessment.probeTitle') }}</h2>
      <span class="text-xs text-muted-foreground">
        {{ $t('learn.assessment.probeProgress', { current: index + 1, total: probes.length }) }}
      </span>
    </div>
    <p class="text-sm text-muted-foreground">{{ $t('learn.assessment.probeIntro') }}</p>
    <div class="rounded-xl border border-border p-4">
      <p class="font-medium text-foreground">{{ prompt }}</p>
      <p class="mb-3 mt-1 text-xs text-muted-foreground">
        {{ complement
          ? $t('learn.assessment.probeComplement')
          : multiple ? $t('learn.assessment.chooseAll') : $t('learn.assessment.chooseOne') }}
      </p>
      <label
        v-for="(opt, pi) in options"
        :key="pi"
        class="mb-1.5 flex items-center gap-3 rounded-lg border border-border p-2.5 text-sm"
      >
        <input
          :type="multiple ? 'checkbox' : 'radio'"
          :name="multiple ? undefined : `probe-${current.id}`"
          :disabled="busy"
          :checked="selected.has(pi)"
          @change="toggle(pi)"
        />
        <span class="text-foreground">{{ opt }}</span>
      </label>
    </div>
    <div class="flex items-center justify-between gap-3">
      <span v-if="remainingMs !== null" class="font-mono text-xs text-muted-foreground" role="timer">
        {{ $t('learn.assessment.probeTimeLeft', { seconds: secondsLeft }) }}
      </span>
      <div class="ms-auto flex gap-2">
        <AppButton variant="outline" :disabled="busy" @click="submitCurrent(false)">{{ $t('learn.assessment.probeSkip') }}</AppButton>
        <AppButton :loading="busy" @click="submitCurrent(true)">{{ $t('learn.assessment.probeAnswer') }}</AppButton>
      </div>
    </div>
  </div>
  <p v-else-if="starting" class="text-sm text-muted-foreground" role="status" data-testid="probes-preparing">
    {{ $t('learn.assessment.probePreparing') }}
  </p>
</template>
