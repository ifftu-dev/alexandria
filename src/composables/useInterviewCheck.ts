// The candidate's integrity check for an interview held in Alexandria.
//
// The organisation asks for it in the invitation; the candidate says yes or
// no in the waiting room. A yes starts Sentinel for the call on this device
// (purpose `interview`, so nothing is staged as appeal evidence and nothing
// suspends the call). Leaving the call ends the check and sends the
// organisation a signed summary — outcome, score, flag counts — built by the
// backend from the stored session, never from anything held here.
//
// With voice checks also allowed, the check listens to the call's own
// microphone stream for voice onsets (never recording or transcribing) and,
// while the call runs, sends the panel each answer's timing against the
// candidate's own earlier answers and any second voice it heard.
//
// When the panel marks an unscripted live segment as done, the check records
// it as a verification result: the `live` verification rung's evidence.
//
// State is module-level because the check outlives the waiting room: it
// starts there and ends in the call screen.
import { readonly, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { liveSegment, markedQuestion, useInterviewSignals } from '@/composables/useInterviewSignals'
import { useSentinel } from '@/composables/useSentinel'
import { createInterviewFeed, type InterviewFeed } from '@/utils/sentinel/interviewFeed'
import type { InterviewSignalMessage } from '@/types'
import { errorMessage } from '@/utils/errorMessage'

export interface IntegrityFlag {
  flag: string
  severity: 'info' | 'warning' | 'critical'
  count: number
}

/** What the candidate signed and sent, as they keep it. */
export interface SentIntegrityReport {
  integrity_session_id: string
  interview_id: string
  organization: string
  sent_at: string
  report: {
    outcome: 'clear' | 'flagged'
    score: number | null
    started_at: number
    ended_at: number
    snapshots: number
    camera: boolean
    camera_coverage: number
    flags: IntegrityFlag[]
  }
}

interface RunningCheck {
  interviewId: string
  cloud: string
  camera: boolean
  /** Voice checks allowed: listen for voice onsets and feed the panel. */
  microphone?: boolean
}

interface UnsentCheck {
  interviewId: string
  cloud: string
  sessionId: string
}

const running = ref<RunningCheck | null>(null)
const unsent = ref<UnsentCheck | null>(null)
const sendError = ref<string | null>(null)
/** Voice checks are listening on the call's microphone. */
const voiceChecks = ref(false)
let stopSignals: Array<() => void> = []
let stopVoice: Array<() => void> = []

export function useInterviewCheck() {
  const { invoke } = useLocalApi()
  const sentinel = useSentinel()
  const signals = useInterviewSignals()

  /** Start the check for this interview, before joining the call. */
  async function begin(check: RunningCheck) {
    await sentinel.start(null, check.camera, 'interview', { microphone: check.microphone === true })
    running.value = check
  }

  /** The panel marked a live segment done: record it in the running check. */
  async function recordLiveSegment(check: RunningCheck, sessionId: string, segmentId: string, message: InterviewSignalMessage) {
    if (running.value !== check) return
    try {
      await invoke('hiring_interview_live_segment', {
        interviewId: check.interviewId,
        integritySessionId: sessionId,
        segmentId,
        markedBy: message.sender,
        markedByName: message.sender_name,
      })
    } catch (cause) {
      console.warn('[interview] live segment not recorded', errorMessage(cause))
    }
  }

  /**
   * Once in the call: listen for the panel's live-segment marks, and start
   * voice checks and the panel's live feed when the candidate allowed them.
   * The backend refuses voice checks unless the session recorded that
   * consent, and live segments unless the interview is held in Alexandria.
   */
  async function listen() {
    const check = running.value
    const sessionId = sentinel.getSessionId()
    if (!check || !sessionId || stopSignals.length) return
    let feed: InterviewFeed | null = check.microphone
      ? createInterviewFeed({
          responses: sentinel.responses,
          send: signal => { void signals.send(signal).catch(() => undefined) },
          now: () => Date.now(),
        })
      : null
    stopSignals = [await signals.onSignal(message => {
      const segment = liveSegment(message.signal)
      if (segment) {
        void recordLiveSegment(check, sessionId, segment, message)
        return
      }
      const question = markedQuestion(message.signal)
      if (question) feed?.question(question)
    })]
    if (!feed || voiceChecks.value) return
    stopVoice = await Promise.all([
      signals.onVoiceOnset(onset => feed?.voice(onset)),
      signals.onAudioLevel(level => feed?.remoteLevel(level.output_level)),
    ])
    try {
      await invoke('sentinel_voice_listen', { sessionId })
      voiceChecks.value = true
    } catch (cause) {
      console.warn('[interview] voice checks did not start', errorMessage(cause))
      feed = null
      await stopVoiceChecks()
    }
  }

  async function stopVoiceChecks() {
    for (const unlisten of stopVoice) unlisten()
    stopVoice = []
    if (!voiceChecks.value) return
    voiceChecks.value = false
    await invoke('sentinel_voice_stop').catch(() => undefined)
  }

  /** Stop listening to the call: live-segment marks, voice checks and the feed. */
  async function quiet() {
    for (const unlisten of stopSignals) unlisten()
    stopSignals = []
    await stopVoiceChecks()
  }

  /** Joining failed: stop without sending anything. */
  async function abandon() {
    await quiet()
    if (!running.value) return
    running.value = null
    if (sentinel.isActive.value) await sentinel.stop()
  }

  async function send(): Promise<SentIntegrityReport | null> {
    const pending = unsent.value
    if (!pending) return null
    sendError.value = null
    try {
      const sent = await invoke<SentIntegrityReport>('hiring_interview_integrity_send', {
        interviewId: pending.interviewId,
        integritySessionId: pending.sessionId,
      })
      unsent.value = null
      return sent
    } catch (cause) {
      sendError.value = errorMessage(cause)
      return null
    }
  }

  /**
   * End the check and send the result. Returns where the candidate goes next:
   * back to the interview's waiting room, which shows what was shared (or why
   * it could not be, with a way to try again). `null` when no check ran.
   */
  async function finish(): Promise<string | null> {
    const check = running.value
    if (!check) return null
    running.value = null
    await quiet()
    const sessionId = sentinel.isActive.value ? await sentinel.stop() : null
    if (sessionId) {
      unsent.value = { interviewId: check.interviewId, cloud: check.cloud, sessionId }
      await send()
    }
    return `/interview-room/${check.interviewId}?cloud=${encodeURIComponent(check.cloud)}`
  }

  /** The results already sent for one interview. */
  function sent(interviewId: string) {
    return invoke<SentIntegrityReport[]>('hiring_interview_integrity_sent', { interviewId })
  }

  return {
    running: readonly(running),
    unsent: readonly(unsent),
    sendError: readonly(sendError),
    voiceChecks: readonly(voiceChecks),
    begin,
    listen,
    abandon,
    finish,
    send,
    sent,
  }
}
