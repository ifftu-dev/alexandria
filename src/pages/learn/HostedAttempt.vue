<script setup lang="ts">
// A hosted attempt: an assessment request whose organisation holds the
// answers (required custody `issuer`) is taken on the organisation's hosted
// service. The service draws, sends one item at a time, times each on its
// own clock, grades and runs probes; this page only shows the item it was
// sent and sends the answer back (ADR 0009).
//
// Before the first item the candidate reads and accepts the terms the
// service states: answers and per-item timings go to the organisation, the
// device requirement, the retake rule and retention. Nothing is opened
// without that acceptance, and the acceptance names the digest of exactly
// these terms.
//
// Item text lives in this component's memory while the item is shown. It is
// never stored on the device or logged; the backend registers it with the
// assistant's bank guard.
//
// Monitoring runs as for any attempt, under the request's floor. A dropped
// connection retries and resumes at the same item, saying the clock kept
// running; an attempt the service abandoned after its window is explained
// with the retake rule. After close the organisation's credential is
// collected, or the reason it was not issued is shown in plain words.
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useSentinel } from '@/composables/useSentinel'
import { useLocalApi } from '@/composables/useLocalApi'
import { useCameraPresence } from '@/composables/useCameraPresence'
import { announceCredentialsChanged } from '@/composables/credentialEvents'
import { AppButton } from '@/components/ui'
import TrustProfileNotice from '@/components/integrity/TrustProfileNotice.vue'
import { needsVerifiedDevice, verifiedDeviceRefusal, verifiedPhones, type VerifiedPhones } from '@/utils/verifiedDevice'
import { hostedAttemptError, isDisconnected, isGone } from '@/utils/hostedAttempt'
import { errorMessage } from '@/utils/errorMessage'
import type {
  DeviceSupport,
  HostedAttemptState,
  HostedItem,
  HostedOutcome,
  HostedTermsReply,
  RequestAttemptTarget,
  SelectedProfile,
} from '@/types'
import type { DeepReadonly } from 'vue'

type Phase = 'loading' | 'consent' | 'item' | 'reconnecting' | 'closing' | 'result' | 'abandoned' | 'blocked' | 'error'

const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const { invoke } = useLocalApi()
const sentinel = useSentinel()

const requestId = String(route.params.requestId ?? '')
const directoryUrl = typeof route.query.directory === 'string' ? route.query.directory : ''

const phase = ref<Phase>('loading')
const error = ref('')
const target = ref<RequestAttemptTarget | null>(null)
const terms = ref<HostedTermsReply | null>(null)
const accepted = ref(false)
const attemptId = ref('')
const progress = ref<HostedAttemptState['attempt'] | null>(null)
/** The item on screen: in memory only, dropped as soon as it is answered. */
const item = ref<HostedItem | null>(null)
const selected = ref<Set<number>>(new Set())
const remainingMs = ref(0)
const sending = ref(false)
/** Shown after a reconnection: the item's clock kept running meanwhile. */
const resumedAfterDrop = ref(false)
const outcome = ref<HostedOutcome | null>(null)
const checkingAgain = ref(false)
const floorBlocked = ref<DeepReadonly<SelectedProfile> | null>(null)
const deviceSupport = ref<DeviceSupport | null>(null)
const deviceRefused = ref<VerifiedPhones | null>(null)

let shownAt = 0
let clock: ReturnType<typeof setInterval> | null = null
let disposed = false
/** Monitoring this page started and has not stopped. */
const monitoring = ref(false)
/** The ended session to close with, kept for a retry after a dropped connection. */
const closeSession = ref<string | null>(null)

const cameraVideoRef = ref<HTMLVideoElement | null>(null)
const {
  stream: cameraStream,
  starting: cameraStarting,
  error: cameraError,
  enable: enableCamera,
  bind: bindCamera,
  dispose: disposeCamera,
} = useCameraPresence(sentinel, cameraVideoRef)

