// The candidate's integrity check for an interview held in Alexandria.
//
// The organisation asks for it in the invitation; the candidate says yes or
// no in the waiting room. A yes starts Sentinel for the call on this device
// (purpose `interview`, so nothing is staged as appeal evidence and nothing
// suspends the call). Leaving the call ends the check and sends the
// organisation a signed summary — outcome, score, flag counts — built by the
// backend from the stored session, never from anything held here.
//
// State is module-level because the check outlives the waiting room: it
// starts there and ends in the call screen.
import { readonly, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { useSentinel } from '@/composables/useSentinel'

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
}

interface UnsentCheck {
  interviewId: string
  cloud: string
  sessionId: string
}

const running = ref<RunningCheck | null>(null)
const unsent = ref<UnsentCheck | null>(null)
const sendError = ref<string | null>(null)

export function useInterviewCheck() {
  const { invoke } = useLocalApi()
  const sentinel = useSentinel()

  /** Start the check for this interview, before joining the call. */
  async function begin(check: RunningCheck) {
    await sentinel.start(null, check.camera, 'interview')
    running.value = check
  }

  /** Joining failed: stop without sending anything. */
  async function abandon() {
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
      sendError.value = cause instanceof Error ? cause.message : String(cause)
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
    begin,
    abandon,
    finish,
    send,
    sent,
  }
}
