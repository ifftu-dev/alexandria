<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useInterviews } from '@/composables/useInterviews'
import { useLocalSpeechRecognition } from '@/composables/useLocalSpeechRecognition'
import { usePlatform } from '@/composables/usePlatform'
import { useSentinel } from '@/composables/useSentinel'
import { useTutoringRoom } from '@/composables/useTutoringRoom'
import SentinelLiveIndicator from '@/components/integrity/SentinelLiveIndicator.vue'
import { LiveDiagnosticsModal, LiveParticipantTile, LiveSessionControls } from '@/components/live'
import QrCodeDisplay from '@/components/tutoring/QrCodeDisplay.vue'
import { AppAlert, AppBadge, AppButton, AppModal, AppTextarea } from '@/components/ui'
import type {
  DeviceList,
  InterviewConsentRequest,
  InterviewCriterionStatus,
  InterviewParticipant,
  TutoringTranscriptMessage,
} from '@/types'

const route = useRoute()
const router = useRouter()
const interviewId = computed(() => String(route.params.id))

const {
  activeInterview,
  loading,
  error,
  getInterview,
  setStatus,
  recordConsent,
  appendTranscript,
  recommendFollowups,
  setFollowupStatus,
  setCriterion,
  saveNote,
  generateSummary,
} = useInterviews()
const tutoring = useTutoringRoom()
const sentinel = useSentinel()
const speech = useLocalSpeechRecognition()
const { isMobilePlatform, isIOS } = usePlatform()

const showConsent = ref(false)
const consentParticipant = ref<InterviewParticipant | null>(null)
const consentDraft = ref<InterviewConsentRequest>({
  consent_transcription: false,
  consent_audio_recording: false,
  consent_video_recording: false,
  consent_sentinel: false,
  consent_camera: false,
})
const showInvite = ref(false)
const showDiagnostics = ref(false)
const showSentinelDebug = ref(false)
const showChat = ref(false)
const showAudioDevices = ref(false)
const diagnostics = ref<Record<string, unknown> | null>(null)
const availableDevices = ref<DeviceList | null>(null)
const selectedMic = ref<string | null>(null)
const selectedOutput = ref<string | null>(null)
const deviceLoading = ref(false)
const ticketCopied = ref(false)
const selectedSpeakerId = ref('')
const manualTranscript = ref('')
const privateNote = ref('')
const chatInput = ref('')
const sessionStartedAt = ref(0)
const elapsedSeconds = ref(0)
const selfVideo = ref<HTMLVideoElement | null>(null)
const selfStream = ref<MediaStream | null>(null)
let elapsedTimer: number | null = null
let gazeTimer: number | null = null
let faceTimer: number | null = null
let gazeInFlight = false
const processedRemoteTranscript = new Set<string>()
const unassignedRemoteCount = ref(0)

