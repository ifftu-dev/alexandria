<script setup lang="ts">
// The candidate's door into an interview held in Alexandria. The panel opens
// the room; until then this waits, asking Cloud (with the learner's own proof)
// every few seconds. Joining hands off to the ordinary live room.
//
// When the organisation asked for the integrity check, the candidate chooses
// here, before joining, whether to run it. After the call they come back here
// and see exactly what was shared.
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { usePanel } from '@/composables/usePanel'
import { useTutoringRoom } from '@/composables/useTutoringRoom'
import { useInterviewCheck, type SentIntegrityReport } from '@/composables/useInterviewCheck'
import { AppAlert, AppButton } from '@/components/ui'

const POLL_MS = 5000

const route = useRoute()
const router = useRouter()
const panelApi = usePanel()
const tutoring = useTutoringRoom()
const interviewCheck = useInterviewCheck()

const interviewId = computed(() => String(route.params.id))
const cloud = computed(() => String(route.query.cloud ?? ''))
const ticket = ref<string | null>(null)
const checkAsked = ref(false)
const error = ref<string | null>(null)
const joining = ref(false)
/** `run`, `skip`, or no answer yet. Nothing is preselected. */
const checkChoice = ref<'run' | 'skip' | null>(null)
const checkCamera = ref(false)
const shared = ref<SentIntegrityReport[]>([])
const retrying = ref(false)
let timer: number | null = null

const needsChoice = computed(() => checkAsked.value && checkChoice.value === null)
const unsentHere = computed(() => interviewCheck.unsent.value?.interviewId === interviewId.value)

/** Sentinel's own learner-facing wording for a flag, keyed by its camelCase name. */
function flagKey(flag: string) {
  const camel = flag.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase())
  return `sentinel.flags.items.${camel}.description`
}

function minutes(r: SentIntegrityReport['report']) {
  return Math.max(0, Math.round((r.ended_at - r.started_at) / 60))
}

async function loadShared() {
  try {
    shared.value = await interviewCheck.sent(interviewId.value)
  } catch {
    shared.value = []
  }
}

async function check() {
  try {
    const room = await panelApi.candidateRoom(cloud.value, interviewId.value)
    error.value = null
    ticket.value = room.ticket
    checkAsked.value = room.integrityCheck === true
    if (room.ticket && timer) {
      window.clearInterval(timer)
      timer = null
    }
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  }
}

async function join() {
  if (!ticket.value || needsChoice.value) return
  joining.value = true
  error.value = null
  const runCheck = checkAsked.value && checkChoice.value === 'run'
  try {
    if (runCheck) {
      await interviewCheck.begin({ interviewId: interviewId.value, cloud: cloud.value, camera: checkCamera.value })
    }
    const room = await tutoring.joinRoom(ticket.value)
    await router.push(`/tutoring/${room.id}`)
  } catch (cause) {
    if (runCheck) await interviewCheck.abandon()
    error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    joining.value = false
  }
}

async function retrySend() {
  retrying.value = true
  try {
    if (await interviewCheck.send()) await loadShared()
  } finally {
    retrying.value = false
  }
}

onMounted(async () => {
  await Promise.all([check(), loadShared()])
  if (!ticket.value) timer = window.setInterval(() => void check(), POLL_MS)
})

onUnmounted(() => {
  if (timer) window.clearInterval(timer)
})
</script>

