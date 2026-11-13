// The interview signal feed on a live call: question markers from the
// panel, answer timings and second voices from the candidate's app, and the
// call events the candidate's side needs to time answers (voice onsets from
// the voice checks, the far end's playback level). See
// `utils/sentinel/interviewFeed.ts` for the candidate's side and
// `tutoring::interview_signal` for the wire.
import { listen } from '@tauri-apps/api/event'
import type { AudioLevelEvent, InterviewSignal, InterviewSignalMessage, VoiceOnsetEvent } from '@/types'
import { useLocalApi } from '@/composables/useLocalApi'

type Unlisten = () => void

function on<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
  return listen<T>(event, e => handler(e.payload))
}

export function useInterviewSignals() {
  const { invoke } = useLocalApi()
  return {
    /** Broadcast a signal to everyone else in the call. */
    send: (signal: InterviewSignal) => invoke<void>('tutoring_send_interview_signal', { signal }),
    /** Signals from the other participants. */
    onSignal: (handler: (message: InterviewSignalMessage) => void) => on('tutoring:interview-signal', handler),
    /** Voice onsets the voice checks heard on this device's microphone. */
    onVoiceOnset: (handler: (onset: VoiceOnsetEvent) => void) => on('tutoring:voice-onset', handler),
    /** This device's microphone and playback levels. */
    onAudioLevel: (handler: (level: AudioLevelEvent) => void) => on('tutoring:audio-level', handler),
  }
}

/** The question id in a panel's marker, or null for any other signal. */
export function markedQuestion(signal: InterviewSignal): string | null {
  return typeof signal === 'object' && 'question' in signal ? signal.question.question_id : null
}

/** The segment id in a panel's live-segment mark, or null for any other signal. */
export function liveSegment(signal: InterviewSignal): string | null {
  return typeof signal === 'object' && 'live_segment' in signal ? signal.live_segment.segment_id : null
}