const session = computed(() => activeInterview.value?.session ?? null)
const participants = computed(() => activeInterview.value?.participants ?? [])
const candidate = computed(() => participants.value.find(item => item.role === 'candidate') ?? null)
const interviewer = computed(() => participants.value.find(item => item.role === 'interviewer') ?? null)
const selectedSpeaker = computed(() => participants.value.find(item => item.id === selectedSpeakerId.value) ?? null)
const isLive = computed(() => session.value?.status === 'live')
const tutoringActive = computed(() => tutoring.sessionStatus.value?.session_id === session.value?.tutoring_session_id)
const peers = computed(() => tutoring.sessionStatus.value?.peers ?? [])
const transcript = computed(() => activeInterview.value?.transcript ?? [])
const suggestions = computed(() => activeInterview.value?.followups.filter(item => item.status === 'suggested') ?? [])
const criteria = computed(() => activeInterview.value?.criteria ?? [])
const allRequestedConsentsReady = computed(() => participants.value.every(
  participant => participant.consented_at !== null,
))
const connectionQuality = computed(() => {
  if (!tutoringActive.value) return 'Offline'
  if (peers.value.length === 0) return 'Ready'
  const connected = peers.value.filter(peer => peer.connected).length
  if (connected === peers.value.length) return 'Good'
  if (connected > 0) return 'Fair'
  return 'Poor'
})
const formattedDuration = computed(() => {
  const hours = Math.floor(elapsedSeconds.value / 3600)
  const minutes = Math.floor((elapsedSeconds.value % 3600) / 60)
  const seconds = elapsedSeconds.value % 60
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`
    : `${minutes}:${String(seconds).padStart(2, '0')}`
})

onMounted(async () => {
  const bundle = await getInterview(interviewId.value)
  if (!bundle) return
  selectedSpeakerId.value = bundle.participants.find(item => item.role === 'candidate')?.id
    ?? bundle.participants[0]?.id
    ?? ''
  if (bundle.notes[0]) privateNote.value = bundle.notes[0].text
  await tutoring.setupEventListeners()
  await tutoring.refreshStatus()
  if (bundle.session.status === 'live') {
    tutoring.startPolling(2000)
    sessionStartedAt.value = bundle.session.started_at
      ? new Date(bundle.session.started_at).getTime()
      : Date.now()
    startElapsedTimer()
  }
})

onUnmounted(() => {
  speech.stop()
  stopSelfPreview()
  stopSentinelCameraLoops()
  if (elapsedTimer) window.clearInterval(elapsedTimer)
})

watch(() => tutoring.sessionStatus.value?.video_enabled, (enabled) => {
  if (enabled) startSelfPreview()
  else stopSelfPreview()
})

watch(selfVideo, () => {
  if (selfVideo.value && selfStream.value) {
    selfVideo.value.srcObject = selfStream.value
    selfVideo.value.play().catch(() => undefined)
  }
})

watch(() => tutoring.transcriptMessages.value, (messages) => {
  for (const message of messages) {
    if (message.sender === 'self') continue
    const key = `${message.sender}:${message.timestamp}:${message.text}`
    if (processedRemoteTranscript.has(key)) continue
    processedRemoteTranscript.add(key)
    persistRemoteTranscript(message).catch(() => { unassignedRemoteCount.value += 1 })
  }
})

function startElapsedTimer() {
  if (elapsedTimer) window.clearInterval(elapsedTimer)
  elapsedTimer = window.setInterval(() => {
    elapsedSeconds.value = Math.max(0, Math.floor((Date.now() - sessionStartedAt.value) / 1000))
  }, 1000)
}

function openConsent(participant: InterviewParticipant) {
  consentParticipant.value = participant
  consentDraft.value = {
    consent_transcription: participant.consent_transcription,
    consent_audio_recording: participant.consent_audio_recording,
    consent_video_recording: participant.consent_video_recording,
    consent_sentinel: participant.consent_sentinel,
    consent_camera: participant.consent_camera,
  }
  showConsent.value = true
}

async function saveConsent() {
  if (!consentParticipant.value) return
  await recordConsent(consentParticipant.value.id, consentDraft.value)
  showConsent.value = false
}

async function startInterview() {
  if (!session.value || !allRequestedConsentsReady.value) return
  const room = await tutoring.createRoom(
    `Interview · ${session.value.title}`,
    interviewer.value?.display_name ?? 'Interviewer',
  )
  // Sentinel observes this conductor device. Prefer the interviewer attached
  // to that device; a candidate-only interview falls back to its sole party.
  const monitorParticipant = interviewer.value ?? candidate.value
  if (session.value.sentinel_enabled && monitorParticipant?.consent_sentinel) {
    await sentinel.start(null, monitorParticipant.consent_camera, 'interview')
  }
  await setStatus(session.value.id, 'live', room.id, sentinel.getSessionId() ?? undefined)
  sessionStartedAt.value = Date.now()
  startElapsedTimer()
  tutoring.startPolling(2000)
  if (tutoring.sessionStatus.value?.video_enabled) await startSelfPreview()
}

async function endInterview() {
  speech.stop()
  stopSelfPreview()
  stopSentinelCameraLoops()
  if (sentinel.isActive.value) await sentinel.stop()
  if (tutoringActive.value) await tutoring.leaveRoom()
  if (session.value) {
    await setStatus(session.value.id, 'completed')
    await generateSummary(session.value.id)
    await router.push(`/interviews/${session.value.id}/review`)
  }
}

async function startSelfPreview() {
  if (selfStream.value || isMobilePlatform) return
  try {
    selfStream.value = await navigator.mediaDevices.getUserMedia({
      video: { width: { ideal: 1280 }, height: { ideal: 720 }, frameRate: { ideal: 30 } },
      audio: false,
    })
    await nextTick()
    if (selfVideo.value) {
      selfVideo.value.srcObject = selfStream.value
      await selfVideo.value.play().catch(() => undefined)
    }
    if (sentinel.cameraOptedIn.value) startSentinelCameraLoops()
  } catch {
    // The tutoring backend may already own the camera; its JPEG self frame
    // remains the visual fallback and non-camera Sentinel signals continue.
  }
}

function stopSelfPreview() {
  if (selfStream.value) {
    for (const track of selfStream.value.getTracks()) track.stop()
  }
  selfStream.value = null
  if (selfVideo.value) selfVideo.value.srcObject = null
}

function startSentinelCameraLoops() {
  if (gazeTimer || faceTimer) return
  gazeTimer = window.setInterval(() => {
    if (!selfVideo.value || gazeInFlight) return
    gazeInFlight = true
    sentinel.scoreGaze(selfVideo.value).finally(() => { gazeInFlight = false })
  }, 1500)
  faceTimer = window.setInterval(() => {
    if (selfVideo.value) sentinel.verifyFace(selfVideo.value)
  }, 3000)
}

function stopSentinelCameraLoops() {
  if (gazeTimer) window.clearInterval(gazeTimer)
  if (faceTimer) window.clearInterval(faceTimer)
  gazeTimer = null
  faceTimer = null
  gazeInFlight = false
}

async function addTranscriptText(text: string, source: 'local_stt' | 'manual', confidence?: number) {
  const speaker = selectedSpeaker.value
  if (!speaker || !speaker.consent_transcription || !text.trim()) return
  const segment = await appendTranscript({
    session_id: interviewId.value,
    participant_id: speaker.id,
    speaker_label: speaker.display_name,
    text: text.trim(),
    start_ms: elapsedSeconds.value * 1000,
    end_ms: elapsedSeconds.value * 1000,
    is_final: true,
    confidence,
    source,
  })
  if (source === 'local_stt') {
    await tutoring.sendTranscript(text.trim(), confidence)
  }
  await recommendFollowups(interviewId.value, segment.id)
}

async function persistRemoteTranscript(message: TutoringTranscriptMessage) {
  const eligible = participants.value.filter(item => item.consent_transcription && !item.revoked_at)
  const normalizedName = message.sender_name?.trim().toLocaleLowerCase()
  const participant = eligible.find(item => item.peer_id === message.sender)
    ?? eligible.find(item => normalizedName && item.display_name.trim().toLocaleLowerCase() === normalizedName)
    ?? (eligible.length === 1 ? eligible[0] : undefined)
  if (!participant) throw new Error('Remote transcript speaker could not be matched to a consenting participant')
  const relativeTime = Math.max(0, message.timestamp - sessionStartedAt.value)
  const segment = await appendTranscript({
    session_id: interviewId.value,
    participant_id: participant.id,
    speaker_label: participant.display_name,
    text: message.text,
    start_ms: relativeTime,
    end_ms: relativeTime,
    is_final: true,
    confidence: message.confidence ?? undefined,
    source: 'remote_stt',
  })
  await recommendFollowups(interviewId.value, segment.id)
}

function toggleTranscription() {
  if (speech.listening.value) {
    speech.stop()
    return
  }
  const speaker = selectedSpeaker.value
  if (!speaker?.consent_transcription) return
  speech.start((segment) => {
    if (segment.isFinal) addTranscriptText(segment.text, 'local_stt', segment.confidence).catch(() => undefined)
  })
}

async function submitManualTranscript() {
  const text = manualTranscript.value
  manualTranscript.value = ''
  await addTranscriptText(text, 'manual')
}

async function savePrivateNote() {
  if (!privateNote.value.trim()) return
  await saveNote(interviewId.value, privateNote.value, activeInterview.value?.notes[0]?.id)
}

async function cycleCriterion(id: string, current: InterviewCriterionStatus) {
  const next: InterviewCriterionStatus = current === 'not_covered'
    ? 'partial'
    : current === 'partial' ? 'covered' : 'not_covered'
  await setCriterion(id, next)
  await recommendFollowups(interviewId.value)
}

async function sendChat() {
  const text = chatInput.value.trim()
  if (!text) return
  chatInput.value = ''
  await tutoring.sendChat(text)
}

async function openDebug() {
  diagnostics.value = await tutoring.getDiagnostics()
  showDiagnostics.value = true
}

async function openAudioDevices() {
  deviceLoading.value = true
  try {
    availableDevices.value = await tutoring.listDevices()
    selectedMic.value = availableDevices.value.selected_audio_input
      ?? availableDevices.value.audio_inputs.find(item => item.is_default)?.id
      ?? availableDevices.value.audio_inputs[0]?.id
      ?? null
    selectedOutput.value = availableDevices.value.selected_audio_output
      ?? availableDevices.value.audio_outputs.find(item => item.is_default)?.id
      ?? availableDevices.value.audio_outputs[0]?.id
      ?? null
    showAudioDevices.value = true
  } finally {
    deviceLoading.value = false
  }
}

async function applyAudioDevices() {
  await tutoring.setAudioDevices(selectedMic.value, selectedOutput.value)
  showAudioDevices.value = false
}

async function copyTicket() {
  const ticket = tutoring.sessionStatus.value?.ticket
  if (!ticket) return
  try {
    await navigator.clipboard.writeText(ticket)
    ticketCopied.value = true
    window.setTimeout(() => { ticketCopied.value = false }, 2000)
  } catch {
    showInvite.value = true
  }
}

function participantInitials(name: string) {
  return name.split(/\s+/).map(part => part[0]).filter(Boolean).slice(0, 2).join('').toUpperCase()
}

function formatTime(ms: number) {
  const total = Math.floor(ms / 1000)
  return `${String(Math.floor(total / 60)).padStart(2, '0')}:${String(total % 60).padStart(2, '0')}`
}
</script>

<template>
  <div v-if="activeInterview" class="-m-4 flex min-h-[calc(100dvh-4rem)] flex-col bg-background sm:-m-6 lg:-m-8">
    <header class="flex min-h-14 flex-wrap items-center gap-3 border-b border-border bg-card px-3 py-2 sm:px-5">
      <button class="rounded-lg p-2 text-muted-foreground hover:bg-muted hover:text-foreground" :title="$t('interviews.session.back')" @click="router.push('/interviews')">
        <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" /></svg>
      </button>
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2">
          <span v-if="isLive" class="relative flex h-2 w-2"><span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-success opacity-70" /><span class="relative h-2 w-2 rounded-full bg-success" /></span>
          <AppBadge v-else :variant="session?.status === 'completed' ? 'secondary' : 'warning'">{{ session?.status }}</AppBadge>
          <h1 class="truncate text-sm font-semibold text-foreground">{{ session?.title }}</h1>
          <span v-if="isLive" class="font-mono text-xs tabular-nums text-muted-foreground">{{ formattedDuration }}</span>
        </div>
        <p class="mt-0.5 hidden truncate text-xs text-muted-foreground sm:block">{{ session?.objective || $t('interviews.session.structuredInterview') }}</p>
      </div>

      <template v-if="isLive">
        <div class="hidden items-center gap-1.5 rounded-full border border-border bg-muted/40 px-2.5 py-1 text-xs text-muted-foreground sm:flex">
          <span class="h-1.5 w-1.5 rounded-full" :class="connectionQuality === 'Good' || connectionQuality === 'Ready' ? 'bg-success' : connectionQuality === 'Fair' ? 'bg-warning' : 'bg-destructive'" />
          {{ connectionQuality }} · {{ peers.filter(peer => peer.connected).length }}/{{ peers.length }} {{ $t('interviews.common.peers') }}
        </div>
        <AppButton variant="outline" size="xs" @click="showSentinelDebug = true">Sentinel</AppButton>
        <AppButton variant="outline" size="xs" @click="openDebug">{{ $t('interviews.session.diagnostics') }}</AppButton>
        <AppButton v-if="isIOS" variant="outline" size="xs" :loading="deviceLoading" @click="openAudioDevices">{{ $t('interviews.session.audio') }}</AppButton>
        <AppButton v-if="tutoring.sessionStatus.value?.ticket" variant="outline" size="xs" @click="copyTicket">{{ ticketCopied ? $t('interviews.common.copied') : $t('interviews.session.invite') }}</AppButton>
        <AppButton variant="outline" size="xs" @click="showChat = !showChat">{{ $t('interviews.session.chat') }}<span v-if="tutoring.unreadChatCount.value"> · {{ tutoring.unreadChatCount.value }}</span></AppButton>
      </template>
    </header>

    <AppAlert v-if="error || tutoring.lastError.value" variant="error" class="m-3">{{ error || tutoring.lastError.value }}</AppAlert>

    <main v-if="!isLive" class="flex-1 overflow-auto p-4 sm:p-6">
      <div class="mx-auto grid max-w-6xl gap-5 lg:grid-cols-[minmax(0,1fr)_22rem]">
        <section class="space-y-5">
          <div class="card p-5 sm:p-6">
            <div class="flex items-start justify-between gap-4">
              <div>
                <p class="text-xs font-semibold uppercase tracking-[0.14em] text-primary">{{ $t('interviews.session.preparation') }}</p>
                <h2 class="mt-2 text-xl font-semibold text-foreground">{{ $t('interviews.session.consentTitle') }}</h2>
                <p class="mt-2 max-w-2xl text-sm leading-relaxed text-muted-foreground">{{ $t('interviews.session.consentDescription') }}</p>
              </div>
              <div class="rounded-lg bg-muted px-3 py-2 text-right">
                <p class="text-[0.65rem] uppercase tracking-wide text-muted-foreground">{{ $t('interviews.session.retention') }}</p>
                <p class="text-sm font-semibold text-foreground">{{ session?.retention_days }} {{ $t('interviews.common.days') }}</p>
              </div>
            </div>

            <div class="mt-6 grid gap-3 sm:grid-cols-2">
              <article v-for="participant in participants" :key="participant.id" class="rounded-xl border border-border bg-background p-4">
                <div class="flex items-start gap-3">
                  <div class="grid h-10 w-10 place-items-center rounded-full bg-primary/10 text-sm font-semibold text-primary">{{ participantInitials(participant.display_name) }}</div>
                  <div class="min-w-0 flex-1">
                    <p class="truncate text-sm font-semibold text-foreground">{{ participant.display_name }}</p>
                    <p class="text-xs text-muted-foreground"><span class="capitalize">{{ participant.role }}</span> · {{ $t('interviews.session.storedAs', { pseudonym: participant.pseudonym }) }}</p>
                  </div>
                  <AppBadge :variant="participant.consented_at ? 'success' : 'warning'">{{ $t(participant.consented_at ? 'interviews.session.choiceRecorded' : 'interviews.session.choiceNeeded') }}</AppBadge>
                </div>
                <div class="mt-4 flex flex-wrap gap-1.5 text-[0.68rem]">
                  <span class="rounded-full bg-muted px-2 py-1" :class="participant.consent_transcription ? 'text-success' : 'text-muted-foreground'">{{ $t('interviews.session.transcriptState', { state: $t(participant.consent_transcription ? 'interviews.session.on' : 'interviews.session.off') }) }}</span>
                  <span v-if="session?.sentinel_enabled" class="rounded-full bg-muted px-2 py-1" :class="participant.consent_sentinel ? 'text-success' : 'text-muted-foreground'">{{ $t('interviews.session.sentinelState', { state: $t(participant.consent_sentinel ? 'interviews.session.on' : 'interviews.session.off') }) }}</span>
                  <span class="rounded-full bg-muted px-2 py-1 text-muted-foreground">{{ $t('interviews.session.cameraState', { state: $t(participant.consent_camera ? 'interviews.session.on' : 'interviews.session.off') }) }}</span>
                </div>
                <AppButton class="mt-4 w-full" variant="outline" size="sm" @click="openConsent(participant)">{{ $t('interviews.session.reviewParticipant') }}</AppButton>
              </article>
            </div>
          </div>

          <div class="card p-5">
            <div class="flex items-center justify-between">
              <h2 class="text-sm font-semibold text-foreground">{{ $t('interviews.session.criteria') }}</h2>
              <span class="text-xs text-muted-foreground">{{ $t('interviews.session.criteriaHint') }}</span>
            </div>
            <div class="mt-4 grid gap-2 sm:grid-cols-2">
              <div v-for="(criterion, index) in criteria" :key="criterion.id" class="flex items-center gap-3 rounded-lg border border-border px-3 py-2.5">
                <span class="font-mono text-xs text-muted-foreground">{{ String(index + 1).padStart(2, '0') }}</span>
                <span class="text-sm text-foreground">{{ criterion.label }}</span>
              </div>
            </div>
          </div>
        </section>

        <aside class="space-y-4">
          <div class="card border-primary/20 p-5">
            <p class="text-xs font-semibold uppercase tracking-[0.13em] text-primary">{{ $t('interviews.session.readyCheck') }}</p>
            <ul class="mt-4 space-y-3 text-sm">
              <li class="flex items-start gap-2"><span class="mt-1 h-2 w-2 rounded-full" :class="allRequestedConsentsReady ? 'bg-success' : 'bg-warning'" /><span class="text-muted-foreground">{{ $t('interviews.session.consentReady', { state: $t(allRequestedConsentsReady ? 'interviews.session.recorded' : 'interviews.session.needed') }) }}</span></li>
              <li class="flex items-start gap-2"><span class="mt-1 h-2 w-2 rounded-full bg-success" /><span class="text-muted-foreground">{{ $t('interviews.session.localRecord') }}</span></li>
              <li class="flex items-start gap-2"><span class="mt-1 h-2 w-2 rounded-full bg-success" /><span class="text-muted-foreground">{{ $t('interviews.session.notGossiped') }}</span></li>
              <li class="flex items-start gap-2"><span class="mt-1 h-2 w-2 rounded-full bg-success" /><span class="text-muted-foreground">{{ $t('interviews.session.expires', { date: new Date(session?.expires_at ?? '').toLocaleDateString() }) }}</span></li>
            </ul>
            <AppButton class="mt-5 w-full" :loading="loading" :disabled="!allRequestedConsentsReady" @click="startInterview">{{ $t('interviews.session.start') }}</AppButton>
            <p v-if="!allRequestedConsentsReady" class="mt-2 text-center text-xs text-muted-foreground">{{ $t('interviews.session.recordChoicesFirst') }}</p>
          </div>
        </aside>
      </div>
    </main>

    <main v-else class="grid min-h-0 flex-1 grid-cols-1 overflow-hidden lg:grid-cols-[minmax(20rem,1fr)_22rem_20rem]">
      <section class="flex min-h-0 flex-col border-e border-border bg-muted/20">
        <div class="flex-1 overflow-auto p-3">
          <div class="grid min-h-full content-center gap-3" :class="peers.length > 0 ? 'grid-cols-2' : 'grid-cols-1'">
            <LiveParticipantTile
              v-for="peer in peers"
              :key="peer.node_id"
              :name="peer.display_name || peer.node_id.slice(0, 8)"
              :initials="participantInitials(peer.display_name || peer.node_id)"
              :video-src="tutoring.videoFrames.value[peer.node_id]"
              :connected="peer.connected"
              label="Remote participant"
            />
            <LiveParticipantTile
              :name="interviewer?.display_name || 'Interviewer'"
              :initials="participantInitials(interviewer?.display_name || 'Interviewer')"
              :video-src="tutoring.videoFrames.value.self"
              :muted="!tutoring.sessionStatus.value?.audio_enabled"
              self
              :compact="peers.length > 0"
              label="This device"
            >
              <template v-if="selfStream" #video>
                <video ref="selfVideo" autoplay muted playsinline class="absolute inset-0 h-full w-full scale-x-[-1] object-cover" />
              </template>
            </LiveParticipantTile>
          </div>
        </div>
        <LiveSessionControls
          :audio-enabled="tutoring.sessionStatus.value?.audio_enabled ?? false"
          :video-enabled="tutoring.sessionStatus.value?.video_enabled ?? false"
          :screen-sharing="tutoring.sessionStatus.value?.screen_sharing ?? false"
          :mic-level="tutoring.micLevel.value"
          :incoming-level="tutoring.outputLevel.value"
          :can-share-screen="!isMobilePlatform"
          end-label="End interview"
          @toggle-audio="tutoring.toggleAudio(!(tutoring.sessionStatus.value?.audio_enabled ?? false))"
          @toggle-video="tutoring.toggleVideo(!(tutoring.sessionStatus.value?.video_enabled ?? false))"
          @toggle-screen-share="tutoring.toggleScreenShare(!(tutoring.sessionStatus.value?.screen_sharing ?? false))"
          @end="endInterview"
        />
      </section>

      <section class="flex min-h-0 flex-col border-e border-border bg-card">
        <div class="border-b border-border p-3">
          <div class="flex items-center justify-between gap-2">
            <div>
              <h2 class="text-sm font-semibold text-foreground">{{ $t('interviews.session.liveTranscript') }}</h2>
              <p class="text-[0.68rem] text-muted-foreground">{{ $t('interviews.session.attributedLocal') }}</p>
              <p v-if="unassignedRemoteCount" class="mt-1 text-[0.65rem] text-warning">{{ $t('interviews.session.unassignedRemote', { count: unassignedRemoteCount }) }}</p>
            </div>
            <button
              class="rounded-full px-2.5 py-1 text-xs font-medium"
              :class="speech.listening.value ? 'bg-destructive/10 text-destructive' : speech.locallyAvailable.value ? 'bg-success/10 text-success' : 'bg-muted text-muted-foreground'"
              :disabled="!selectedSpeaker?.consent_transcription"
              @click="toggleTranscription"
            >
              {{ speech.listening.value ? $t('interviews.session.stopListening') : speech.locallyAvailable.value ? $t('interviews.session.startStt') : $t('interviews.session.sttUnavailable') }}
            </button>
          </div>
          <label class="mt-3 block text-[0.68rem] font-medium text-muted-foreground">{{ $t('interviews.session.speakerLabel') }}</label>
          <select v-model="selectedSpeakerId" class="input mt-1 py-1.5 text-xs">
            <option v-for="participant in participants" :key="participant.id" :value="participant.id">{{ participant.display_name }} · {{ participant.role }}</option>
          </select>
          <p v-if="speech.error.value" class="mt-2 text-xs text-warning">{{ speech.error.value }} {{ $t('interviews.session.manualFallback') }}</p>
        </div>

        <div class="flex-1 space-y-3 overflow-y-auto p-3">
          <div v-if="transcript.length === 0" class="rounded-xl border border-dashed border-border p-5 text-center">
            <p class="text-sm font-medium text-foreground">{{ $t('interviews.session.emptyTranscript') }}</p>
            <p class="mt-1 text-xs leading-relaxed text-muted-foreground">{{ $t('interviews.session.emptyTranscriptHint') }}</p>
          </div>
          <article v-for="segment in transcript" :key="segment.id" class="rounded-xl border border-border bg-background p-3">
            <div class="flex items-center justify-between gap-2">
              <p class="truncate text-xs font-semibold text-primary">{{ segment.speaker_label }}</p>
              <span class="font-mono text-[0.65rem] text-muted-foreground">{{ formatTime(segment.start_ms) }}</span>
            </div>
            <p class="mt-1.5 text-sm leading-relaxed text-foreground">{{ segment.text }}</p>
            <p class="mt-2 text-[0.62rem] uppercase tracking-wide text-muted-foreground">{{ segment.source.replace('_', ' ') }}</p>
          </article>
          <div v-if="speech.interimText.value" class="rounded-xl border border-primary/20 bg-primary/5 p-3 text-sm italic text-muted-foreground">{{ speech.interimText.value }}</div>
        </div>

        <form class="border-t border-border p-3" @submit.prevent="submitManualTranscript">
          <textarea v-model="manualTranscript" rows="2" class="input resize-none text-sm" :placeholder="$t('interviews.session.manualPlaceholder')" />
          <div class="mt-2 flex items-center justify-between gap-2">
            <span class="text-[0.65rem] text-muted-foreground">{{ $t('interviews.session.requiresConsent', { name: selectedSpeaker?.display_name || $t('interviews.session.speaker') }) }}</span>
            <AppButton type="submit" size="xs" :disabled="!manualTranscript.trim() || !selectedSpeaker?.consent_transcription">{{ $t('interviews.common.add') }}</AppButton>
          </div>
        </form>
      </section>

      <aside class="flex min-h-0 flex-col bg-background">
        <div class="border-b border-border p-3">
          <h2 class="text-sm font-semibold text-foreground">{{ $t('interviews.session.assistant') }}</h2>
          <p class="text-[0.68rem] text-muted-foreground">{{ $t('interviews.session.assistantHint') }}</p>
        </div>
        <div class="flex-1 space-y-5 overflow-y-auto p-3">
          <section>
            <div class="mb-2 flex items-center justify-between"><h3 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{{ $t('interviews.session.followups') }}</h3><span class="text-[0.65rem] text-muted-foreground">{{ suggestions.length }}</span></div>
            <div class="space-y-2">
              <article v-for="suggestion in suggestions.slice(0, 4)" :key="suggestion.id" class="rounded-xl border border-primary/20 bg-primary/[0.035] p-3">
                <p class="text-sm font-medium leading-snug text-foreground">{{ suggestion.question }}</p>
                <p class="mt-1 text-xs leading-relaxed text-muted-foreground">{{ suggestion.reason }}</p>
                <div class="mt-3 flex gap-2">
                  <AppButton size="xs" @click="setFollowupStatus(suggestion.id, 'asked')">{{ $t('interviews.session.markAsked') }}</AppButton>
                  <AppButton size="xs" variant="ghost" @click="setFollowupStatus(suggestion.id, 'dismissed')">{{ $t('interviews.session.dismiss') }}</AppButton>
                </div>
              </article>
              <p v-if="suggestions.length === 0" class="rounded-lg border border-dashed border-border p-3 text-xs leading-relaxed text-muted-foreground">{{ $t('interviews.session.suggestionsEmpty') }}</p>
            </div>
          </section>

          <section>
            <h3 class="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{{ $t('interviews.session.coverage') }}</h3>
            <button v-for="criterion in criteria" :key="criterion.id" class="mb-1.5 flex w-full items-center gap-2 rounded-lg border border-border bg-card px-2.5 py-2 text-left hover:border-primary/40" @click="cycleCriterion(criterion.id, criterion.status)">
              <span class="h-2 w-2 shrink-0 rounded-full" :class="criterion.status === 'covered' ? 'bg-success' : criterion.status === 'partial' ? 'bg-warning' : 'bg-border'" />
              <span class="min-w-0 flex-1 truncate text-xs text-foreground">{{ criterion.label }}</span>
              <span class="text-[0.62rem] capitalize text-muted-foreground">{{ criterion.status.replace('_', ' ') }}</span>
            </button>
          </section>

          <section>
            <h3 class="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{{ $t('interviews.session.privateNotes') }}</h3>
            <AppTextarea v-model="privateNote" :rows="5" :placeholder="$t('interviews.session.notesPlaceholder')" />
            <AppButton class="mt-2" size="xs" variant="outline" :disabled="!privateNote.trim()" @click="savePrivateNote">{{ $t('interviews.session.saveNote') }}</AppButton>
          </section>
        </div>
      </aside>
    </main>

    <AppModal :open="showConsent" :title="`Consent · ${consentParticipant?.display_name ?? ''}`" max-width="34rem" @close="showConsent = false">
      <p class="text-sm leading-relaxed text-muted-foreground">{{ $t('interviews.session.consentIntro') }}</p>
      <div class="mt-5 space-y-3">
        <label class="consent-row"><input v-model="consentDraft.consent_transcription" type="checkbox" class="accent-primary" /><span><strong>{{ $t('interviews.session.attributedTranscript') }}</strong><small>{{ $t('interviews.session.attributedTranscriptHelp') }}</small></span></label>
        <label v-if="session?.record_audio" class="consent-row"><input v-model="consentDraft.consent_audio_recording" type="checkbox" class="accent-primary" /><span><strong>{{ $t('interviews.session.audioRecording') }}</strong><small>{{ $t('interviews.session.recordingHelp') }}</small></span></label>
        <label v-if="session?.record_video" class="consent-row"><input v-model="consentDraft.consent_video_recording" type="checkbox" class="accent-primary" /><span><strong>{{ $t('interviews.session.videoRecording') }}</strong><small>{{ $t('interviews.session.recordingHelp') }}</small></span></label>
        <label v-if="session?.sentinel_enabled" class="consent-row"><input v-model="consentDraft.consent_sentinel" type="checkbox" class="accent-primary" /><span><strong>{{ $t('interviews.session.sentinelSignals') }}</strong><small>{{ $t('interviews.session.sentinelHelp') }}</small></span></label>
        <label v-if="session?.sentinel_enabled" class="consent-row"><input v-model="consentDraft.consent_camera" type="checkbox" class="accent-primary" /><span><strong>{{ $t('interviews.session.cameraSentinel') }}</strong><small>{{ $t('interviews.session.cameraSentinelHelp') }}</small></span></label>
      </div>
      <template #footer><div class="flex justify-end gap-2"><AppButton variant="outline" @click="showConsent = false">{{ $t('interviews.common.cancel') }}</AppButton><AppButton @click="saveConsent">{{ $t('interviews.session.recordChoices') }}</AppButton></div></template>
    </AppModal>

    <AppModal :open="showInvite" :title="$t('interviews.session.inviteTitle')" max-width="28rem" @close="showInvite = false">
      <p class="text-sm text-muted-foreground">{{ $t('interviews.session.inviteHelp') }}</p>
      <div v-if="tutoring.sessionStatus.value?.ticket" class="mt-4 flex justify-center"><QrCodeDisplay :value="tutoring.sessionStatus.value.ticket" :size="240" /></div>
      <textarea readonly :value="tutoring.sessionStatus.value?.ticket ?? ''" rows="4" class="input mt-4 resize-none font-mono text-xs" @focus="($event.target as HTMLTextAreaElement).select()" />
    </AppModal>

    <AppModal :open="showChat" :title="$t('interviews.session.chatTitle')" max-width="28rem" @close="showChat = false">
      <div class="max-h-80 space-y-2 overflow-y-auto">
        <p v-if="tutoring.chatMessages.value.length === 0" class="py-8 text-center text-sm text-muted-foreground">{{ $t('interviews.session.chatEmpty') }}</p>
        <div v-for="(message, index) in tutoring.chatMessages.value" :key="index" class="rounded-lg bg-muted p-3">
          <p class="text-xs font-semibold text-primary">{{ message.sender_name || (message.sender === 'self' ? 'You' : message.sender.slice(0, 8)) }}</p>
          <p class="mt-1 text-sm text-foreground">{{ message.text }}</p>
        </div>
      </div>
      <form class="mt-4 flex gap-2" @submit.prevent="sendChat"><input v-model="chatInput" class="input flex-1" :placeholder="$t('interviews.session.chatPlaceholder')" /><AppButton type="submit" size="sm" :disabled="!chatInput.trim()">{{ $t('interviews.common.send') }}</AppButton></form>
    </AppModal>

    <AppModal :open="showAudioDevices" :title="$t('interviews.session.audioTitle')" max-width="30rem" @close="showAudioDevices = false">
      <div class="space-y-4">
        <label class="block"><span class="label text-xs text-muted-foreground">{{ $t('interviews.session.microphone') }}</span><select v-model="selectedMic" class="input"><option v-for="device in availableDevices?.audio_inputs ?? []" :key="device.id" :value="device.id">{{ device.name || device.id }}{{ device.is_default ? $t('interviews.session.defaultDevice') : '' }}</option></select></label>
        <label class="block"><span class="label text-xs text-muted-foreground">{{ $t('interviews.session.speakerOutput') }}</span><select v-model="selectedOutput" class="input"><option v-for="device in availableDevices?.audio_outputs ?? []" :key="device.id" :value="device.id">{{ device.name || device.id }}{{ device.is_default ? $t('interviews.session.defaultDevice') : '' }}</option></select></label>
        <p class="text-xs leading-relaxed text-muted-foreground">{{ $t('interviews.session.deviceHelp') }}</p>
      </div>
      <template #footer><div class="flex justify-end gap-2"><AppButton variant="outline" @click="showAudioDevices = false">{{ $t('interviews.common.cancel') }}</AppButton><AppButton @click="applyAudioDevices">{{ $t('interviews.common.apply') }}</AppButton></div></template>
    </AppModal>

    <LiveDiagnosticsModal :open="showDiagnostics" :diagnostics="diagnostics" @close="showDiagnostics = false" @refresh="openDebug" />

    <AppModal :open="showSentinelDebug" :title="$t('interviews.session.sentinelDebugTitle')" max-width="48rem" @close="showSentinelDebug = false">
      <p class="text-sm leading-relaxed text-muted-foreground">{{ $t('interviews.session.sentinelDebugHelp') }}</p>
      <div class="mt-4 grid gap-3 sm:grid-cols-3">
        <div class="rounded-lg bg-muted p-3"><p class="text-[0.65rem] uppercase text-muted-foreground">{{ $t('interviews.session.integrity') }}</p><p class="mt-1 font-mono text-lg text-foreground">{{ Math.round(sentinel.integrityScore.value * 100) }}%</p></div>
        <div class="rounded-lg bg-muted p-3"><p class="text-[0.65rem] uppercase text-muted-foreground">{{ $t('interviews.session.consistency') }}</p><p class="mt-1 font-mono text-lg text-foreground">{{ Math.round(sentinel.consistencyScore.value * 100) }}%</p></div>
        <div class="rounded-lg bg-muted p-3"><p class="text-[0.65rem] uppercase text-muted-foreground">{{ $t('interviews.session.cameraSignals') }}</p><p class="mt-1 text-sm text-foreground">{{ sentinel.cameraOptedIn.value ? $t('interviews.session.consented') : $t('interviews.session.off') }}</p></div>
      </div>
      <pre class="mt-4 max-h-[48vh] overflow-auto rounded-lg bg-muted p-3 font-mono text-xs text-foreground whitespace-pre-wrap break-all select-all">{{ JSON.stringify(sentinel.debug, null, 2) }}</pre>
    </AppModal>

    <SentinelLiveIndicator />
  </div>
  <div v-else class="grid min-h-80 place-items-center"><p class="text-sm text-muted-foreground">{{ loading ? $t('interviews.common.loading') : $t('interviews.common.notFound') }}</p></div>
</template>

<style scoped>
.consent-row {
  display: flex;
  align-items: flex-start;
  gap: 0.75rem;
  border: 1px solid var(--app-border);
  border-radius: 0.75rem;
  padding: 0.8rem;
  cursor: pointer;
}

.consent-row input {
  margin-top: 0.2rem;
}

.consent-row span {
  display: grid;
  gap: 0.2rem;
}

.consent-row strong {
  color: var(--app-foreground);
  font-size: 0.875rem;
  font-weight: 600;
}

.consent-row small {
  color: var(--app-muted-foreground);
  font-size: 0.75rem;
  line-height: 1.45;
}
</style>