const cameraRequired = computed(() => !!target.value?.camera_required)
const verifiedDeviceRequired = computed(() =>
  terms.value?.terms.device_requirement === 'device_attested'
  || needsVerifiedDevice(target.value?.issuance_policy?.required_assurance_level),
)
const phones = computed<VerifiedPhones>(() => deviceRefused.value ?? verifiedPhones(deviceSupport.value))
const verifiedDeviceBlocked = computed(() =>
  verifiedDeviceRequired.value && (deviceRefused.value !== null || deviceSupport.value?.verified_device === false),
)
const resuming = computed(() => !!terms.value?.open_attempt)
const canStart = computed(() =>
  !!terms.value
  && (resuming.value || (terms.value.can_open && accepted.value))
  && !(cameraRequired.value && !cameraStream.value)
  && !verifiedDeviceBlocked.value,
)
const retakeRule = computed(() => {
  const r = terms.value?.terms.retakes
  if (!r) return ''
  return r.limit === 0 ? t('learn.hosted.retakeNone') : t('learn.hosted.retakeRule', { count: r.limit, days: r.wait_days })
})
const attemptsLeft = computed(() => {
  const reply = terms.value
  if (!reply) return 0
  // The attempt just finished counts once it closed.
  return Math.max(0, reply.attempts_allowed - reply.attempts_used - 1)
})
const remainingLabel = computed(() => {
  const total = Math.ceil(remainingMs.value / 1000)
  const minutes = Math.floor(total / 60)
  const seconds = String(total % 60).padStart(2, '0')
  return `${minutes}:${seconds}`
})

function failWith(e: unknown) {
  if (disposed) return
  error.value = hostedAttemptError(e)?.message ?? errorMessage(e)
  phase.value = 'error'
}

function stopClock() {
  if (clock) clearInterval(clock)
  clock = null
}

async function stopMonitoring(): Promise<void> {
  if (!monitoring.value) return
  monitoring.value = false
  disposeCamera()
  await sentinel.stop()
}

/** Show the state the service sent: the next item, or close when none is left. */
function show(state: HostedAttemptState) {
  attemptId.value = state.attempt.id
  progress.value = state.attempt
  stopClock()
  item.value = state.item
  selected.value = new Set()
  if (!state.item) {
    if (state.attempt.ready_to_close) void close()
    return
  }
  phase.value = 'item'
  shownAt = Date.now()
  remainingMs.value = state.item.remaining_ms
  const started = Date.now()
  const startRemaining = state.item.remaining_ms
  clock = setInterval(() => {
    remainingMs.value = Math.max(0, startRemaining - (Date.now() - started))
  }, 250)
  sentinel.responses.present(`hosted:${state.attempt.id}:${state.item.seq}`, {
    chars: state.item.question.length + state.item.options.reduce((sum, o) => sum + o.length, 0),
    difficulty: null,
    input: 'choice',
  })
}

async function detectDeviceSupport(): Promise<void> {
  try {
    deviceSupport.value = await invoke<DeviceSupport>('integrity_device_support')
  } catch {
    /* unknown: nothing is claimed; the backend still refuses */
  }
}

onMounted(async () => {
  try {
    // The request is verified against the organisation's domain and stored
    // first, so monitoring takes its floor from what this device verified.
    target.value = await invoke<RequestAttemptTarget>('exchange_prepare_assessment', { directoryUrl, requestId })
    terms.value = await invoke<HostedTermsReply>('hosted_attempt_terms', { directoryUrl, requestId })
    if (disposed) return
    if (verifiedDeviceRequired.value) await detectDeviceSupport()
    phase.value = 'consent'
  } catch (e) {
    failWith(e)
  }
})

onUnmounted(() => {
  disposed = true
  stopClock()
  item.value = null
  void stopMonitoring()
})