<template>
  <div class="mx-auto max-w-xl space-y-5 py-10">
    <div class="card p-6">
      <p class="text-xs font-semibold uppercase tracking-[0.14em] text-primary">{{ $t('tutoring.interviewRoom.eyebrow') }}</p>
      <h1 class="mt-2 text-xl font-semibold text-foreground">
        {{ ticket ? $t('tutoring.interviewRoom.ready') : $t('tutoring.interviewRoom.waiting') }}
      </h1>
      <p class="mt-2 text-sm leading-relaxed text-muted-foreground">{{ $t('tutoring.interviewRoom.explain') }}</p>
      <ul class="mt-4 space-y-1.5 text-sm text-muted-foreground">
        <li>• {{ $t('tutoring.interviewRoom.noRecording') }}</li>
        <li>• {{ $t('tutoring.interviewRoom.captions') }}</li>
        <li>• {{ $t('tutoring.interviewRoom.leave') }}</li>
      </ul>

      <section
        v-if="checkAsked"
        class="mt-6 rounded-xl border border-border bg-muted/40 p-4"
        data-testid="integrity-consent"
      >
        <h2 class="text-sm font-semibold text-foreground">{{ $t('tutoring.interviewRoom.check.title') }}</h2>
        <p class="mt-1.5 text-sm leading-relaxed text-muted-foreground">{{ $t('tutoring.interviewRoom.check.what') }}</p>
        <p class="mt-1.5 text-sm leading-relaxed text-muted-foreground">{{ $t('tutoring.interviewRoom.check.shared') }}</p>
        <p class="mt-1.5 text-sm leading-relaxed text-muted-foreground">{{ $t('tutoring.interviewRoom.check.optional') }}</p>
        <fieldset class="mt-3 space-y-2" :aria-label="$t('tutoring.interviewRoom.check.title')">
          <label class="flex items-center gap-2 text-sm text-foreground">
            <input v-model="checkChoice" type="radio" value="run" data-testid="check-run">
            {{ $t('tutoring.interviewRoom.check.run') }}
          </label>
          <label v-if="checkChoice === 'run'" class="ml-6 flex items-start gap-2 text-sm text-foreground">
            <input v-model="checkCamera" type="checkbox" class="mt-0.5" data-testid="check-camera">
            <span>
              {{ $t('tutoring.interviewRoom.check.camera') }}
              <span class="block text-xs text-muted-foreground">{{ $t('tutoring.interviewRoom.check.cameraHint') }}</span>
            </span>
          </label>
          <label class="flex items-center gap-2 text-sm text-foreground">
            <input v-model="checkChoice" type="radio" value="skip" data-testid="check-skip">
            {{ $t('tutoring.interviewRoom.check.skip') }}
          </label>
        </fieldset>
      </section>

      <AppAlert v-if="error" variant="error" class="mt-4">{{ error }}</AppAlert>
      <div class="mt-6 flex items-center gap-3">
        <AppButton :disabled="!ticket || needsChoice" :loading="joining" data-testid="join-interview" @click="join">
          {{ $t('tutoring.interviewRoom.join') }}
        </AppButton>
        <span v-if="!ticket" class="text-xs text-muted-foreground">{{ $t('tutoring.interviewRoom.checking') }}</span>
        <span v-else-if="needsChoice" class="text-xs text-muted-foreground">{{ $t('tutoring.interviewRoom.check.choose') }}</span>
      </div>
    </div>

    <div v-if="unsentHere" class="card p-6" data-testid="integrity-unsent">
      <h2 class="text-sm font-semibold text-foreground">{{ $t('tutoring.interviewRoom.check.unsentTitle') }}</h2>
      <p class="mt-1.5 text-sm text-muted-foreground">{{ $t('tutoring.interviewRoom.check.unsentBody') }}</p>
      <AppAlert v-if="interviewCheck.sendError.value" variant="error" class="mt-3">{{ interviewCheck.sendError.value }}</AppAlert>
      <AppButton class="mt-4" variant="secondary" :loading="retrying" data-testid="integrity-retry" @click="retrySend">
        {{ $t('tutoring.interviewRoom.check.retry') }}
      </AppButton>
    </div>

    <div v-for="s in shared" :key="s.integrity_session_id" class="card p-6" data-testid="integrity-shared">
      <h2 class="text-sm font-semibold text-foreground">{{ $t('tutoring.interviewRoom.check.sharedTitle', { org: s.organization }) }}</h2>
      <dl class="mt-3 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-sm">
        <dt class="text-muted-foreground">{{ $t('tutoring.interviewRoom.check.outcome') }}</dt>
        <dd class="text-foreground">
          {{ s.report.outcome === 'clear' ? $t('tutoring.interviewRoom.check.clear') : $t('tutoring.interviewRoom.check.flagged') }}<template v-if="s.report.score !== null"> · {{ s.report.score }} / 100</template>
        </dd>
        <dt class="text-muted-foreground">{{ $t('tutoring.interviewRoom.check.ran') }}</dt>
        <dd class="text-foreground">{{ $t('tutoring.interviewRoom.check.ranFor', { minutes: minutes(s.report), snapshots: s.report.snapshots }) }}</dd>
        <dt class="text-muted-foreground">{{ $t('tutoring.interviewRoom.check.cameraLabel') }}</dt>
        <dd class="text-foreground">{{ s.report.camera ? $t('tutoring.interviewRoom.check.cameraOn', { percent: s.report.camera_coverage }) : $t('tutoring.interviewRoom.check.cameraOff') }}</dd>
        <dt class="text-muted-foreground">{{ $t('tutoring.interviewRoom.check.flags') }}</dt>
        <dd class="text-foreground">
          <template v-if="s.report.flags.length">
            <span v-for="f in s.report.flags" :key="f.flag" class="block">{{ $te(flagKey(f.flag)) ? $t(flagKey(f.flag)) : f.flag }} × {{ f.count }}</span>
          </template>
          <template v-else>{{ $t('tutoring.interviewRoom.check.none') }}</template>
        </dd>
      </dl>
      <p class="mt-3 text-xs text-muted-foreground">{{ $t('tutoring.interviewRoom.check.sharedNote') }}</p>
    </div>
  </div>
</template>