async function start() {
  if (!terms.value || !canStart.value) return
  sending.value = true
  error.value = ''
  try {
    await sentinel.start(null, cameraStream.value !== null, 'assessment', { exchangeRequestId: requestId })
    monitoring.value = true
    if (disposed) return
    if (!sentinel.getSessionId() || !sentinel.isActive.value) throw new Error(t('learn.hosted.monitoringOff'))
    const selection = sentinel.trustProfile.value
    if (selection && !selection.policy_floor.satisfied) {
      floorBlocked.value = selection
      phase.value = 'blocked'
      await stopMonitoring()
      return
    }
    bindCamera()
    const state = terms.value.open_attempt
      ? await invoke<HostedAttemptState>('hosted_attempt_resume', { directoryUrl, attemptId: terms.value.open_attempt })
      : await invoke<HostedAttemptState>('hosted_attempt_open', {
        directoryUrl,
        requestId,
        consent: { accepted: true, terms_digest: terms.value.terms_digest },
      })
    if (disposed) return
    show(state)
  } catch (e) {
    await stopMonitoring()
    const refused = verifiedDeviceRefusal(e)
    if (refused) {
      deviceRefused.value = refused
      phase.value = 'consent'
    } else if (hostedAttemptError(e)?.code === 'consent_required') {
      // The terms changed since they were read: read them again.
      accepted.value = false
      terms.value = await invoke<HostedTermsReply>('hosted_attempt_terms', { directoryUrl, requestId }).catch(() => terms.value)
      error.value = t('learn.hosted.termsChanged')
      phase.value = 'consent'
    } else if (isGone(e)) {
      phase.value = 'abandoned'
    } else {
      failWith(e)
    }
  } finally {
    sending.value = false
  }
}

function toggle(position: number) {
  const current = item.value
  if (!current || sending.value) return
  const previous = selected.value
  const changesAnswer = current.select === 'many' ? previous.has(position) : previous.size > 0 && !previous.has(position)
  sentinel.responses.input(`hosted:${attemptId.value}:${current.seq}`, { changesAnswer })
  if (current.select === 'one') {
    selected.value = new Set([position])
    return
  }
  const next = new Set(previous)
  next.has(position) ? next.delete(position) : next.add(position)
  selected.value = next
}

const RETRY_MS = 3000

/** Resume after a dropped connection; resend the answer if it never arrived. */
async function reconnect(pending: { seq: number; selected: number[]; shownMs: number }) {
  phase.value = 'reconnecting'
  for (;;) {
    if (disposed) return
    await new Promise(resolve => setTimeout(resolve, RETRY_MS))
    try {
      let state = await invoke<HostedAttemptState>('hosted_attempt_resume', { directoryUrl, attemptId: attemptId.value })
      if (state.item?.seq === pending.seq) {
        // The answer did not arrive: send it again (a repeat is harmless).
        state = await invoke<HostedAttemptState>('hosted_attempt_answer', {
          directoryUrl,
          attemptId: attemptId.value,
          seq: pending.seq,
          selected: pending.selected,
          deviceTimings: { shown_ms: pending.shownMs },
        })
      }
      if (disposed) return
      resumedAfterDrop.value = true
      show(state)
      return
    } catch (e) {
      if (isDisconnected(e)) continue
      if (isGone(e)) {
        stopClock()
        item.value = null
        await stopMonitoring()
        phase.value = 'abandoned'
        return
      }
      failWith(e)
      return
    }
  }
}

async function answer() {
  const current = item.value
  if (!current || sending.value) return
  const pending = {
    seq: current.seq,
    selected: [...selected.value].sort((a, b) => a - b),
    shownMs: Date.now() - shownAt,
  }
  sending.value = true
  resumedAfterDrop.value = false
  try {
    const state = await invoke<HostedAttemptState>('hosted_attempt_answer', {
      directoryUrl,
      attemptId: attemptId.value,
      seq: pending.seq,
      selected: pending.selected,
      deviceTimings: { shown_ms: pending.shownMs },
    })
    if (disposed) return
    show(state)
  } catch (e) {
    if (isDisconnected(e)) {
      await reconnect(pending)
    } else if (isGone(e)) {
      stopClock()
      item.value = null
      await stopMonitoring()
      phase.value = 'abandoned'
    } else {
      failWith(e)
    }
  } finally {
    sending.value = false
  }
}

async function close() {
  phase.value = 'closing'
  error.value = ''
  try {
    closeSession.value = sentinel.getSessionId() ?? closeSession.value
    await stopMonitoring()
    if (!closeSession.value) throw new Error(t('learn.hosted.monitoringOff'))
    const closed = await invoke<HostedOutcome>('hosted_attempt_close', {
      directoryUrl,
      attemptId: attemptId.value,
      integritySessionId: closeSession.value,
    })
    if (disposed) return
    settle(closed)
  } catch (e) {
    if (isDisconnected(e)) {
      error.value = t('learn.hosted.closeRetry')
      return
    }
    failWith(e)
  }
}

async function retryClose() {
  error.value = ''
  try {
    // Already closed on the service's side: read what became of it.
    const closed = await invoke<HostedOutcome>('hosted_attempt_result', { directoryUrl, attemptId: attemptId.value })
      .catch(async (e: unknown) => {
        if (!isGone(e) || !closeSession.value) throw e
        return invoke<HostedOutcome>('hosted_attempt_close', {
          directoryUrl,
          attemptId: attemptId.value,
          integritySessionId: closeSession.value,
        })
      })
    if (!disposed) settle(closed)
  } catch (e) {
    error.value = isDisconnected(e) ? t('learn.hosted.closeRetry') : (hostedAttemptError(e)?.message ?? errorMessage(e))
  }
}

function settle(closed: HostedOutcome) {
  outcome.value = closed
  phase.value = 'result'
  if (closed.issuance.state === 'issued') announceCredentialsChanged()
}

async function checkAgain() {
  checkingAgain.value = true
  try {
    settle(await invoke<HostedOutcome>('hosted_attempt_result', { directoryUrl, attemptId: attemptId.value }))
  } catch (e) {
    error.value = hostedAttemptError(e)?.message ?? errorMessage(e)
  } finally {
    checkingAgain.value = false
  }
}
</script>

<template>
  <div class="mx-auto max-w-2xl space-y-5 py-6" data-testid="hosted-attempt">
    <div v-if="monitoring && sentinel.isActive.value" class="flex items-center gap-3 rounded-lg border border-border bg-card p-3 text-sm">
      <span class="h-2.5 w-2.5 rounded-full" :class="sentinel.suspended.value ? 'bg-warning' : 'bg-success'" />
      <span class="flex-1 font-medium text-foreground">{{ $t('learn.assessment.integrityOn') }}</span>
    </div>

    <div v-if="phase === 'loading'" class="py-10 text-center text-sm text-muted-foreground">{{ $t('learn.assessment.preparing') }}</div>

    <div v-else-if="phase === 'error'" role="alert" class="space-y-3 rounded-lg border border-error/40 bg-error/5 p-4 text-sm text-error">
      <p>{{ error }}</p>
      <AppButton variant="outline" @click="router.back()">{{ $t('learn.hosted.back') }}</AppButton>
    </div>

    <!-- Terms and consent, before the first item -->
    <div v-else-if="phase === 'consent' && terms" class="space-y-4" data-testid="hosted-consent">
      <h1 class="text-xl font-bold text-foreground">{{ $t('learn.hosted.title', { role: terms.terms.role, org: terms.terms.organisation }) }}</h1>
      <p class="text-sm text-muted-foreground">
        {{ $t('learn.hosted.intro', { org: terms.terms.organisation, items: terms.terms.items }) }}
      </p>
      <ul class="list-disc space-y-1.5 rounded-lg border border-border bg-card p-3 ps-8 text-sm text-foreground" data-testid="hosted-statements">
        <li v-for="(statement, i) in terms.terms.statements" :key="i">{{ statement }}</li>
      </ul>
      <p class="text-xs text-muted-foreground">
        {{ $t('learn.hosted.attempts', { used: terms.attempts_used, allowed: terms.attempts_allowed }) }}
      </p>

      <p
        v-if="verifiedDeviceRequired"
        role="status"
        class="rounded-lg border border-border bg-card p-3 text-sm"
        :class="verifiedDeviceBlocked ? 'font-medium text-warning' : 'text-muted-foreground'"
        data-testid="hosted-device-gate"
      >
        {{ $t(`learn.assessment.verifiedDeviceNote.${phones}`) }}
      </p>

      <div v-if="cameraRequired" class="space-y-2 rounded-lg border border-border bg-card p-3 text-sm" data-testid="hosted-camera-gate">
        <p class="text-muted-foreground">{{ $t('learn.assessment.requestCameraNote') }}</p>
        <p v-if="cameraStream" class="font-medium text-success">{{ $t('learn.assessment.cameraReady') }}</p>
        <AppButton v-else variant="outline" size="sm" :loading="cameraStarting" @click="enableCamera">
          {{ $t('learn.assessment.cameraEnable') }}
        </AppButton>
        <p v-if="cameraError" class="text-error">{{ cameraError }}</p>
      </div>

      <template v-if="resuming">
        <p class="text-sm text-foreground">{{ $t('learn.hosted.resumeNote') }}</p>
      </template>
      <template v-else-if="terms.can_open">
        <label class="flex items-start gap-3 rounded-lg border border-border bg-card p-3 text-sm" data-testid="hosted-accept">
          <input v-model="accepted" type="checkbox" class="mt-0.5" />
          <span class="text-foreground">{{ $t('learn.hosted.accept', { org: terms.terms.organisation }) }}</span>
        </label>
      </template>
      <p v-else-if="terms.next_attempt_at" class="text-sm font-medium text-warning" data-testid="hosted-retake-wait">
        {{ $t('learn.hosted.retakeWait', { when: new Date(terms.next_attempt_at).toLocaleString() }) }}
      </p>
      <p v-else class="text-sm font-medium text-warning" data-testid="hosted-retake-limit">{{ $t('learn.hosted.retakeLimit') }}</p>

      <p v-if="error" class="text-sm text-error">{{ error }}</p>
      <div class="flex gap-2">
        <AppButton v-if="resuming || terms.can_open" :disabled="!canStart" :loading="sending" data-testid="hosted-start" @click="start">
          {{ resuming ? $t('learn.hosted.resume') : $t('learn.hosted.start') }}
        </AppButton>
        <AppButton variant="outline" @click="router.back()">{{ $t('learn.hosted.decline') }}</AppButton>
      </div>
    </div>

    <div v-else-if="phase === 'blocked' && floorBlocked" class="space-y-3">
      <TrustProfileNotice :selected="floorBlocked" />
      <AppButton variant="outline" @click="router.go(0)">{{ $t('sentinel.trust.notice.tryAgain') }}</AppButton>
    </div>

    <!-- One item at a time -->
    <div v-else-if="(phase === 'item' || phase === 'reconnecting') && item && progress" class="space-y-4" data-testid="hosted-item">
      <div class="flex items-center justify-between text-xs text-muted-foreground">
        <span>{{ item.probe ? $t('learn.hosted.followUp') : $t('learn.hosted.progress', { n: progress.answered + 1, total: progress.items }) }}</span>
        <span class="font-mono" :class="remainingMs < 10000 ? 'text-warning' : ''" data-testid="hosted-clock">{{ remainingLabel }}</span>
      </div>
      <p v-if="item.probe" class="rounded-lg border border-border bg-card p-3 text-sm text-muted-foreground">
        {{ $t('learn.hosted.followUpNote', { seconds: Math.round(item.time_limit_ms / 1000) }) }}
      </p>
      <p v-if="resumedAfterDrop" role="status" class="rounded-lg border border-border bg-card p-3 text-sm text-muted-foreground" data-testid="hosted-resumed">
        {{ $t('learn.hosted.resumed') }}
      </p>
      <p v-if="phase === 'reconnecting'" role="status" class="rounded-lg border border-warning/40 bg-warning/5 p-3 text-sm text-warning" data-testid="hosted-reconnecting">
        {{ $t('learn.hosted.reconnecting', { minutes: Math.round((terms?.terms.disconnect_grace_secs ?? 600) / 60) }) }}
      </p>
      <p class="whitespace-pre-wrap text-base text-foreground">{{ item.question }}</p>
      <p class="text-xs text-muted-foreground">{{ item.select === 'many' ? $t('learn.hosted.selectMany') : $t('learn.hosted.selectOne') }}</p>
      <div class="space-y-2">
        <label
          v-for="(option, position) in item.options"
          :key="position"
          class="flex items-start gap-3 rounded-lg border border-border p-2.5 text-sm"
        >
          <input
            :type="item.select === 'many' ? 'checkbox' : 'radio'"
            name="hosted-option"
            class="mt-0.5"
            :checked="selected.has(position)"
            :disabled="sending || phase === 'reconnecting'"
            @change="toggle(position)"
          />
          <span class="text-foreground">{{ option }}</span>
        </label>
      </div>
      <AppButton :loading="sending" :disabled="phase === 'reconnecting'" data-testid="hosted-answer" @click="answer">
        {{ $t('learn.hosted.next') }}
      </AppButton>
    </div>

    <div v-else-if="phase === 'closing'" class="space-y-3 py-6 text-center text-sm text-muted-foreground">
      <p>{{ $t('learn.hosted.closing') }}</p>
      <template v-if="error">
        <p class="text-warning">{{ error }}</p>
        <AppButton variant="outline" @click="retryClose">{{ $t('common.actions.retry') }}</AppButton>
      </template>
    </div>

    <div v-else-if="phase === 'abandoned'" class="space-y-3 rounded-lg border border-warning/40 bg-warning/5 p-4 text-sm" data-testid="hosted-abandoned">
      <p class="font-medium text-foreground">{{ $t('learn.hosted.abandoned', { minutes: Math.round((terms?.terms.disconnect_grace_secs ?? 600) / 60) }) }}</p>
      <p class="text-muted-foreground">{{ retakeRule }}</p>
      <AppButton variant="outline" @click="router.back()">{{ $t('learn.hosted.back') }}</AppButton>
    </div>

    <!-- What the organisation decided -->
    <div v-else-if="phase === 'result' && outcome" class="space-y-4" data-testid="hosted-result">
      <h1 class="text-xl font-bold text-foreground">
        {{ $t('learn.hosted.scored', { percent: Math.round(outcome.result.score.fraction * 100), correct: outcome.result.score.correct, total: outcome.result.score.total }) }}
      </h1>
      <p class="text-sm text-muted-foreground">{{ $t('learn.hosted.scoredBy', { org: terms?.terms.organisation ?? target?.organization ?? '' }) }}</p>
      <p v-if="outcome.result.probes.asked > 0" class="text-sm text-muted-foreground">
        {{ $t('learn.hosted.probes', { passed: outcome.result.probes.passed, asked: outcome.result.probes.asked }) }}
      </p>

      <div v-if="outcome.issuance.state === 'issued'" class="rounded-lg border border-success/40 bg-success/5 p-3 text-sm" data-testid="hosted-issued">
        <p class="font-medium text-success">{{ $t('learn.hosted.issued') }}</p>
        <p v-if="outcome.issuance.collected !== 'stored' && outcome.issuance.collected !== 'already_present'" class="text-muted-foreground">
          {{ $t('learn.hosted.issuedLater') }}
        </p>
      </div>
      <div v-else-if="outcome.issuance.state === 'refused'" class="space-y-1 rounded-lg border border-warning/40 bg-warning/5 p-3 text-sm" data-testid="hosted-refused">
        <p class="font-medium text-foreground">{{ $t('learn.hosted.refused') }}</p>
        <p class="text-foreground">{{ outcome.issuance.refusal.message }}</p>
        <p class="text-muted-foreground">
          <template v-if="!outcome.issuance.refusal.retake_may_help">{{ $t('learn.hosted.retakeWontHelp') }}</template>
          <template v-else-if="attemptsLeft > 0">{{ retakeRule }}</template>
          <template v-else>{{ $t('learn.hosted.retakeLimit') }}</template>
        </p>
      </div>
      <div v-else-if="outcome.issuance.state === 'held'" class="space-y-2 rounded-lg border border-border bg-card p-3 text-sm" data-testid="hosted-held">
        <p class="text-foreground">{{ $t('learn.hosted.held') }}</p>
        <AppButton variant="outline" size="sm" :loading="checkingAgain" @click="checkAgain">{{ $t('learn.hosted.checkAgain') }}</AppButton>
      </div>
      <p v-else class="text-sm text-muted-foreground">{{ $t('learn.hosted.recorded') }}</p>
      <p v-if="error" class="text-sm text-error">{{ error }}</p>
      <AppButton variant="outline" @click="router.back()">{{ $t('learn.hosted.back') }}</AppButton>
    </div>

    <!-- Off-screen video the backend reads frames from (see AssessmentRunner). -->
    <video
      v-if="cameraStream"
      ref="cameraVideoRef"
      class="pointer-events-none fixed -start-[10000px] top-0 h-[240px] w-[320px] opacity-0"
      playsinline
      autoplay
      muted
      width="320"
      height="240"
    />
  </div>
</template>
